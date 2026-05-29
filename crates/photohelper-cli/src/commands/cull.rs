//! `photohelper cull` — AI aesthetic culling via the NIMA model.
//!
//! Pipeline per photo: re-derive `PhotoId` (content-change detection) →
//! existence check → RGB decode → NIMA inference → persist score.
//! All five failure modes have per-photo counters; no single failure
//! aborts the batch.
//!
//! See `docs/plans/session-04.md §D3` for the full pipeline spec.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Context as _;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

use photohelper_ai::{MODEL_SLUG, Nima, VerifiedModelBytes};
use photohelper_catalog::{Catalog, InsertScoreOutcome};
use photohelper_core::model::PhotoId;
use photohelper_raw::decode::read_raw_rgb;

use crate::commands::ingest::{HeartbeatStop, heartbeat_interval};
use crate::{Cli, exit_code};

/// Clap args for `photohelper cull`.
#[derive(clap::Args, Debug)]
pub(crate) struct CullArgs {
    /// Treat decode, inference, or derive failures as fatal at end-of-run.
    #[arg(long, default_value_t = false)]
    strict: bool,
}

/// Atomic counters for the `cull` summary line.
struct CullStats {
    walked: AtomicU64,
    scored: AtomicU64,
    already_scored: AtomicU64,
    decode_failed: AtomicU64,
    infer_failed: AtomicU64,
    file_missing: AtomicU64,
    content_changed: AtomicU64,
    catalog_inconsistency: AtomicU64,
    derive_failed: AtomicU64,
}

impl CullStats {
    fn new() -> Self {
        Self {
            walked: AtomicU64::new(0),
            scored: AtomicU64::new(0),
            already_scored: AtomicU64::new(0),
            decode_failed: AtomicU64::new(0),
            infer_failed: AtomicU64::new(0),
            file_missing: AtomicU64::new(0),
            content_changed: AtomicU64::new(0),
            catalog_inconsistency: AtomicU64::new(0),
            derive_failed: AtomicU64::new(0),
        }
    }

    fn summary_line(&self) -> String {
        format!(
            "walked: {}, scored: {}, already-scored: {}, decode-failed: {}, \
             infer-failed: {}, file-missing: {}, content-changed: {}, \
             catalog-inconsistency: {}, derive-failed: {}",
            self.walked.load(Ordering::Relaxed),
            self.scored.load(Ordering::Relaxed),
            self.already_scored.load(Ordering::Relaxed),
            self.decode_failed.load(Ordering::Relaxed),
            self.infer_failed.load(Ordering::Relaxed),
            self.file_missing.load(Ordering::Relaxed),
            self.content_changed.load(Ordering::Relaxed),
            self.catalog_inconsistency.load(Ordering::Relaxed),
            self.derive_failed.load(Ordering::Relaxed),
        )
    }
}

