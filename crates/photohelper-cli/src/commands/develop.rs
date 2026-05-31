//! `photohelper develop` — write Lightroom-compatible XMP sidecars for ingested photos.
//!
//! Pipeline:
//! 1. Load all non-superseded photos + NIMA scores + duplicate cluster IDs from catalog.
//! 2. For each photo: check existence, build XMP settings, call merge_and_write.
//! 3. Print summary; exit 0 (or EX_STRICT_FAIL if --strict and errors > 0).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context as _;

use photohelper_ai::{CLIP_MODEL_SLUG, MODEL_SLUG};
use photohelper_catalog::Catalog;
use photohelper_sidecar::SidecarSettings;
use photohelper_sidecar::conflict::{ConflictStrategy, WriteOutcome, merge_and_write};
use photohelper_sidecar::is_valid_xml_string;
use rayon::prelude::*;

use crate::Cli;
use crate::exit_code;
use crate::heartbeat::{HeartbeatStop, heartbeat_interval, run_heartbeat_loop};

/// Clap args for `photohelper develop`.
#[derive(clap::Args, Debug, Clone)]
// TD-040: Refactor into grouped clap structs
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct DevelopArgs {
    /// Exit non-zero if any per-photo error occurs (file_missing or write errors).
    #[arg(long, default_value_t = false)]
    pub(crate) strict: bool,
    /// Always overwrite existing XMP sidecars (skip conflict check).
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,
    /// Exposure compensation in stops (–5.0 to 5.0).
    #[arg(long)]
    pub(crate) exposure: Option<f32>,
    /// White balance temperature in Kelvin (2000–50000).
    #[arg(long)]
    pub(crate) temp: Option<i32>,
    /// White balance tint (–150 to 150).
    #[arg(long)]
    pub(crate) tint: Option<i32>,
    /// Contrast (–100 to 100).
    #[arg(long)]
    pub(crate) contrast: Option<i32>,
    /// Highlights (–100 to 100).
    #[arg(long)]
    pub(crate) highlights: Option<i32>,
    /// Shadows (–100 to 100).
    #[arg(long)]
    pub(crate) shadows: Option<i32>,
    /// Write Lightroom star ratings (1 to 5) based on NIMA score.
    #[arg(long, default_value_t = false)]
    pub(crate) lr_rating: bool,
    /// Write Lightroom color labels (Red/Green) based on NIMA score.
    #[arg(long, default_value_t = false)]
    pub(crate) lr_label: bool,
    /// Write the exact NIMA score into the Lightroom color label field (e.g. '09.50').
    /// This enables native Lightroom sorting by 'Label Text'.
    #[arg(long, default_value_t = false, conflicts_with = "lr_label")]
    pub(crate) lr_label_score: bool,
    /// Write photohelper keywords (tier/cluster) based on NIMA score and duplicate cluster ID.
    #[arg(long, default_value_t = false)]
    pub(crate) lr_keywords: bool,
    /// Write rating, label, and keywords (convenience shorthand).
    #[arg(long, default_value_t = false)]
    pub(crate) all_lr: bool,
    /// Custom Lightroom color label for 'Red' (NIMA < 4.0)
    #[arg(long, env = "PHOTOHELPER_LR_LABEL_RED", default_value = "Red")]
    pub(crate) lr_label_red: String,
    /// Custom Lightroom color label for 'Green' (NIMA >= 7.0)
    #[arg(long, env = "PHOTOHELPER_LR_LABEL_GREEN", default_value = "Green")]
    pub(crate) lr_label_green: String,
    /// Defers to Lightroom's internal `AutoTone` engine and does not apply numerical adjustments.
    #[arg(long, default_value_t = false)]
    pub(crate) auto_tone: bool,
}

/// AtomicU64 counters used for concurrent updates by Rayon parallel threads and safe cross-thread visibility for the heartbeat loop.
struct DevelopStats {
    walked: AtomicU64,
    /// WriteOutcome::Created — new sidecar file.
    written: AtomicU64,
    /// WriteOutcome::Overwritten — existing sidecar updated.
    updated: AtomicU64,
    /// WriteOutcome::ConflictPreserved — Lightroom/other tool is newer.
    conflict_preserved: AtomicU64,
    /// WriteOutcome::ForcedOverwrite — --force unconditional overwrite.
    force_overwritten: AtomicU64,
    /// Source file no longer exists on disk.
    file_missing: AtomicU64,
    /// merge_and_write returned Err.
    errored: AtomicU64,
}

