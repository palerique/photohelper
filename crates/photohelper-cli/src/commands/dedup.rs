//! `photohelper dedup` — duplicate-detection via CLIP ViT-B/32 embeddings.
//!
//! Pipeline:
//! Phase 1 — Embed: walk unembedded photos → decode RGB → CLIP embed → store.
//! Phase 2 — Cluster: load all embeddings → cosine-similarity union-find → write clusters.
//!
//! See `docs/plans/session-05.md §D3` for the full pipeline spec.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::Context as _;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

use photohelper_ai::{CLIP_MODEL_SLUG, ImageEmbedding, MobileClip, VerifiedModelBytes};
use photohelper_catalog::{Catalog, InsertEmbeddingOutcome};
use photohelper_core::model::PhotoId;
use photohelper_raw::decode::read_raw_rgb;

use crate::heartbeat::{HeartbeatStop, heartbeat_interval, run_heartbeat_loop};
use crate::{Cli, exit_code};

/// Clap args for `photohelper dedup`.
#[derive(clap::Args, Debug)]
pub(crate) struct DedupeArgs {
    /// Exit non-zero if any per-photo error occurs during the embed phase.
    #[arg(long, default_value_t = false)]
    strict: bool,

    /// Cosine-similarity threshold: photos with sim >= this are considered duplicates.
    /// Valid range: (0.0, 1.0].
    #[arg(
        long,
        default_value_t = 0.95_f32,
        value_parser = parse_similarity_threshold
    )]
    similarity_threshold: f32,
}

fn parse_similarity_threshold(s: &str) -> Result<f32, String> {
    let v: f32 = s.parse().map_err(|_| format!("'{s}' is not a valid f32"))?;
    if !v.is_finite() || v <= 0.0 || v > 1.0 {
        return Err(format!(
            "similarity threshold must be in (0.0, 1.0], got {v}"
        ));
    }
    Ok(v)
}

/// Atomic counters for the Phase 1 embed summary (9 concurrent fields).
struct DedupeStats {
    walked: AtomicU64,
    embedded: AtomicU64,
    derive_failed: AtomicU64,
    decode_failed: AtomicU64,
    infer_failed: AtomicU64,
    file_missing: AtomicU64,
    content_changed: AtomicU64,
    catalog_inconsistency: AtomicU64,
    cluster_write_failed: AtomicU64,
}

impl DedupeStats {
    fn new() -> Self {
        Self {
            walked: AtomicU64::new(0),
            embedded: AtomicU64::new(0),
            derive_failed: AtomicU64::new(0),
            decode_failed: AtomicU64::new(0),
            infer_failed: AtomicU64::new(0),
            file_missing: AtomicU64::new(0),
            content_changed: AtomicU64::new(0),
            catalog_inconsistency: AtomicU64::new(0),
            cluster_write_failed: AtomicU64::new(0),
        }
    }

    fn summary_line(&self, clusters_found: usize, singletons: usize) -> String {
        format!(
            "walked: {}, embedded: {}, derive-failed: {}, decode-failed: {}, \
             infer-failed: {}, file-missing: {}, content-changed: {}, \
             catalog-inconsistency: {}, cluster-write-failed: {}, \
             clusters-found: {clusters_found}, singletons: {singletons}",
            self.walked.load(Ordering::Relaxed),
            self.embedded.load(Ordering::Relaxed),
            self.derive_failed.load(Ordering::Relaxed),
            self.decode_failed.load(Ordering::Relaxed),
            self.infer_failed.load(Ordering::Relaxed),
            self.file_missing.load(Ordering::Relaxed),
            self.content_changed.load(Ordering::Relaxed),
            self.catalog_inconsistency.load(Ordering::Relaxed),
            self.cluster_write_failed.load(Ordering::Relaxed),
        )
    }
}

/// Result of the cosine-similarity threshold clustering pass.
struct ClusteringResult {
    /// Cluster assignment per photo (index → cluster_id).
    assignments: Vec<(PhotoId, i64)>,
    cluster_count: usize,
    singleton_count: usize,
}

