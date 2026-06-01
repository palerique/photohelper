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
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct DedupeArgs {
    /// Exit non-zero if any per-photo error occurs during the embed phase.
    #[arg(long, default_value_t = false)]
    pub(crate) strict: bool,

    /// Cosine-similarity threshold: photos with sim >= this are considered duplicates.
    /// Valid range: (0.0, 1.0].
    #[arg(
        long,
        default_value_t = 0.85_f32,
        value_parser = parse_similarity_threshold
    )]
    pub(crate) similarity_threshold: f32,
}

pub fn parse_similarity_threshold(s: &str) -> Result<f32, String> {
    let v: f32 = s.parse().map_err(|_| format!("'{s}' is not a valid f32"))?;
    if !v.is_finite() || v <= 0.0 || v > 1.0 {
        return Err(format!(
            "similarity threshold must be in (0.0, 1.0], got {v}"
        ));
    }
    Ok(v)
}

/// Atomic counters for the dedup summary (Phase 1 per-photo + Phase 2 cluster).
struct DedupeStats {
    /// Total photos walked from unembedded_rows.
    walked: AtomicU64,
    /// Successfully embedded and persisted.
    embedded: AtomicU64,
    derive_failed: AtomicU64,
    decode_failed: AtomicU64,
    infer_failed: AtomicU64,
    file_missing: AtomicU64,
    content_changed: AtomicU64,
    /// Benign inter-process race: another writer embedded first (not a real error).
    already_embedded: AtomicU64,
    /// Real catalog failure (disk full, FK violation, lock timeout).
    catalog_insert_failed: AtomicU64,
    /// Phase-2 only: corrupt embeddings dropped during deserialization.
    deserialize_failed: AtomicU64,
    /// Phase-2 only: dup_cluster write failure (does not trigger --strict).
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
            already_embedded: AtomicU64::new(0),
            catalog_insert_failed: AtomicU64::new(0),
            deserialize_failed: AtomicU64::new(0),
            cluster_write_failed: AtomicU64::new(0),
        }
    }

    fn summary_line(&self, clusters_found: usize, singletons: usize) -> String {
        format!(
            "walked: {}, embedded: {}, derive-failed: {}, decode-failed: {}, \
             infer-failed: {}, file-missing: {}, content-changed: {}, \
             already-embedded: {}, catalog-insert-failed: {}, \
             deserialize-failed: {}, cluster-write-failed: {}, \
             clusters-found: {clusters_found}, singletons: {singletons}",
            self.walked.load(Ordering::Relaxed),
            self.embedded.load(Ordering::Relaxed),
            self.derive_failed.load(Ordering::Relaxed),
            self.decode_failed.load(Ordering::Relaxed),
            self.infer_failed.load(Ordering::Relaxed),
            self.file_missing.load(Ordering::Relaxed),
            self.content_changed.load(Ordering::Relaxed),
            self.already_embedded.load(Ordering::Relaxed),
            self.catalog_insert_failed.load(Ordering::Relaxed),
            self.deserialize_failed.load(Ordering::Relaxed),
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
                // Benign inter-process race: another writer won between unembedded_rows and
                // insert_embedding. Not a data error; excluded from --strict exit check.
                tracing::warn!(
                    path = %source_path.display(),
                    "insert_embedding returned AlreadyEmbedded — inter-process race?"
                );
                stats.already_embedded.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "catalog insert failed");
                stats.catalog_insert_failed.fetch_add(1, Ordering::Relaxed);
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
    if let Err(e) = heartbeat_handle.join() {
        tracing::error!("heartbeat thread panicked: {:?}", e);
    }

    // ── Phase 2 — Cluster (sequential, after Phase 1 completes) ───────────────
    let all_embeddings = catalog
        .all_embeddings_for_model(CLIP_MODEL_SLUG)
        .with_context(|| "loading embeddings for clustering")?;

    if all_embeddings.is_empty() {
        tracing::info!(
            model = CLIP_MODEL_SLUG,
            "no embeddings found for model; skipping clustering phase"
        );
    }
    let (clusters_found, singletons) = if all_embeddings.len() < 2 {
        // 0 or 1 embedding: nothing to cluster.
        (0_usize, all_embeddings.len())
    } else {
        // Deserialize raw bytes to ImageEmbedding for clustering.
        // `insert_embedding` enforces `dim*4 == bytes.len()` at write time; we re-check
        // here to surface any catalog corruption rather than silently misbehaving.
        let photo_embeddings: Vec<(PhotoId, ImageEmbedding)> = all_embeddings
            .into_iter()
            .filter_map(|(pid, bytes, dim)| {
                if bytes.len() != dim * 4 {
                    tracing::error!(
                        photo_id = %pid, stored_dim = dim, byte_len = bytes.len(),
                        "embedding dim/byte-length mismatch (catalog corruption?); skipping"
                    );
                    stats.deserialize_failed.fetch_add(1, Ordering::Relaxed);
                    return None;
                }
                match ImageEmbedding::from_f32_le_bytes(&bytes) {
                    Ok(emb) => Some((pid, emb)),
                    Err(e) => {
                        tracing::warn!(error = %e, "skipping corrupt embedding during clustering");
                        stats.deserialize_failed.fetch_add(1, Ordering::Relaxed);
                        None
                    }
                }
            })
            .collect();

        if photo_embeddings.is_empty() {
            (0, 0)
        } else {
            let result = threshold_cluster(&photo_embeddings, args.similarity_threshold);
            let clusters_found = result.cluster_count;
            let singletons = result.singleton_count;
            let clustered_at = unix_now();

            if let Err(e) = catalog.insert_dup_clusters(
                &result.assignments,
                CLIP_MODEL_SLUG,
                args.similarity_threshold,
                clustered_at,
            ) {
                tracing::warn!(error = %e, "batch cluster write failed");
                stats
                    .cluster_write_failed
                    .fetch_add(result.assignments.len() as u64, Ordering::Relaxed);
            }
            (clusters_found, singletons)
        }
    };

    eprintln!("{}", stats.summary_line(clusters_found, singletons));

    // Exit code logic.
    // Exit code logic.
    // `already_embedded` is a benign race (excluded from strict); all Phase 1 per-photo failures
    // (derive, decode, infer, file_missing, content_changed) AND Phase 2 cluster write failures
    // are real errors that prevent data from being persisted, triggering strict mode.
    let all_errors = stats.derive_failed.load(Ordering::Relaxed)
        + stats.decode_failed.load(Ordering::Relaxed)
        + stats.infer_failed.load(Ordering::Relaxed)
        + stats.file_missing.load(Ordering::Relaxed)
        + stats.content_changed.load(Ordering::Relaxed)
        + stats.catalog_insert_failed.load(Ordering::Relaxed)
        + stats.cluster_write_failed.load(Ordering::Relaxed)
        + stats.deserialize_failed.load(Ordering::Relaxed);

    if args.strict && all_errors > 0 {
        return Ok(exit_code::EX_STRICT_FAIL);
    }
    let walked = stats.walked.load(Ordering::Relaxed);
    if walked > 0
        && (stats.embedded.load(Ordering::Relaxed) + stats.already_embedded.load(Ordering::Relaxed))
            == 0
        && all_errors == 0
    {
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
            match embeddings[i].1.cosine_similarity(&embeddings[j].1) {
                Ok(sim) if sim >= threshold => {
                    uf_union(&mut parent, &mut rank, i, j);
                }
                Ok(_) => {} // below threshold
                Err(e) => {
                    // Dim mismatch within a single model slug indicates catalog corruption
                    // (all embeddings from the same model must have equal dimension).
                    tracing::error!(
                        i_pid = %embeddings[i].0, j_pid = %embeddings[j].0, error = %e,
                        "cosine_similarity failed — embedding dim mismatch (DB corruption?)"
                    );
                }
            }
        }
    }

    let roots: Vec<usize> = (0..n).map(|i| uf_find(&mut parent, i)).collect();
    let mut root_to_id: std::collections::HashMap<usize, i64> = std::collections::HashMap::new();
    let mut next_id: i64 = 0;
    let mut id_counts: std::collections::HashMap<i64, usize> = std::collections::HashMap::new();

    let mut assignments: Vec<(PhotoId, i64)> = Vec::with_capacity(embeddings.len());
    for ((photo_id, _), &root) in embeddings.iter().zip(roots.iter()) {
        let cluster_id = *root_to_id.entry(root).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        *id_counts.entry(cluster_id).or_insert(0) += 1;
        assignments.push((*photo_id, cluster_id));
    }

    // Filter out singletons (size == 1)
    assignments.retain(|(_, cluster_id)| id_counts.get(cluster_id).unwrap_or(&0) > &1);

    let singleton_count = id_counts.values().filter(|&&c| c == 1).count();
    let cluster_count = id_counts.len();

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

