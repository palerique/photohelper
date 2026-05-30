//! `photohelper develop` — write Lightroom-compatible XMP sidecars for ingested photos.
//!
//! Pipeline:
//! 1. Load all non-superseded photos + NIMA scores + duplicate cluster IDs from catalog.
//! 2. For each photo: check existence, build XMP settings, call merge_and_write.
//! 3. Print summary; exit 0 (or EX_STRICT_FAIL if --strict and errors > 0).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::Context as _;

use photohelper_ai::{CLIP_MODEL_SLUG, MODEL_SLUG};
use photohelper_catalog::Catalog;
use photohelper_sidecar::SidecarSettings;
use photohelper_sidecar::conflict::{WriteOutcome, merge_and_write};
use rayon::prelude::*;

use crate::Cli;
use crate::exit_code;
use crate::heartbeat::{HeartbeatStop, heartbeat_interval, run_heartbeat_loop};

/// Clap args for `photohelper develop`.
#[derive(clap::Args, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct DevelopArgs {
    /// Exit non-zero if any per-photo error occurs (file_missing or write errors).
    #[arg(long, default_value_t = false)]
    strict: bool,
    /// Always overwrite existing XMP sidecars (skip conflict check).
    #[arg(long, default_value_t = false)]
    force: bool,
    /// Exposure compensation in stops (–5.0 to 5.0).
    #[arg(long)]
    exposure: Option<f32>,
    /// White balance temperature in Kelvin (2000–50000).
    #[arg(long)]
    temp: Option<i32>,
    /// White balance tint (–150 to 150).
    #[arg(long)]
    tint: Option<i32>,
    /// Contrast (–100 to 100).
    #[arg(long)]
    contrast: Option<i32>,
    /// Highlights (–100 to 100).
    #[arg(long)]
    highlights: Option<i32>,
    /// Shadows (–100 to 100).
    #[arg(long)]
    shadows: Option<i32>,
    /// Write Lightroom star ratings (1 to 5) based on NIMA score.
    #[arg(long, default_value_t = false)]
    lr_rating: bool,
    /// Write Lightroom color labels (Red/Green) based on NIMA score.
    #[arg(long, default_value_t = false)]
    lr_label: bool,
    /// Write photohelper keywords (tier/cluster) based on NIMA score and duplicate cluster ID.
    #[arg(long, default_value_t = false)]
    lr_keywords: bool,
}

/// AtomicU64 counters for the develop pipeline summary.
/// AtomicU64 used for heartbeat-thread cross-thread visibility (not rayon parallelism).
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
}

