//! `photohelper export` — export photos with resizing and watermarking.
//!
//! Pipeline:
//! 1. Validate output directory and write permissions.
//! 2. Retrieve all active catalog photos with scores.
//! 3. Build upfront deterministic collision mapping.
//! 4. Run export pipeline in parallel using Rayon.
//! 5. Output progressive-scan, progressive Huffman MozJPEG images.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Context as _;
use photohelper_ai::{CLIP_MODEL_SLUG, MODEL_SLUG};
use photohelper_catalog::Catalog;
use photohelper_export::{
    ExportMetadata, ExportOptions, NimaScore, Rating, WatermarkPosition, export_photo,
};
use photohelper_sidecar::read_xmp;
use rayon::prelude::*;

use crate::Cli;
use crate::commands::util::{TempFileGuard, resolve_collisions};
use crate::exit_code;
use crate::heartbeat::{HeartbeatStop, heartbeat_interval, run_heartbeat_loop};

/// Position of watermark.
#[derive(clap::ValueEnum, Clone, Debug, Copy, PartialEq, Eq)]
pub(crate) enum CliWatermarkPosition {
    #[value(name = "bottom-left")]
    BottomLeft,
    #[value(name = "top-right")]
    TopRight,
    #[value(name = "top-left")]
    TopLeft,
    #[value(name = "bottom-right")]
    BottomRight,
    #[value(name = "center")]
    Center,
}

impl From<CliWatermarkPosition> for WatermarkPosition {
    fn from(pos: CliWatermarkPosition) -> Self {
        match pos {
            CliWatermarkPosition::BottomLeft => WatermarkPosition::BottomLeft,
            CliWatermarkPosition::TopRight => WatermarkPosition::TopRight,
            CliWatermarkPosition::TopLeft => WatermarkPosition::TopLeft,
            CliWatermarkPosition::BottomRight => WatermarkPosition::BottomRight,
            CliWatermarkPosition::Center => WatermarkPosition::Center,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BadgeArg {
    pub path: PathBuf,
    pub pos: WatermarkPosition,
    pub scale: Option<f32>,
}

impl std::str::FromStr for BadgeArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut path = None;
        let mut pos = None;
        let mut scale = None;

        for part in s.split(',') {
            let Some((k, v)) = part.split_once('=') else {
                return Err(format!("Invalid badge arg part (expected k=v): {part}"));
            };
            match k {
                "path" => path = Some(PathBuf::from(v)),
                "pos" => {
                    pos = Some(match v {
                        "bottom-left" => WatermarkPosition::BottomLeft,
                        "top-right" => WatermarkPosition::TopRight,
                        "top-left" => WatermarkPosition::TopLeft,
                        "bottom-right" => WatermarkPosition::BottomRight,
                        "center" => WatermarkPosition::Center,
                        _ => return Err(format!("Invalid position: {v}")),
                    });
                }
                "scale" => {
                    scale = Some(
                        v.parse::<f32>()
                            .map_err(|e| format!("Invalid scale: {e}"))?,
                    );
                }
                _ => return Err(format!("Unknown badge parameter: {k}")),
            }
        }

        let path = path.ok_or_else(|| "Missing path in --badge".to_string())?;
        let pos = pos.ok_or_else(|| "Missing pos in --badge".to_string())?;

        Ok(Self { path, pos, scale })
    }
}

/// Clap args for `photohelper export`.
#[derive(clap::Args, Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ExportArgs {
    /// Output directory for compiled JPEGs.
    #[arg(long)]
    pub(crate) output: PathBuf,

    /// Long-edge resize limit in pixels (strictly >= 16).
    #[arg(long, value_parser = validate_long_edge)]
    pub(crate) long_edge: Option<u32>,

    /// JPEG quality level (1..=100).
    #[arg(long, default_value = "80", value_parser = clap::value_parser!(u8).range(1..=100))]
    pub(crate) quality: u8,

    /// Watermark text.
    #[arg(long)]
    pub(crate) watermark: Option<String>,

    /// Position of watermark text (e.g. bottom-left, top-right).
    #[arg(long, default_value = "bottom-left")]
    pub(crate) watermark_position: CliWatermarkPosition,

    /// Image badge watermark. Format: path=<PATH>,pos=<POS>[,scale=<PCT>]
    #[arg(long = "badge")]
    pub(crate) badges: Vec<BadgeArg>,

    /// Minimum rating to export (0..=5).
    #[arg(long, default_value = "3", value_parser = clap::value_parser!(u8).range(0..=5))]
    pub(crate) min_rating: u8,

    /// Force overwriting of existing output JPEGs.
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,

    /// Treat any single-photo export failure as fatal, causing immediate pipeline cancellation.
    #[arg(long, default_value_t = false)]
    pub(crate) strict: bool,
}