#[cfg(test)]
mod threshold_cluster_tests {
    use photohelper_ai::ImageEmbedding;
    use photohelper_core::model::PhotoId;

    use super::threshold_cluster;

    fn make_id(seed: u8) -> PhotoId {
        // Derive from a deterministic tiny fixture path by writing a tmp file.
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join(format!("{seed}.bin"));
        std::fs::write(&p, vec![seed; 64 * 1024]).expect("write fixture");
        PhotoId::derive(&p).expect("derive")
    }

    fn unit_vec(dim: usize, hot: usize) -> ImageEmbedding {
        let mut v = vec![0.0_f32; dim];
        v[hot] = 1.0;
        ImageEmbedding::from_raw(&v).expect("unit vector")
    }

    fn identical_vecs(dim: usize) -> ImageEmbedding {
        // All-equal normalized vector: each element = 1/sqrt(dim).
        let val = 1.0_f32 / (dim as f32).sqrt();
        let v = vec![val; dim];
        ImageEmbedding::from_raw(&v).expect("uniform vector")
    }

    #[test]
    fn empty_input_returns_zero_clusters() {
        let result = threshold_cluster(&[], 0.95);
        assert_eq!(result.cluster_count, 0);
        assert_eq!(result.singleton_count, 0);
        assert!(result.assignments.is_empty());
    }