/// Driver for `photohelper dedup`.
///
/// # Errors
///
/// Returns `Err` only for fatal setup failures (catalog open, model load, thread spawn).
pub fn run_dedup(cli: &Cli, args: &DedupeArgs, model: &VerifiedModelBytes) -> anyhow::Result<u8> {
    let catalog_path = cli.catalog.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".photohelper")
            .join("catalog.db")
    });

    let catalog = Arc::new(
        Catalog::open(&catalog_path, cli.catalog_lock_timeout_seconds)
            .with_context(|| format!("opening catalog at {}", catalog_path.display()))?,
    );

    let clip = Arc::new(MobileClip::new(model));

    let rows = catalog
        .unembedded_rows(CLIP_MODEL_SLUG)
        .with_context(|| "querying unembedded rows")?;

    if rows.is_empty() {
        eprintln!("{}", DedupeStats::new().summary_line(0, 0));
        return Ok(0);
    }

    let stats = Arc::new(DedupeStats::new());

    // ── Phase 1 — Embed ───────────────────────────────────────────────────────
    // Heartbeat thread (dedup is the 3rd consumer → TD-016 closed).
    let stop = Arc::new(HeartbeatStop::new());
    let heartbeat_handle = {
        let stats = Arc::clone(&stats);
        let stop = Arc::clone(&stop);
        let interval = heartbeat_interval();
        std::thread::Builder::new()
            .name("ph-heartbeat".into())
            .spawn(move || {
                run_heartbeat_loop(&stop, interval, || {
                    eprintln!(
                        "[heartbeat] walked {}, embedded {}, decode-failed {}",
                        stats.walked.load(Ordering::Relaxed),
                        stats.embedded.load(Ordering::Relaxed),
                        stats.decode_failed.load(Ordering::Relaxed),
                    );
                });
            })
            .context("spawning heartbeat thread")?
    };

    rows.into_par_iter().for_each(|row| {
        stats.walked.fetch_add(1, Ordering::Relaxed);
        let source_path = row.source_path().to_path_buf();

        // Step 1: existence pre-check.
        if !source_path.exists() {
            tracing::warn!(path = %source_path.display(), "file missing since ingest; skipping");
            stats.file_missing.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Step 2: re-derive PhotoId (content-change detection).
        let current_id = match PhotoId::derive(&source_path) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "derive failed; skipping");
                stats.derive_failed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        if current_id != row.photo_id() {
            tracing::warn!(path = %source_path.display(), "content changed since ingest; skipping");
            stats.content_changed.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Step 3: decode to 8-bit sRGB.
        let rgb = match read_raw_rgb(&source_path) {
            Ok(img) => img,
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "decode failed");
                stats.decode_failed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // Step 4: CLIP embedding.
        let embedding = match clip.embed(&rgb, &source_path) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "CLIP embed failed");
                stats.infer_failed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // Step 5: persist embedding.
        let embedded_at = unix_now();
        let bytes = embedding.as_f32_le_bytes();
        match catalog.insert_embedding(
            row.photo_id(),
            CLIP_MODEL_SLUG,
            &bytes,
            embedding.dim(),
            embedded_at,
        ) {
            Ok(InsertEmbeddingOutcome::Inserted) => {
                stats.embedded.fetch_add(1, Ordering::Relaxed);
            }
            Ok(InsertEmbeddingOutcome::AlreadyEmbedded) => {
                // Already embedded since unembedded_rows was queried (inter-process race).
                tracing::warn!(
                    path = %source_path.display(),
                    "insert_embedding returned AlreadyEmbedded for an unembedded row \
                     — inter-process race?"
                );
                stats.catalog_inconsistency.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "catalog insert failed");
                stats.catalog_inconsistency.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Heartbeat shutdown (same pattern as ingest + cull).
    if heartbeat_handle.is_finished() {
        tracing::warn!(
            "heartbeat thread died before end-of-dedup; liveness signal was unavailable"
        );
    }
    stop.signal();
    let _ = heartbeat_handle.join();

    // ── Phase 2 — Cluster (sequential, after Phase 1 completes) ───────────────
    let all_embeddings = catalog
        .all_embeddings_for_model(CLIP_MODEL_SLUG)
        .with_context(|| "loading embeddings for clustering")?;

    let (clusters_found, singletons) = if all_embeddings.len() < 2 {
        // 0 or 1 embedding: nothing to cluster.
        (0_usize, all_embeddings.len())
    } else {
        // Deserialize raw bytes to ImageEmbedding for clustering.
        let photo_embeddings: Vec<(PhotoId, ImageEmbedding)> = all_embeddings
            .into_iter()
            .filter_map(
                |(pid, bytes, _dim)| match ImageEmbedding::from_f32_le_bytes(&bytes) {
                    Ok(emb) => Some((pid, emb)),
                    Err(e) => {
                        tracing::warn!(error = %e, "skipping corrupt embedding during clustering");
                        None
                    }
                },
            )
            .collect();

        if photo_embeddings.is_empty() {
            (0, 0)
        } else {
            let result = threshold_cluster(&photo_embeddings, args.similarity_threshold);
            let clusters_found = result.cluster_count;
            let singletons = result.singleton_count;
            let clustered_at = unix_now();

            for (photo_id, cluster_id) in &result.assignments {
                if let Err(e) = catalog.insert_dup_cluster(
                    *photo_id,
                    CLIP_MODEL_SLUG,
                    *cluster_id,
                    args.similarity_threshold,
                    clustered_at,
                ) {
                    tracing::warn!(error = %e, "cluster write failed");
                    stats.cluster_write_failed.fetch_add(1, Ordering::Relaxed);
                }
            }
            (clusters_found, singletons)
        }
    };

    eprintln!("{}", stats.summary_line(clusters_found, singletons));

    // Exit code logic.
    let all_per_photo_errors = stats.derive_failed.load(Ordering::Relaxed)
        + stats.decode_failed.load(Ordering::Relaxed)
        + stats.infer_failed.load(Ordering::Relaxed)
        + stats.file_missing.load(Ordering::Relaxed)
        + stats.content_changed.load(Ordering::Relaxed)
        + stats.catalog_inconsistency.load(Ordering::Relaxed);

    // Note: cluster_write_failed is a Phase-2 error; it does NOT trigger EX_STRICT_FAIL.
    if args.strict && all_per_photo_errors > 0 {
        return Ok(exit_code::EX_STRICT_FAIL);
    }
    let walked = stats.walked.load(Ordering::Relaxed);
    if walked > 0 && stats.embedded.load(Ordering::Relaxed) == 0 && all_per_photo_errors == 0 {
        return Ok(exit_code::EX_USAGE);
    }
    Ok(0)
}

// Union-find helpers at module scope (avoids "adding items after statements" lint).
// Indexing is safe: parent and rank have length n; all indices are in 0..n.
#[allow(
    clippy::indexing_slicing,
    reason = "union-find: indices are in 0..n by loop invariant; bounds are provably safe"
)]
fn uf_find(parent: &mut [usize], i: usize) -> usize {
    if parent[i] != i {
        parent[i] = uf_find(parent, parent[i]);
    }
    parent[i]
}