pub fn validate_long_edge(s: &str) -> Result<u32, String> {
    let val: u32 = s.parse().map_err(|e| format!("invalid number: {e}"))?;
    if val < 16 {
        return Err("long-edge limit must be at least 16 pixels".to_string());
    }
    Ok(val)
}

struct ExportStats {
    walked: AtomicU64,
    written: AtomicU64,
    skipped_existing: AtomicU64,
    skipped_rating: AtomicU64,
    file_missing: AtomicU64,
    errored: AtomicU64,
}

impl ExportStats {
    fn new() -> Self {
        Self {
            walked: AtomicU64::new(0),
            written: AtomicU64::new(0),
            skipped_existing: AtomicU64::new(0),
            skipped_rating: AtomicU64::new(0),
            file_missing: AtomicU64::new(0),
            errored: AtomicU64::new(0),
        }
    }

    fn summary_line(&self) -> String {
        format!(
            "walked: {}, written: {}, skipped-existing: {}, skipped-rating: {}, file-missing: {}, errored: {}",
            self.walked.load(Ordering::Relaxed),
            self.written.load(Ordering::Relaxed),
            self.skipped_existing.load(Ordering::Relaxed),
            self.skipped_rating.load(Ordering::Relaxed),
            self.file_missing.load(Ordering::Relaxed),
            self.errored.load(Ordering::Relaxed),
        )
    }
}

