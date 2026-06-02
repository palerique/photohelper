//! `photohelper watermark` — standardise + shadow + dual-mark → JPEG batch.
//!
//! Pipeline:
//! 1. Validate args: canonicalize source/output; reject nested output; writability probe.
//! 2. Pre-load mark1 and mark2 PNG badges fatally up-front.
//! 3. Walk source directory (follow_links=false), prune output subtree, build sorted file list.
//! 4. Run per-file pipeline in parallel (rayon + heartbeat):
//!    load_source_image → render_to_jpeg (resize → shadow → mark1 → mark2) → atomic write.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Context as _;
use photohelper_export::{
    BadgeSizeBasis, ExportError, MARK_MARGIN_FRAC, MARK1_HEIGHT_FRAC, MARK2_HEIGHT_FRAC, MarkSlot,
    MarkSpec, PreloadedBadge, RenderOptions, SHADOW_BAND_FRAC, ShadowSpec, SourceKind,
    load_source_image, render_to_jpeg,
};
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::Cli;
use crate::commands::export::validate_long_edge;
use crate::commands::util::TempFileGuard;
use crate::exit_code;
use crate::heartbeat::{HeartbeatStop, heartbeat_interval, run_heartbeat_loop};

/// Fixed JPEG quality for the `watermark` subcommand (no `--quality` flag per spec).
const WATERMARK_JPEG_QUALITY: u8 = 85;

/// Clap args for `photohelper watermark`.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct WatermarkArgs {
    /// Source directory (read-only; JPEG, PNG, CR3 processed by default).
    #[arg(long)]
    pub(crate) source: PathBuf,

    /// Top-right corner mark (PNG only; fatal up-front if unreadable or non-PNG).
    #[arg(long)]
    pub(crate) mark1: PathBuf,

    /// Bottom-left corner mark (PNG only; fatal up-front if unreadable or non-PNG).
    #[arg(long)]
    pub(crate) mark2: PathBuf,

    /// Long-edge resize limit in pixels (strictly ≥ 16); downscale-only.
    #[arg(long, value_parser = validate_long_edge)]
    pub(crate) max_long_edge: Option<u32>,

    /// Output directory for produced JPEGs.
    #[arg(long)]
    pub(crate) output: PathBuf,

    /// Allow non-CR3 RAW files (LibRaw-supported; adds post-decode sanity guard).
    #[arg(long, default_value_t = false)]
    pub(crate) allow_untested_raw: bool,

    /// Overwrite existing output JPEGs.
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,

    /// Treat any single-file failure as immediately fatal.
    #[arg(long, default_value_t = false)]
    pub(crate) strict: bool,
}

struct WatermarkStats {
    walked: AtomicU64,
    written: AtomicU64,
    skipped_unsupported: AtomicU64,
    skipped_existing: AtomicU64,
    decode_failed: AtomicU64,
    mark_doesnt_fit: AtomicU64,
    errored: AtomicU64,
}

impl WatermarkStats {
    fn new() -> Self {
        Self {
            walked: AtomicU64::new(0),
            written: AtomicU64::new(0),
            skipped_unsupported: AtomicU64::new(0),
            skipped_existing: AtomicU64::new(0),
            decode_failed: AtomicU64::new(0),
            mark_doesnt_fit: AtomicU64::new(0),
            errored: AtomicU64::new(0),
        }
    }

    fn summary_line(&self) -> String {
        format!(
            "walked: {}, written: {}, skipped-unsupported: {}, skipped-existing: {}, \
             decode-failed: {}, mark-doesnt-fit: {}, errored: {}",
            self.walked.load(Ordering::Relaxed),
            self.written.load(Ordering::Relaxed),
            self.skipped_unsupported.load(Ordering::Relaxed),
            self.skipped_existing.load(Ordering::Relaxed),
            self.decode_failed.load(Ordering::Relaxed),
            self.mark_doesnt_fit.load(Ordering::Relaxed),
            self.errored.load(Ordering::Relaxed),
        )
    }

    fn total_failures(&self) -> u64 {
        self.decode_failed.load(Ordering::Relaxed)
            + self.mark_doesnt_fit.load(Ordering::Relaxed)
            + self.errored.load(Ordering::Relaxed)
    }
}