/// Driver for `photohelper develop`.
///
/// # Errors
///
/// Returns `Err` only for fatal setup failures (catalog open, photo query, heartbeat spawn).
pub fn run_develop(cli: &Cli, args: &DevelopArgs) -> anyhow::Result<u8> {
    // Validate command-line parameters up-front before doing database locks or starting workers.
    {
        let mut builder = SidecarSettings::builder();
        if let Some(v) = args.exposure {
            builder = builder.exposure(v);
        }
        if let Some(v) = args.temp {
            builder = builder.temperature(v);
        }
        if let Some(v) = args.tint {
            builder = builder.tint(v);
        }
        if let Some(v) = args.contrast {
            builder = builder.contrast(v);
        }
        if let Some(v) = args.highlights {
            builder = builder.highlights(v);
        }
        if let Some(v) = args.shadows {
            builder = builder.shadows(v);
        }
        builder.build().context("invalid develop parameters")?;
    }

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

        // On case-insensitive filesystems (macOS, Windows), normalize path casing for deduplication
        // to prevent duplicate rows targeting the same sidecar from causing concurrent write races.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let dedup_key = PathBuf::from(sidecar_path.to_string_lossy().to_lowercase());
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let dedup_key = sidecar_path.clone();

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

    if (args.lr_rating || args.lr_label) && !has_any_cull_score {
        eprintln!(
            "WARNING: Lightroom rating/label flags were requested, but no culled scores exist in the catalog."
        );
    }
    if args.lr_keywords && !has_any_cull_score && !has_any_cluster {
        eprintln!(
            "WARNING: Lightroom keywords flag was requested, but neither culled scores nor duplicate clusters exist in the catalog."
        );
    }

    let stats = Arc::new(DevelopStats::new());

    // Capture CLI flags by value so each iteration can build a fresh builder.
    let cli_exposure = args.exposure;
    let cli_temp = args.temp;
    let cli_tint = args.tint;
    let cli_contrast = args.contrast;
    let cli_highlights = args.highlights;
    let cli_shadows = args.shadows;

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

    // Check the clock once before the loop (not per-photo) to avoid log spam.
    // A broken clock degrades conflict resolution (sidecars written without timestamps).
    let now_utc = unix_now_as_datetime();
    if now_utc.is_none() {
        tracing::warn!(
            "system clock returned a pre-epoch or invalid time; \
             XMP sidecars will be written without timestamps — \
             conflict resolution with Lightroom edits is degraded"
        );
    }

    // Walk in parallel using Rayon (sidecar I/O is per-photo; heartbeat reads stats cross-thread).
    rows.par_iter().for_each(|row| {
        stats.walked.fetch_add(1, Ordering::Relaxed);
        let source_path = row.source_path();

        // Step a: existence pre-check.
        if !source_path.exists() {
            tracing::warn!(path = %source_path.display(), "file missing since ingest; skipping");
            stats.file_missing.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Step b: sidecar path (Lightroom convention: replace extension).
        let sidecar_path = source_path.with_extension("xmp");

        // Step c: build per-photo settings (fresh builder each photo).
        let mut builder = SidecarSettings::builder();
        if let Some(v) = cli_exposure {
            builder = builder.exposure(v);
        }
        if let Some(v) = cli_temp {
            builder = builder.temperature(v);
        }
        if let Some(v) = cli_tint {
            builder = builder.tint(v);
        }
        if let Some(v) = cli_contrast {
            builder = builder.contrast(v);
        }
        if let Some(v) = cli_highlights {
            builder = builder.highlights(v);
        }
        if let Some(v) = cli_shadows {
            builder = builder.shadows(v);
        }

        // Validate early that the NIMA score is finite
        let valid_nima = row.nima_score().and_then(|score| {
            if score.is_finite() {
                Some(score)
            } else {
                tracing::warn!(
                    path = %source_path.display(),
                    value = score,
                    "NIMA score is non-finite; ignoring score for develop"
                );
                None
            }
        });

        if let Some(score) = valid_nima {
            builder = builder.nima_score(score);
        }

        if let Some(cluster_id) = row.dedup_cluster_id().filter(|&id| id >= 0) {
            builder = builder.dedup_cluster_id(cluster_id);
        }

        builder = builder.photohelper_id(row.photo_id().to_string());
        if let Some(dt) = now_utc {
            builder = builder.last_processed_at(dt);
        }

        // Write Lightroom star ratings (1 to 5) based on NIMA score.
        if args.lr_rating {
            if let Some(score) = valid_nima {
                let rating = if score < 4.0 {
                    photohelper_sidecar::Rating::One
                } else if score < 5.5 {
                    photohelper_sidecar::Rating::Two
                } else if score < 7.0 {
                    photohelper_sidecar::Rating::Three
                } else if score < 8.5 {
                    photohelper_sidecar::Rating::Four
                } else {
                    photohelper_sidecar::Rating::Five
                };
                builder = builder.rating(rating);
            }
        }

        // Write Lightroom color labels (Red/Green) based on NIMA score.
        if args.lr_label {
            if let Some(score) = valid_nima {
                let label = if score < 4.0 {
                    "Red"
                } else if score >= 7.0 {
                    "Green"
                } else {
                    "" // Empty string clears existing label during merge
                };
                builder = builder.label(label);
            }
        }

        // Write photohelper keywords (tier/cluster) based on NIMA score and duplicate cluster ID.
        if args.lr_keywords {
            let mut flat = std::collections::BTreeSet::new();
            let mut hierarchical = std::collections::BTreeSet::new();

            flat.insert("photohelper".to_string());
            hierarchical.insert("photohelper".to_string());

            if let Some(score) = valid_nima {
                let tier = if score < 4.0 {
                    "discard"
                } else if score < 5.5 {
                    "poor"
                } else if score < 7.0 {
                    "fair"
                } else if score < 8.5 {
                    "good"
                } else {
                    "excellent"
                };
                flat.insert(format!("nima:{tier}"));
                hierarchical.insert(format!("photohelper|nima:{tier}"));
            }

            if let Some(cluster_id) = row.dedup_cluster_id().filter(|&id| id >= 0) {
                flat.insert(format!("cluster:{cluster_id}"));
                hierarchical.insert(format!("photohelper|cluster:{cluster_id}"));
            }

            builder = builder.keywords(flat);
            builder = builder.hierarchical_keywords(hierarchical);
        }

        let settings = match builder.build() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "invalid settings; skipping");
                stats.errored.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // Step d: write sidecar.
        match merge_and_write(&sidecar_path, &settings, args.force) {
            Ok(WriteOutcome::Created) => {
                stats.written.fetch_add(1, Ordering::Relaxed);
            }
            Ok(WriteOutcome::Overwritten) => {
                stats.updated.fetch_add(1, Ordering::Relaxed);
            }
            Ok(WriteOutcome::ConflictPreserved) => {
                stats.conflict_preserved.fetch_add(1, Ordering::Relaxed);
            }
            Ok(WriteOutcome::ForcedOverwrite) => {
                stats.force_overwritten.fetch_add(1, Ordering::Relaxed);
            }
            Ok(other) => {
                tracing::warn!("encountered unexpected XMP write outcome: {:?}", other);
            }
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "XMP write failed");
                stats.errored.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    // Heartbeat shutdown.
    if heartbeat_handle.is_finished() {
        tracing::warn!("heartbeat thread died before end-of-develop");
    }
    stop.signal();
    if let Err(e) = heartbeat_handle.join() {
        tracing::error!("heartbeat thread panicked: {:?}", e);
    }

    eprintln!("{}", stats.summary_line());

    // Exit code.
    let errors = stats.file_missing.load(Ordering::Relaxed) + stats.errored.load(Ordering::Relaxed);
    if args.strict && errors > 0 {
        return Ok(exit_code::EX_STRICT_FAIL);
    }
    Ok(0)
}

/// Returns the current UTC time as a `time::OffsetDateTime`, or `None` on
/// clock failure (highly unlikely but handle gracefully).
fn unix_now_as_datetime() -> Option<time::OffsetDateTime> {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .and_then(|s| time::OffsetDateTime::from_unix_timestamp(s).ok())
}