impl DevelopStats {
    fn new() -> Self {
        Self {
            walked: AtomicU64::new(0),
            written: AtomicU64::new(0),
            updated: AtomicU64::new(0),
            conflict_preserved: AtomicU64::new(0),
            force_overwritten: AtomicU64::new(0),
            file_missing: AtomicU64::new(0),
            errored: AtomicU64::new(0),
        }
    }

    fn summary_line(&self) -> String {
        format!(
            "walked: {}, written: {}, updated: {}, conflict-preserved: {}, \
             force-overwritten: {}, file-missing: {}, errored: {}",
            self.walked.load(Ordering::Relaxed),
            self.written.load(Ordering::Relaxed),
            self.updated.load(Ordering::Relaxed),
            self.conflict_preserved.load(Ordering::Relaxed),
            self.force_overwritten.load(Ordering::Relaxed),
            self.file_missing.load(Ordering::Relaxed),
            self.errored.load(Ordering::Relaxed),
        )
    }
    fn record_outcome(&self, result: Result<WriteOutcome, ()>) {
        self.walked.fetch_add(1, Ordering::Relaxed);
        match result {
            Ok(WriteOutcome::Created) => {
                self.written.fetch_add(1, Ordering::Relaxed);
            }
            Ok(WriteOutcome::Overwritten) => {
                self.updated.fetch_add(1, Ordering::Relaxed);
            }
            Ok(WriteOutcome::ConflictPreserved) => {
                self.conflict_preserved.fetch_add(1, Ordering::Relaxed);
            }
            Ok(WriteOutcome::ForcedOverwrite) => {
                self.force_overwritten.fetch_add(1, Ordering::Relaxed);
            }
            Err(()) => {
                self.errored.fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => { /* forward-compatibility for unknown successful outcomes */ }
        }
    }

    fn record_missing(&self) {
        self.walked.fetch_add(1, Ordering::Relaxed);
        self.file_missing.fetch_add(1, Ordering::Relaxed);
    }
}

/// Driver for `photohelper develop`.
///
/// # Errors
///
/// Returns `Err` only for fatal setup failures (catalog open, photo query, heartbeat spawn, or parameter validation failures).
pub fn run_develop(cli: &Cli, args: &DevelopArgs) -> anyhow::Result<u8> {
    let lr_rating = args.all_lr || args.lr_rating;
    let lr_label = args.all_lr || args.lr_label;
    let lr_label_score = args.lr_label_score; // all_lr does not trigger this, since it conflicts with lr_label
    let lr_keywords = args.all_lr || args.lr_keywords;
    let cancelled = std::sync::atomic::AtomicBool::new(false);

    let red_trimmed = args.lr_label_red.trim();
    let green_trimmed = args.lr_label_green.trim();

    if lr_label && !lr_label_score {
        if red_trimmed.is_empty() {
            anyhow::bail!(
                "invalid custom color label: 'Red' label cannot be empty or whitespace-only"
            );
        }
        if green_trimmed.is_empty() {
            anyhow::bail!(
                "invalid custom color label: 'Green' label cannot be empty or whitespace-only"
            );
        }
        if red_trimmed == green_trimmed {
            anyhow::bail!(
                "invalid custom color label: 'Red' and 'Green' labels must be distinct (got '{red_trimmed}')"
            );
        }
        if !is_valid_xml_string(red_trimmed) {
            anyhow::bail!(
                "invalid custom color label: 'Red' label contains illegal XML characters"
            );
        }
        if !is_valid_xml_string(green_trimmed) {
            anyhow::bail!(
                "invalid custom color label: 'Green' label contains illegal XML characters"
            );
        }
    }

    // Validate command-line parameters up-front before doing database locks or starting workers.
    let mut base_builder = SidecarSettings::builder();
    if let Some(v) = args.exposure {
        base_builder = base_builder.exposure(v);
    }
    if let Some(v) = args.temp {
        base_builder = base_builder.temperature(v);
    }
    if let Some(v) = args.tint {
        base_builder = base_builder.tint(v);
    }
    if let Some(v) = args.contrast {
        base_builder = base_builder.contrast(v);
    }
    if let Some(v) = args.highlights {
        base_builder = base_builder.highlights(v);
    }
    if let Some(v) = args.shadows {
        base_builder = base_builder.shadows(v);
    }
    if args.auto_tone {
        base_builder = base_builder.auto_tone(true);
    }
    base_builder
        .clone()
        .build()
        .context("invalid develop parameters")?;

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

    let rows = catalog
        .all_photos_with_cull_scores(MODEL_SLUG, CLIP_MODEL_SLUG)
        .with_context(|| "querying catalog for develop")?;

    let mut unique_rows = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();
    for row in rows {
        let sidecar_path = row.source_path().with_extension("xmp");

        // On case-insensitive filesystems (or FAT32/exFAT mounts on Linux), normalize path casing for deduplication
        // to prevent duplicate rows targeting the same sidecar from causing concurrent write races.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let dedup_key: Vec<u8> = {
            use unicode_normalization::UnicodeNormalization;
            sidecar_path
                .to_string_lossy()
                .nfc()
                .collect::<String>()
                .to_lowercase()
                .into_bytes()
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let dedup_key: Vec<u8> = {
            use unicode_normalization::UnicodeNormalization;
            sidecar_path
                .to_string_lossy()
                .nfc()
                .collect::<String>()
                .into_bytes()
        };

        if seen_paths.insert(dedup_key) {
            unique_rows.push(row);
        } else {
            tracing::warn!(
                path = %sidecar_path.display(),
                photo = %row.source_path().display(),
                "skipping duplicate photo row targeting the same sidecar path to prevent concurrent write race hazards"
            );
        }
    }
    let rows = unique_rows;

    if rows.is_empty() {
        eprintln!(
            "walked: 0, written: 0, updated: 0, conflict-preserved: 0, \
             force-overwritten: 0, file-missing: 0, errored: 0"
        );
        return Ok(0);
    }

    let has_any_cull_score = rows.iter().any(|r| r.nima_score().is_some());
    let has_any_cluster = rows.iter().any(|r| r.dedup_cluster_id().is_some());

    if !lr_rating && !lr_label && !lr_keywords && !lr_label_score {
        eprintln!(
            "WARNING: photohelper develop is running without any Lightroom NIMA mapping flags activated.\n\
             No Lightroom rating, color label, or keywords will be written to sidecars.\n\
             To enable metadata mapping, use the individual --lr-* flags or pass --all-lr."
        );
    }

    if (lr_rating || lr_label) && !has_any_cull_score {
        eprintln!(
            "WARNING: Lightroom rating/label flags were requested, but no culled scores exist in the catalog."
        );
    }
    if lr_keywords && !has_any_cull_score && !has_any_cluster {
        eprintln!(
            "WARNING: Lightroom keywords flag was requested, but neither culled scores nor duplicate clusters exist in the catalog."
        );
    }

    let stats = Arc::new(DevelopStats::new());

    let strategy = if args.force {
        ConflictStrategy::ForceOverwrite
    } else {
        ConflictStrategy::Safe
    };

    // base_builder initialized above

    // Spawn heartbeat thread (same pattern as ingest/cull/dedup).
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
                        "[heartbeat] develop: walked {}, written {}",
                        stats.walked.load(Ordering::Relaxed),
                        stats.written.load(Ordering::Relaxed),
                    );
                });
            })
            .context("spawning heartbeat thread")?
    };

    // Walk in parallel using Rayon (sidecar I/O is per-photo; heartbeat reads stats cross-thread).
    rows.par_iter().for_each(|row| {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        let source_path = row.source_path();

        // Step a: existence pre-check.
        match std::fs::metadata(source_path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(path = %source_path.display(), "file missing since ingest; skipping");
                stats.record_missing();
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "failed to check file existence");
                stats.record_outcome(Err(()));
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
        }

        // Step b: sidecar path (Lightroom convention: replace extension).
        let sidecar_path = source_path.with_extension("xmp");

        // Step c: build per-photo settings (fresh builder each photo).
        let mut builder = base_builder.clone();

        // Validate early that the NIMA score is finite
        let valid_nima = row.nima_score();
        if let Some(score) = valid_nima {
            if !score.is_finite() {
                tracing::warn!(
                    path = %source_path.display(),
                    value = score,
                    "NIMA score is non-finite; failing develop"
                );
                stats.record_outcome(Err(()));
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
        }

        if let Some(cluster_id) = row.dedup_cluster_id().filter(|&id| id >= 0) {
            builder = builder.dedup_cluster_id(cluster_id);
        }

        builder = builder.photohelper_id(row.photo_id().to_string());

        // Retrieve the UTC timestamp per-photo before conflict resolution
        // and XMP generation.
        let now_utc = time::OffsetDateTime::now_utc();
        builder = builder.last_processed_at(now_utc);

        let cluster_id = row.dedup_cluster_id().filter(|&id| id >= 0);

        if let Some(score) = valid_nima {
            builder = builder.nima_score(score);

            if lr_rating {
                let (rating_num, _) = crate::commands::util::nima_score_to_rating_and_tier(score);
                let rating = std::convert::TryFrom::try_from(rating_num)
                    .unwrap_or(photohelper_sidecar::Rating::Unrated);
                builder = builder.rating(rating);
            }

            if lr_label_score {
                builder = builder.label(crate::commands::util::format_nima_score_label(score));
            } else if lr_label {
                let label = if score < 4.0 {
                    red_trimmed
                } else if score >= 7.0 {
                    green_trimmed
                } else {
                    "" // Empty string clears existing label during merge
                };
                builder = builder.label(label);
            }

            if lr_keywords {
                let mut flat = std::collections::BTreeSet::new();
                let mut hierarchical = std::collections::BTreeSet::new();

                flat.insert("photohelper".to_string());
                hierarchical.insert("photohelper".to_string());

                let (_, tier) = crate::commands::util::nima_score_to_rating_and_tier(score);
                flat.insert(format!("nima:{tier}"));
                hierarchical.insert(format!("photohelper|nima:{tier}"));

                if let Some(id) = cluster_id {
                    flat.insert(format!("cluster:{id}"));
                    hierarchical.insert(format!("photohelper|cluster:{id}"));
                }

                builder = builder.keywords(flat);
                builder = builder.hierarchical_keywords(hierarchical);
            }
        } else {
            builder = builder.clear_nima_score();

            if lr_rating {
                builder = builder.rating(photohelper_sidecar::Rating::Unrated);
            }
            if lr_label_score || lr_label {
                builder = builder.label("");
            }
            if lr_keywords {
                let mut flat = std::collections::BTreeSet::new();
                let mut hierarchical = std::collections::BTreeSet::new();

                if let Some(id) = cluster_id {
                    flat.insert("photohelper".to_string());
                    hierarchical.insert("photohelper".to_string());
                    flat.insert(format!("cluster:{id}"));
                    hierarchical.insert(format!("photohelper|cluster:{id}"));
                }

                builder = builder.keywords(flat);
                builder = builder.hierarchical_keywords(hierarchical);
            }
        }

        let settings = match builder.build() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "invalid settings; skipping");
                stats.record_outcome(Err(()));
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
        };

        let sidecar_path_typed = match photohelper_sidecar::SidecarPath::new(&sidecar_path) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(path = %sidecar_path.display(), error = %e, "invalid sidecar path");
                stats.record_outcome(Err(()));
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
        };

        // Step d: write sidecar.
        match merge_and_write(&sidecar_path_typed, &settings, strategy) {
            Ok(outcome) => {
                stats.record_outcome(Ok(outcome));
                if outcome == WriteOutcome::ConflictPreserved {
                    tracing::info!(
                        path = %sidecar_path.display(),
                        "Preserved newer Lightroom Classic edits; skipped"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(path = %sidecar_path.display(), error = %e, "XMP write failed");
                stats.record_outcome(Err(()));
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
            }
        }
    });

    // Heartbeat shutdown.
    if heartbeat_handle.is_finished() {
        tracing::warn!("heartbeat thread died before end-of-develop");
    }
    stop.signal();
    if let Err(e) = heartbeat_handle.join() {
        anyhow::bail!("heartbeat thread panicked: {e:?}");
    }

    eprintln!("{}", stats.summary_line());

    let conflict_count = stats.conflict_preserved.load(Ordering::Relaxed);
    if conflict_count > 0 {
        eprintln!(
            "\nWARNING: {conflict_count} files were skipped to protect Lightroom Classic manual edits.\n\
             If you want to unconditionally force overwrite, re-run with --force."
        );
    }

    // Exit code.
    let errors = stats.file_missing.load(Ordering::Relaxed) + stats.errored.load(Ordering::Relaxed);
    if args.strict && errors > 0 {
        return Ok(exit_code::EX_STRICT_FAIL);
    }
    Ok(0)
}