/// Driver for `photohelper watermark`.
///
/// # Errors
///
/// Returns `Err` only for fatal setup failures (bad args, unreadable marks,
/// non-writable output directory). Per-file failures are counted and surfaced
/// in the summary line.
pub fn run_watermark(_cli: &Cli, args: &WatermarkArgs) -> anyhow::Result<u8> {
    // 1. Canonicalize source and output.
    let source_canonical = dunce::canonicalize(&args.source)
        .with_context(|| format!("source directory not found: {}", args.source.display()))?;

    std::fs::create_dir_all(&args.output).with_context(|| {
        format!(
            "failed to create output directory: {}",
            args.output.display()
        )
    })?;
    let output_canonical = dunce::canonicalize(&args.output)
        .with_context(|| format!("cannot canonicalize output: {}", args.output.display()))?;

    // Reject output == source or output nested inside source.
    anyhow::ensure!(
        !output_canonical.starts_with(&source_canonical),
        "output directory {} must not be inside source directory {}",
        output_canonical.display(),
        source_canonical.display()
    );

    // Writability probe.
    let probe = output_canonical.join(".ph_write_test");
    std::fs::write(&probe, b"").with_context(|| {
        format!(
            "output directory {} is not writable",
            output_canonical.display()
        )
    })?;
    let _ = std::fs::remove_file(&probe);

    // 2. Pre-load mark PNGs up-front (fatal on any error per D-Q6).
    let mark1_badge = load_mark_png(&args.mark1, "mark1")?;
    let mark2_badge = load_mark_png(&args.mark2, "mark2")?;

    // 3. Walk source, prune output subtree, collect sorted file list.
    let files = collect_source_files(&source_canonical, &output_canonical);

    if files.is_empty() {
        eprintln!(
            "walked: 0, written: 0, skipped-unsupported: 0, skipped-existing: 0, \
             decode-failed: 0, mark-doesnt-fit: 0, errored: 0"
        );
        return Ok(0);
    }

    let stats = Arc::new(WatermarkStats::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let mark1_arc = Arc::clone(&mark1_badge.pixmap);
    let mark2_arc = Arc::clone(&mark2_badge.pixmap);

    // Spawn heartbeat.
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
                        "[heartbeat] watermark: walked {}, written {}, errored {}",
                        stats.walked.load(Ordering::Relaxed),
                        stats.written.load(Ordering::Relaxed),
                        stats.errored.load(Ordering::Relaxed),
                    );
                });
            })
            .context("spawning heartbeat thread")?
    };

    // 4. Parallel per-file pipeline.
    files.par_iter().for_each(|src_path| {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        stats.walked.fetch_add(1, Ordering::Relaxed);

        // Classify source.
        match SourceKind::classify(src_path) {
            None => {
                let ext = src_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("(no extension)");
                tracing::warn!(
                    path = %src_path.display(),
                    extension = ext,
                    "unsupported format — skipping. Supported: JPEG, PNG, CR3. \
                     Convert to JPEG first (e.g. export from Lightroom as JPEG)."
                );
                stats.skipped_unsupported.fetch_add(1, Ordering::Relaxed);
                return;
            }
            Some(SourceKind::UntestedRaw) if !args.allow_untested_raw => {
                stats.skipped_unsupported.fetch_add(1, Ordering::Relaxed);
                return;
            }
            _ => {}
        }

        // Output path: same stem + ".jpg" in output_canonical.
        let out_name = src_path.file_stem().map_or_else(
            || "output.jpg".to_string(),
            |s| format!("{}.jpg", s.to_string_lossy()),
        );
        let out_path = output_canonical.join(&out_name);

        if out_path.exists() && !args.force {
            stats.skipped_existing.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Load source image.
        let img = match load_source_image(src_path, args.allow_untested_raw) {
            Ok(img) => img,
            Err(ExportError::UnsupportedSource { .. } | ExportError::UntestedRawGated { .. }) => {
                stats.skipped_unsupported.fetch_add(1, Ordering::Relaxed);
                return;
            }
            Err(e) => {
                tracing::warn!(path = %src_path.display(), error = %e, "decode failed");
                stats.decode_failed.fetch_add(1, Ordering::Relaxed);
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
        };

        let w = img.width().get();
        let h = img.height().get();

        // Build render options: downscale-only resize, shadow band, mark1+mark2.
        // margin_x/margin_y are FRACTIONS (0..1); composite_mark_on_pixmap converts
        // to pixels against the post-resize dimensions — fixes the source-dims bug.
        let render_opts = RenderOptions {
            long_edge: args.max_long_edge,
            downscale_only: true,
            quality: WATERMARK_JPEG_QUALITY,
            shadow: Some(ShadowSpec {
                band_frac: SHADOW_BAND_FRAC,
            }),
            marks: vec![
                MarkSpec {
                    badge: Arc::clone(&mark1_arc),
                    basis: BadgeSizeBasis::Height(MARK1_HEIGHT_FRAC),
                    slot: MarkSlot::Mark1,
                    margin_x: MARK_MARGIN_FRAC,
                    margin_y: MARK_MARGIN_FRAC,
                },
                MarkSpec {
                    badge: Arc::clone(&mark2_arc),
                    basis: BadgeSizeBasis::Height(MARK2_HEIGHT_FRAC),
                    slot: MarkSlot::Mark2,
                    margin_x: MARK_MARGIN_FRAC,
                    margin_y: MARK_MARGIN_FRAC,
                },
            ],
        };

        // Render to JPEG bytes.
        let jpeg_bytes = match render_to_jpeg(img.pixels(), w, h, &render_opts) {
            Ok(bytes) => bytes,
            Err(ExportError::Geometry(ref _geo_err)) => {
                tracing::warn!(
                    path = %src_path.display(),
                    "mark does not fit on image; no output written"
                );
                stats.mark_doesnt_fit.fetch_add(1, Ordering::Relaxed);
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
            Err(e) => {
                tracing::warn!(path = %src_path.display(), error = %e, "render failed");
                stats.errored.fetch_add(1, Ordering::Relaxed);
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return;
            }
        };

        // Atomic write: tmp → rename.
        let tmp_path = out_path.with_extension("tmp");
        let mut guard = TempFileGuard::new(tmp_path.clone());
        if let Err(e) = std::fs::write(&tmp_path, &jpeg_bytes) {
            tracing::warn!(path = %tmp_path.display(), error = %e, "write tmp failed");
            stats.errored.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }
        if let Err(e) = std::fs::rename(&tmp_path, &out_path) {
            tracing::warn!(
                from = %tmp_path.display(),
                to = %out_path.display(),
                error = %e,
                "rename tmp → final failed"
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
    });

    // Heartbeat shutdown.
    if heartbeat_handle.is_finished() {
        tracing::warn!("heartbeat thread died before end-of-watermark");
    }
    stop.signal();
    if let Err(e) = heartbeat_handle.join() {
        tracing::error!("heartbeat thread panicked: {:?}", e);
    }

    eprintln!("{}", stats.summary_line());

    let total = stats.total_failures();
    if total > 0 {
        if args.strict {
            return Ok(exit_code::EX_STRICT_FAIL);
        }
        return Ok(exit_code::EX_PARTIAL_FAIL);
    }
    Ok(0)
}

/// Load a PNG file fatally (non-PNG or unreadable = fatal up-front per D-Q6).
fn load_mark_png(path: &Path, label: &str) -> anyhow::Result<PreloadedBadge> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    anyhow::ensure!(
        matches!(ext.as_deref(), Some("png")),
        "--{label} must be a PNG file (got: {})",
        path.display()
    );
    PreloadedBadge::load(path, None)
        .with_context(|| format!("failed to load {label} PNG at {}", path.display()))
}

/// Walk `source_canonical`, exclude `output_canonical` subtree, return sorted file list.
///
/// Walk errors (e.g. permission-denied subdirectories) are logged as warnings so
/// the user is informed of any files that could not be reached.
fn collect_source_files(source: &Path, output: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| match e {
            Ok(entry) => Some(entry),
            Err(err) => {
                tracing::warn!(error = %err, "directory walk error; entry skipped");
                None
            }
        })
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            match dunce::canonicalize(e.path()) {
                Ok(p) => {
                    if p.starts_with(output) {
                        None // Defense-in-depth: prune output subtree.
                    } else {
                        Some(p)
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        path = %e.path().display(),
                        error = %err,
                        "could not canonicalize path; entry skipped"
                    );
                    None
                }
            }
        })
        .collect();
    files.sort();
    files
}