/// Driver for `photohelper export`.
///
/// # Errors
///
/// Returns `Err` only for fatal setup failures.
pub fn run_export(cli: &Cli, args: &ExportArgs) -> anyhow::Result<u8> {
    // 1. Upfront Directory Prep & Write check
    std::fs::create_dir_all(&args.output).with_context(|| {
        format!(
            "failed to create output directory {}",
            args.output.display()
        )
    })?;

    let test_file = args.output.join(".ph_write_test");
    std::fs::write(&test_file, b"").map_err(|e| {
        anyhow::anyhow!(
            "output directory {} is not writable: {}",
            args.output.display(),
            e
        )
    })?;
    // Best-effort cleanup; ignore if not found or already deleted
    let _ = std::fs::remove_file(test_file);

    // 2. Open Catalog and fetch photos
    let catalog_path = cli.catalog.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".photohelper")
            .join("catalog.db")
    });

    let rows = {
        let catalog = Catalog::open(&catalog_path, cli.catalog_lock_timeout_seconds)
            .with_context(|| format!("opening catalog at {}", catalog_path.display()))?;

        catalog
            .all_photos_with_cull_scores(MODEL_SLUG, CLIP_MODEL_SLUG)
            .with_context(|| "querying catalog for active photos")?
    };

    if rows.is_empty() {
        eprintln!(
            "walked: 0, written: 0, skipped-existing: 0, skipped-rating: 0, file-missing: 0, errored: 0"
        );
        return Ok(0);
    }

    // 3. Deterministic Collision Resolution
    let source_paths: Vec<PathBuf> = rows.iter().map(|r| r.source_path().to_path_buf()).collect();
    let collision_map = resolve_collisions(&args.output, &source_paths, |src| {
        let stem = src
            .file_stem()
            .unwrap_or_else(|| std::ffi::OsStr::new("photo"));
        // Find the corresponding row to get cull score + cluster id.
        let row_opt = rows.iter().find(|r| r.source_path() == src);
        let actual_score = row_opt
            .and_then(|r| r.nima_score())
            .filter(|s| s.is_finite() && !s.is_nan());
        let cluster_prefix = row_opt
            .and_then(|r| r.dedup_cluster_id())
            .map(|id| format!("cluster-{id:03}-"))
            .unwrap_or_default();
        if let Some(score) = actual_score {
            format!(
                "{}cull-{:05.2}-{}.jpg",
                cluster_prefix,
                score,
                stem.to_string_lossy()
            )
        } else {
            format!("{}{}.jpg", cluster_prefix, stem.to_string_lossy())
        }
    });

    let stats = Arc::new(ExportStats::new());
    let cancelled = Arc::new(AtomicBool::new(false));

    // 4. Preload badges
    let mut preloaded_badges = HashMap::new();
    for badge in &args.badges {
        let preloaded = photohelper_export::PreloadedBadge::load(
            &badge.path,
            badge.scale.map(photohelper_export::Scale::new),
        )
        .with_context(|| format!("failed to preload badge at {}", badge.path.display()))?;

        if preloaded_badges.insert(badge.pos, preloaded).is_some() {
            anyhow::bail!("Duplicate watermark position detected: {:?}", badge.pos);
        }
    }

    // 4. Spawn Heartbeat progress thread
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
                        "[heartbeat] export: walked {}, written {}, errored {}",
                        stats.walked.load(Ordering::Relaxed),
                        stats.written.load(Ordering::Relaxed),
                        stats.errored.load(Ordering::Relaxed),
                    );
                });
            })
            .context("spawning heartbeat thread")?
    };

    // 5. Parallel Processing loop
    rows.par_iter().for_each(|row| {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        stats.walked.fetch_add(1, Ordering::Relaxed);
        let source_path = row.source_path();

        // Step a: existence pre-check
        if !source_path.exists() {
            tracing::warn!(path = %source_path.display(), "source RAW file missing; skipping");
            stats.file_missing.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }

        // Step b: effective rating and tone mapping evaluation
        let xmp_path = source_path.with_extension("xmp");
        let mut rating_val = None;
        let mut tone_mapping = photohelper_export::ToneMappingOptions::default();

        if xmp_path.exists() {
            match read_xmp(&xmp_path) {
                Ok(settings) => {
                    if let Some(r) = settings.rating() {
                        rating_val = Some(r.as_i32());
                    }
                    if let Some(ev) = settings.exposure() {
                        tone_mapping.exposure_ev = ev;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        path = %xmp_path.display(),
                        error = %e,
                        "failed to parse XMP sidecar rating"
                    );
                    stats.errored.fetch_add(1, Ordering::Relaxed);
                    if args.strict {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                    return;
                }
            }
        }

        let rating_val = match rating_val {
            Some(v) => v,
            None => {
                if let Some(score) = row.nima_score() {
                    if score.is_finite() && !score.is_nan() {
                        let (rating, _) = crate::commands::util::nima_score_to_rating_and_tier(score);
                        rating
                    } else {
                        tracing::warn!(
                            path = %source_path.display(),
                            value = score,
                            "NIMA score is non-finite; failing export"
                        );
                        stats.errored.fetch_add(1, Ordering::Relaxed);
                        if args.strict {
                            cancelled.store(true, Ordering::Relaxed);
                        }
                        return;
                    }
                } else {
                    0
                }
            }
        };

        if rating_val == -1 || rating_val < i32::from(args.min_rating) {
            stats.skipped_rating.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Step c: identify target output path
        let Some(final_target_path) = collision_map.get(source_path) else {
            tracing::error!(path = %source_path.display(), "source RAW path not found in collision map");
            stats.errored.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        };

        if final_target_path.exists() && !args.force {
            tracing::info!(path = %final_target_path.display(), "output JPEG already exists; skipping");
            stats.skipped_existing.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let tmp_path = final_target_path.with_extension("tmp");
        let mut guard = TempFileGuard::new(tmp_path.clone());

        let mut watermarks = HashMap::new();
        if let Some(ref text) = args.watermark {
            watermarks.insert(args.watermark_position.into(), photohelper_export::Watermark::Text(text.clone()));
        }
        for (pos, badge) in &preloaded_badges {
            watermarks.insert(*pos, photohelper_export::Watermark::Image(badge.clone()));
        }

        // Step d: invoke export_photo pipeline
        let options = ExportOptions {
            output_path: tmp_path.clone(),
            quality: args.quality,
            long_edge: args.long_edge,
            watermarks,
            force: args.force,
            tone_mapping,
        };

        let rating = Rating::new(rating_val);
        let nima_score = row.nima_score().and_then(NimaScore::new);
        let metadata = ExportMetadata {
            rating,
            nima_score,
        };

        match export_photo(&options, source_path, &metadata) {
            Ok(()) => {
                if let Err(e) = std::fs::rename(&tmp_path, final_target_path) {
                    tracing::warn!(
                        from = %tmp_path.display(),
                        to = %final_target_path.display(),
                        error = %e,
                        "failed to rename temporary file to final target"
                    );
                    stats.errored.fetch_add(1, Ordering::Relaxed);
                    // guard.drop() cleans up tmp_path automatically.
                    if args.strict {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                } else {
                    guard.commit(); // Disarm cleanup only after rename succeeds.
                    stats.written.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                tracing::warn!(
                    path = %source_path.display(),
                    error = %e,
                    "failed to export photo"
                );
                stats.errored.fetch_add(1, Ordering::Relaxed);
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
            }
        }
    });

    // Heartbeat shutdown
    if heartbeat_handle.is_finished() {
        tracing::warn!("heartbeat thread died before end-of-export");
    }
    stop.signal();
    if let Err(e) = heartbeat_handle.join() {
        tracing::error!("heartbeat thread panicked: {:?}", e);
    }

    eprintln!("{}", stats.summary_line());

    // 6. Return appropriate Exit Code
    let total_failures =
        stats.file_missing.load(Ordering::Relaxed) + stats.errored.load(Ordering::Relaxed);

    if total_failures > 0 {
        if args.strict {
            return Ok(exit_code::EX_STRICT_FAIL);
        }
        return Ok(exit_code::EX_PARTIAL_FAIL);
    }

    Ok(0)
}