    #[test]
    fn single_element_is_one_singleton() {
        let id = make_id(1);
        let emb = unit_vec(4, 0);
        let result = threshold_cluster(&[(id, emb)], 0.95);
        assert_eq!(result.cluster_count, 1);
        assert_eq!(result.singleton_count, 1);
    }

    #[test]
    fn two_orthogonal_vectors_are_two_singletons() {
        // e1 = [1,0,0,0], e2 = [0,1,0,0]: cosine_similarity = 0.0 < any threshold.
        let id1 = make_id(10);
        let id2 = make_id(11);
        let e1 = unit_vec(4, 0);
        let e2 = unit_vec(4, 1);
        let result = threshold_cluster(&[(id1, e1), (id2, e2)], 0.95);
        assert_eq!(result.cluster_count, 2);
        assert_eq!(result.singleton_count, 2);
    }

    #[test]
    fn two_identical_vectors_at_threshold_1_0_form_one_cluster() {
        let id1 = make_id(20);
        let id2 = make_id(21);
        let e1 = identical_vecs(4);
        let e2 = identical_vecs(4);
        // cosine_similarity of identical unit vectors = 1.0; threshold=1.0 means exactly 1.0 qualifies.
        let result = threshold_cluster(&[(id1, e1), (id2, e2)], 1.0);
        assert_eq!(result.cluster_count, 1, "identical vectors should cluster");
        assert_eq!(result.singleton_count, 0);
    }

    #[test]
    fn two_orthogonal_vectors_at_threshold_1_0_are_singletons() {
        // cosine_similarity = 0.0 < 1.0 → should NOT cluster.
        let id1 = make_id(30);
        let id2 = make_id(31);
        let e1 = unit_vec(4, 0);
        let e2 = unit_vec(4, 1);
        let result = threshold_cluster(&[(id1, e1), (id2, e2)], 1.0);
        assert_eq!(result.cluster_count, 2);
        assert_eq!(result.singleton_count, 2);
    }

    #[test]
    fn three_elements_partial_cluster() {
        // e1 and e2 identical → same cluster; e3 orthogonal → singleton.
        let id1 = make_id(40);
        let id2 = make_id(41);
        let id3 = make_id(42);
        let e1 = identical_vecs(4);
        let e2 = identical_vecs(4);
        let e3 = unit_vec(4, 3);
        let result = threshold_cluster(&[(id1, e1), (id2, e2), (id3, e3)], 0.95);
        assert_eq!(
            result.cluster_count, 2,
            "one pair-cluster + one singleton = 2 clusters"
        );
        assert_eq!(result.singleton_count, 1);
    }
}
