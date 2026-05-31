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
use photohelper_sidecar::conflict::{WriteOutcome, merge_and_write};
use rayon::prelude::*;

use crate::Cli;
use crate::exit_code;
use crate::heartbeat::{HeartbeatStop, heartbeat_interval, run_heartbeat_loop};

/// Clap args for `photohelper develop`.
#[derive(clap::Args, Debug, Clone)]
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
}

fn is_valid_xml_string(s: &str) -> bool {
    s.chars().all(|c| {
        let val = c as u32;
        let is_valid_xml_char = (0x20..=0xD7FF).contains(&val)
            || val == 0x09
            || val == 0x0A
            || val == 0x0D
            || (0xE000..=0xFFFD).contains(&val)
            || (0x10000..=0x10_FFFF).contains(&val);
        let is_noncharacter = (0xFDD0..=0xFDEF).contains(&val) || (val & 0xFFFE) == 0xFFFE;
        is_valid_xml_char && !is_noncharacter
    })
}

/// Driver for `photohelper develop`.
///
/// # Errors
///
/// Returns `Err` only for fatal setup failures (catalog open, photo query, heartbeat spawn).
pub fn run_develop(cli: &Cli, args: &DevelopArgs) -> anyhow::Result<u8> {
    let lr_rating = args.all_lr || args.lr_rating;
    let lr_label = args.all_lr || args.lr_label;
    let lr_keywords = args.all_lr || args.lr_keywords;
    let cancelled = std::sync::atomic::AtomicBool::new(false);

    let red_trimmed = args.lr_label_red.trim();
    let green_trimmed = args.lr_label_green.trim();

    if lr_label {
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

    if !lr_rating && !lr_label && !lr_keywords {
        eprintln!(
            "WARNING: photohelper develop is running without any metadata flags activated.\n\
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

    // Walk in parallel using Rayon (sidecar I/O is per-photo; heartbeat reads stats cross-thread).
    rows.par_iter().for_each(|row| {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

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
        let valid_nima = if let Some(score) = row.nima_score() {
            if score.is_finite() && !score.is_nan() {
                Some(score)
            } else {
                tracing::warn!(
                    path = %source_path.display(),
                    value = score,
                    "NIMA score is non-finite; failing develop"
                );
                stats.errored.fetch_add(1, Ordering::Relaxed);
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
        } else {
            None
        };

        if let Some(score) = valid_nima {
            builder = builder.nima_score(score);
        }

        if let Some(cluster_id) = row.dedup_cluster_id().filter(|&id| id >= 0) {
            builder = builder.dedup_cluster_id(cluster_id);
        }

        builder = builder.photohelper_id(row.photo_id().to_string());

        // Retrieve the UTC timestamp per-photo immediately before writing
        // to completely eliminate write-buffer delay and scheduling drift.
        let now_utc = time::OffsetDateTime::now_utc();
        builder = builder.last_processed_at(now_utc);

        // Write Lightroom star ratings (1 to 5) based on NIMA score.
        if lr_rating {
            if let Some(score) = valid_nima {
                let (rating_num, _) = crate::commands::util::nima_score_to_rating_and_tier(score);
                let rating = match rating_num {
                    1 => photohelper_sidecar::Rating::One,
                    2 => photohelper_sidecar::Rating::Two,
                    3 => photohelper_sidecar::Rating::Three,
                    4 => photohelper_sidecar::Rating::Four,
                    _ => photohelper_sidecar::Rating::Five,
                };
                builder = builder.rating(rating);
            }
        }

        // Write Lightroom color labels (Red/Green) based on NIMA score.
        if lr_label {
            if let Some(score) = valid_nima {
                let label = if score < 4.0 {
                    red_trimmed
                } else if score >= 7.0 {
                    green_trimmed
                } else {
                    "" // Empty string clears existing label during merge
                };
                builder = builder.label(label);
            }
        }

        // Write photohelper keywords (tier/cluster) based on NIMA score and duplicate cluster ID.
        if lr_keywords {
            let mut flat = std::collections::BTreeSet::new();
            let mut hierarchical = std::collections::BTreeSet::new();

            flat.insert("photohelper".to_string());
            hierarchical.insert("photohelper".to_string());

            if let Some(score) = valid_nima {
                let (_, tier) = crate::commands::util::nima_score_to_rating_and_tier(score);
                flat.insert(format!("nima:{tier}"));
                hierarchical.insert(format!("photohelper|nima:{tier}"));
            }

            if let Some(cluster_id) = row.dedup_cluster_id() {
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
                // Log concisely at info/debug inside loop to prevent lock contention
                tracing::info!(
                    path = %sidecar_path.display(),
                    "Preserved newer Lightroom Classic edits; skipped"
                );
            }
            Ok(WriteOutcome::ForcedOverwrite) => {
                stats.force_overwritten.fetch_add(1, Ordering::Relaxed);
            }
            Ok(other) => {
                tracing::warn!("encountered unexpected XMP write outcome: {:?}", other);
                stats.errored.fetch_add(1, Ordering::Relaxed);
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