#[allow(
    clippy::indexing_slicing,
    reason = "union-find: ra, rb are roots in 0..n; bounds are provably safe"
)]
fn uf_union(parent: &mut [usize], rank: &mut [u8], a: usize, b: usize) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra == rb {
        return;
    }
    match rank[ra].cmp(&rank[rb]) {
        std::cmp::Ordering::Less => parent[ra] = rb,
        std::cmp::Ordering::Greater => parent[rb] = ra,
        std::cmp::Ordering::Equal => {
            parent[rb] = ra;
            rank[ra] += 1;
        }
    }
}

/// Cosine-similarity threshold clustering via union-find with path compression
/// and union-by-rank. O(n² · α(n)) ≈ O(n²).
// TD-017: O(n²) union-find clustering; O(n × dim) memory.
#[allow(
    clippy::indexing_slicing,
    reason = "union-find: i, j in 0..n by loop construction"
)]
fn threshold_cluster(embeddings: &[(PhotoId, ImageEmbedding)], threshold: f32) -> ClusteringResult {
    let n = embeddings.len();
    if n > 5_000 {
        let dim = embeddings.first().map_or(0, |(_, e)| e.dim());
        let mem_mib = n * dim * 4 / 1_048_576;
        eprintln!(
            "dedup: large corpus ({n} photos, ~{mem_mib} MiB embedding memory); \
             clustering O(n²) may take >60s; consider upgrading to DBSCAN (TD-017)."
        );
    }

    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<u8> = vec![0; n];

    for i in 0..n {
        for j in (i + 1)..n {
            // cosine_similarity returns Err only for dim-mismatch (same model → same dim).
            if let Ok(sim) = embeddings[i].1.cosine_similarity(&embeddings[j].1) {
                if sim >= threshold {
                    uf_union(&mut parent, &mut rank, i, j);
                }
            }
        }
    }

    let roots: Vec<usize> = (0..n).map(|i| uf_find(&mut parent, i)).collect();
    let mut root_to_id: std::collections::HashMap<usize, i64> = std::collections::HashMap::new();
    let mut next_id: i64 = 0;
    let mut id_counts: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();

    let assignments: Vec<(PhotoId, i64)> = embeddings
        .iter()
        .zip(roots.iter())
        .map(|((photo_id, _), &root)| {
            let cluster_id = *root_to_id.entry(root).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            *id_counts.entry(cluster_id).or_insert(0) += 1;
            (*photo_id, cluster_id)
        })
        .collect();

    let cluster_count = next_id as usize;
    let singleton_count = id_counts.values().filter(|&&c| c == 1).count();
    ClusteringResult {
        assignments,
        cluster_count,
        singleton_count,
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}