/// Driver for `photohelper cull`.
///
/// Fetches all non-superseded, unscored photos from the catalog, runs NIMA
/// inference on each, and persists the score. Per-photo failures are counted
/// and logged; they never abort the batch.
///
/// # Errors
///
/// Returns `Err` only for fatal setup failures (catalog open, thread spawn).
pub fn run_cull(
    cli: &Cli,
    args: &CullArgs,
    model: &VerifiedModelBytes,
    model_path: PathBuf,
) -> anyhow::Result<u8> {
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

    let nima = Arc::new(Nima::new(model, model_path));

    let rows = catalog
        .unsuperseded_unscored_rows(MODEL_SLUG)
        .with_context(|| "querying unsuperseded unscored rows")?;

    if rows.is_empty() {
        // Use CullStats::new() so the format stays in sync with summary_line().
        eprintln!("{}", CullStats::new().summary_line());
        return Ok(0);
    }

    let stats = Arc::new(CullStats::new());

    // Heartbeat thread.
    // TD-016: heartbeat_loop_cull duplicates logic from heartbeat_loop in
    // ingest.rs; extract to commands/heartbeat.rs at the third consumer.
    let stop = Arc::new(HeartbeatStop::new());
    let heartbeat_handle = {
        let stats = Arc::clone(&stats);
        let stop = Arc::clone(&stop);
        let interval = heartbeat_interval();
        std::thread::Builder::new()
            .name("ph-heartbeat".into())
            .spawn(move || heartbeat_loop_cull(&stats, &stop, interval))
            .context("spawning heartbeat thread")?
    };

    rows.into_par_iter().for_each(|row| {
        stats.walked.fetch_add(1, Ordering::Relaxed);
        let source_path = row.source_path().to_path_buf();

        // Step 1: Existence pre-check — must precede derive (PhotoId::derive
        // calls fs::metadata, so a missing file would route to derive_failed
        // rather than file_missing if the check came second).
        if !source_path.exists() {
            tracing::warn!(path = %source_path.display(), "file missing since ingest; skipping");
            stats.file_missing.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Step 2: Re-derive PhotoId (content-change detection).
        let current_id = match PhotoId::derive(&source_path) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(
                    path = %source_path.display(),
                    error = %e,
                    "derive failed; skipping"
                );
                stats.derive_failed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        if current_id != row.photo_id() {
            tracing::warn!(
                path = %source_path.display(),
                "content changed since ingest; skipping"
            );
            stats.content_changed.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // Step 3: Decode to 8-bit sRGB.
        let rgb = match read_raw_rgb(&source_path) {
            Ok(img) => img,
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "decode failed");
                stats.decode_failed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // Step 4: NIMA inference.
        let score = match nima.score(&rgb) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %source_path.display(), error = %e, "inference failed");
                stats.infer_failed.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        // Step 5: Persist score.
        let scored_at = unix_now();
        match catalog.insert_cull_score(row.photo_id(), MODEL_SLUG, score.as_f64(), scored_at) {
            Ok(InsertScoreOutcome::Inserted) => {
                stats.scored.fetch_add(1, Ordering::Relaxed);
            }
            Ok(InsertScoreOutcome::AlreadyScored) => {
                stats.already_scored.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::warn!(
                    path = %source_path.display(),
                    error = %e,
                    "catalog write failed after inference"
                );
                stats.catalog_inconsistency.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    if heartbeat_handle.is_finished() {
        tracing::warn!("heartbeat thread died before end-of-cull; liveness signal was unavailable");
    }
    stop.signal();
    // join() result discarded intentionally — early death is already surfaced
    // by the is_finished() WARN above; the panic payload adds no extra value.
    let _ = heartbeat_handle.join();

    eprintln!("{}", stats.summary_line());

    let walked = stats.walked.load(Ordering::Relaxed);
    let scored = stats.scored.load(Ordering::Relaxed);
    let already_scored = stats.already_scored.load(Ordering::Relaxed);
    let decode_failed = stats.decode_failed.load(Ordering::Relaxed);
    let infer_failed = stats.infer_failed.load(Ordering::Relaxed);
    let derive_failed = stats.derive_failed.load(Ordering::Relaxed);

    // --strict exits non-zero when the inference pipeline itself fails.
    // Transient / user-caused conditions (file_missing, content_changed,
    // catalog_inconsistency) do not trigger strict.
    if args.strict && (decode_failed + infer_failed + derive_failed) > 0 {
        return Ok(exit_code::EX_STRICT_FAIL);
    }
    // "Nothing useful happened" check: walked at least one photo but produced
    // zero new or existing scores AND no per-photo errors explain the gap —
    // indicates a catalog / path mismatch. Guard `all_per_photo_errors == 0`
    // prevents false EX_USAGE when files are systematically missing/changed.
    let all_per_photo_errors = derive_failed
        + decode_failed
        + infer_failed
        + stats.file_missing.load(Ordering::Relaxed)
        + stats.content_changed.load(Ordering::Relaxed)
        + stats.catalog_inconsistency.load(Ordering::Relaxed);
    if walked > 0 && (scored + already_scored) == 0 && all_per_photo_errors == 0 {
        return Ok(exit_code::EX_USAGE);
    }
    Ok(0)
}

// TD-016: heartbeat_loop_cull duplicates the heartbeat pattern from
// ingest.rs; extract to commands/heartbeat.rs at the third consumer.
fn heartbeat_loop_cull(stats: &CullStats, stop: &HeartbeatStop, interval: Duration) {
    let granularity = interval.min(Duration::from_millis(100));
    let ticks = interval
        .as_millis()
        .checked_div(granularity.as_millis())
        .unwrap_or(1)
        .max(1) as u64;
    let mut counter: u64 = 0;
    loop {
        counter += 1;
        if counter >= ticks {
            counter = 0;
            eprintln!(
                "[heartbeat] walked {}, scored {}, decode-failed {}",
                stats.walked.load(Ordering::Relaxed),
                stats.scored.load(Ordering::Relaxed),
                stats.decode_failed.load(Ordering::Relaxed),
            );
        }
        if stop.wait_for_stop(granularity) {
            return;
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}
