//! `photohelper ingest <path>` — walk directory, derive PhotoId, write
//! catalog rows.
//!
//! See `docs/plans/session-01.md` §Deliverables 5 + §Observability contract.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Context as _;
use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;

use photohelper_cameras::CameraRegistry;
use photohelper_catalog::{Catalog, UpsertOutcome};
use photohelper_core::Error;
use photohelper_core::model::{
    AbsPath, CameraId, ExifMetadata, IngestOutcome, Photo, PhotoId, clamp_mtime,
};

use crate::{Cli, IngestArgs, exit_code};

// Narrowed to `["cr3"]` for v0.1 per plan §4a R2-T8: photohelper supports
// exactly one RAW format (Canon CR3) until a non-Canon `CameraProfile`
// lands (per DN-014's binding trigger). Re-expansion to the full
// 7-format walker behavior happens in the same session that adds the
// second profile.
const RAW_EXTS: &[&str] = &["cr3"];

/// Heartbeat interval. Overridable in tests via the
/// `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` env var so test row 48 (heartbeat
/// at default verbosity) doesn't have to wait 10 seconds in CI.
fn heartbeat_interval() -> Duration {
    if let Ok(s) = std::env::var("PHOTOHELPER_HEARTBEAT_INTERVAL_MS")
        && let Ok(ms) = s.parse::<u64>()
    {
        return Duration::from_millis(ms.max(10));
    }
    Duration::from_secs(10)
}

/// Cooperative stop signal for the heartbeat thread. Pairs a `Mutex<bool>`
/// with a `Condvar` so the heartbeat loop's wait can be cut short the
/// instant `run_ingest` is ready to print the summary line — closes TD-003
/// (the previous `AtomicBool` + `thread::sleep(granularity)` design left
/// the heartbeat thread orphaned, racing stderr against the summary line).
pub(crate) struct HeartbeatStop {
    lock: Mutex<bool>,
    cvar: Condvar,
}

impl HeartbeatStop {
    fn new() -> Self {
        Self {
            lock: Mutex::new(false),
            cvar: Condvar::new(),
        }
    }

    /// Mark the stop flag and wake every waiter immediately.
    fn signal(&self) {
        let mut stopped = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        *stopped = true;
        drop(stopped);
        self.cvar.notify_all();
    }

    /// Wait up to `dur` for `signal()`; returns `true` if stop was observed.
    fn wait_for_stop(&self, dur: Duration) -> bool {
        let stopped = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        if *stopped {
            return true;
        }
        let (stopped, _) = self
            .cvar
            .wait_timeout(stopped, dur)
            .unwrap_or_else(|p| p.into_inner());
        *stopped
    }
}

/// Atomic counters mapped 1:1 to the §Observability summary line.
pub(crate) struct IngestStats {
    pub walked: AtomicU64,
    pub ingested: AtomicU64,
    pub superseded: AtomicU64,
    pub already_catalogued: AtomicU64,
    pub unknown_camera: AtomicU64,
    pub no_exif: AtomicU64,
    pub mtime_anomalous: AtomicU64,
    pub skipped_non_raw: AtomicU64,
    pub skipped_too_small: AtomicU64,
    pub errored: AtomicU64,
    pub in_flight: AtomicU64,
}

impl IngestStats {
    fn new() -> Self {
        Self {
            walked: AtomicU64::new(0),
            ingested: AtomicU64::new(0),
            superseded: AtomicU64::new(0),
            already_catalogued: AtomicU64::new(0),
            unknown_camera: AtomicU64::new(0),
            no_exif: AtomicU64::new(0),
            mtime_anomalous: AtomicU64::new(0),
            skipped_non_raw: AtomicU64::new(0),
            skipped_too_small: AtomicU64::new(0),
            errored: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
        }
    }

    fn summary_line(&self) -> String {
        format!(
            "walked: {}, ingested: {}, superseded: {}, already-catalogued: {}, \
             unknown-camera: {}, no-exif: {}, mtime-anomalous: {}, \
             skipped (non-RAW): {}, skipped (too-small): {}, errored: {}",
            self.walked.load(Ordering::Relaxed),
            self.ingested.load(Ordering::Relaxed),
            self.superseded.load(Ordering::Relaxed),
            self.already_catalogued.load(Ordering::Relaxed),
            self.unknown_camera.load(Ordering::Relaxed),
            self.no_exif.load(Ordering::Relaxed),
            self.mtime_anomalous.load(Ordering::Relaxed),
            self.skipped_non_raw.load(Ordering::Relaxed),
            self.skipped_too_small.load(Ordering::Relaxed),
            self.errored.load(Ordering::Relaxed),
        )
    }
}

/// Driver. Returns the exit code (0 / 64 / 1 only — fatals propagate as Err).
pub fn run_ingest(cli: &Cli, args: &IngestArgs) -> anyhow::Result<u8> {
    let input_root = AbsPath::canonicalize(&args.path)
        .with_context(|| format!("canonicalizing input path {}", args.path.display()))?;

    let catalog_path = cli
        .catalog
        .clone()
        .unwrap_or_else(|| input_root.as_path().join(".photohelper").join("catalog.db"));

    let catalog = Arc::new(
        Catalog::open(&catalog_path, cli.catalog_lock_timeout_seconds)
            .with_context(|| format!("opening catalog at {}", catalog_path.display()))?,
    );

    if let Some(t) = cli.threads {
        // R1.T10 fix: surface build_global failure. If the global pool
        // is already initialized (e.g., a prior test run in the same
        // process), the user's --threads flag is silently ignored —
        // WARN so they know to re-invoke from a fresh process.
        match rayon::ThreadPoolBuilder::new()
            .num_threads(t as usize)
            .build_global()
        {
            Ok(()) => tracing::info!(threads = t, "rayon pool initialized"),
            Err(e) => tracing::warn!(
                error = %e,
                requested = t,
                "rayon global pool already initialized; --threads ignored \
                 (run in a fresh process to take effect)"
            ),
        }
    }

    let stats = Arc::new(IngestStats::new());
    let registry = Arc::new(CameraRegistry::default());
    let seen_unknown = Arc::new(Mutex::new(HashSet::<(String, String)>::new()));

    // Heartbeat thread: spawned with a named handle so debuggers/profilers
    // show "ph-heartbeat" instead of `thread<unnamed>`. The thread parks on
    // `HeartbeatStop`'s Condvar between ticks; `signal()` wakes it the
    // moment the walk completes so the post-walk `.join()` returns near-
    // instantly (no granularity-cycle latency added to summary printing).
    let stop = Arc::new(HeartbeatStop::new());
    let heartbeat_handle = {
        let stats = Arc::clone(&stats);
        let stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("ph-heartbeat".into())
            .spawn(move || heartbeat_loop(&stats, &stop, heartbeat_interval()))
            .context("spawning heartbeat thread")?
    };

    let walker = walkdir::WalkDir::new(input_root.as_path()).follow_links(false);
    // Skip our own `.photohelper/` catalog directory so we don't walk
    // catalog.db / .lock / WAL files we just created.
    let walker_iter = walker
        .into_iter()
        .filter_entry(|e| e.file_name() != ".photohelper")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file());

    walker_iter.par_bridge().for_each(|entry| {
        stats.walked.fetch_add(1, Ordering::Relaxed);
        stats.in_flight.fetch_add(1, Ordering::Relaxed);
        let path = entry.path().to_path_buf();
        match ingest_one(
            &path,
            &input_root,
            catalog.as_ref(),
            registry.as_ref(),
            seen_unknown.as_ref(),
            stats.as_ref(),
        ) {
            Ok(outcome) => apply_outcome(&stats, &outcome),
            Err(err) => {
                tracing::warn!(error = %err, path = %path.display(), "ingest_one failed");
                stats.errored.fetch_add(1, Ordering::Relaxed);
            }
        }
        stats.in_flight.fetch_sub(1, Ordering::Relaxed);
    });

    // R1.T2 fix: check liveness BEFORE setting the stop flag — if the
    // heartbeat thread died early (eprintln! on closed stderr, panic,
    // etc.) the user lost their only liveness signal during the run
    // and deserves a WARN. is_finished() AFTER setting the flag would
    // always return true once the loop sees the flag, hiding the
    // distinction between "expected exit" and "early death."
    if heartbeat_handle.is_finished() {
        tracing::warn!(
            "heartbeat thread died before end-of-walk; liveness signal was \
             unavailable during ingest"
        );
    }
    // TD-003 closure (DN-019 trigger fired): signal the Condvar so the
    // heartbeat thread wakes up immediately, then join the handle so every
    // `[heartbeat]` line is flushed to stderr BEFORE the summary line.
    // Joining also reaps the detached thread that previously leaked once
    // per `run_ingest` call.
    stop.signal();
    // join() flushes the final heartbeat line before the summary. The Result is
    // Ok(()) on normal exit or Err(panicked) if the thread died — early death is
    // already surfaced by the is_finished() WARN above; discarding the join result
    // here is intentional.
    let _ = heartbeat_handle.join();

    eprintln!("{}", stats.summary_line());

    let walked = stats.walked.load(Ordering::Relaxed);
    let ingested = stats.ingested.load(Ordering::Relaxed);
    let superseded = stats.superseded.load(Ordering::Relaxed);
    let already = stats.already_catalogued.load(Ordering::Relaxed);
    let unknown = stats.unknown_camera.load(Ordering::Relaxed);
    let anomalous = stats.mtime_anomalous.load(Ordering::Relaxed);
    let errored = stats.errored.load(Ordering::Relaxed);
    let no_exif = stats.no_exif.load(Ordering::Relaxed);

    // R2-T12 fix: `--strict` was fail-open when EXIF was entirely missing.
    // User's prod trace on 371 real Canon R8 CR3s ran with `--strict` and
    // got `unknown-camera: 0, errored: 0` — strict passed despite every
    // photo being unroutable. The `unknown_camera` counter only bumps when
    // EXIF parsed AND make/model don't match a profile; "EXIF entirely
    // missing" silently fell through. Now strict fails on no_exif > 0 too,
    // which is operationally equivalent to "unrouted photo." This makes
    // strict mode effectively unusable in v0.1 for CR3 (per R2-T13 /
    // DN-006: kamadak-exif can't parse ANY real CR3) — that's the
    // intended escalation; LibRaw EXIF lands in session 02.
    if args.strict && (unknown > 0 || anomalous > 0 || errored > 0 || no_exif > 0) {
        return Ok(exit_code::EX_STRICT_FAIL);
    }
    if walked > 0 && (ingested + superseded + already) == 0 {
        return Ok(exit_code::EX_USAGE);
    }
    Ok(0)
}

fn heartbeat_loop(stats: &IngestStats, stop: &HeartbeatStop, interval: Duration) {
    // R2-T4 fix: granularity = min(interval, 100ms). Pre-fix the granularity
    // was hardcoded to 100ms, which meant `PHOTOHELPER_HEARTBEAT_INTERVAL_MS`
    // values below 100 silently floored to 100ms because the first iteration
    // always slept `granularity` before the tick-counter check. Now sub-100ms
    // env overrides actually take effect (used by tests) while production
    // (interval=10s) still gets the 100ms responsive-to-stop-flag behavior.
    //
    // TD-003 closure: the wait below is a Condvar `wait_timeout`, not a
    // blind `thread::sleep`, so `stop.signal()` cuts the wait short and the
    // join in `run_ingest` returns near-instantly (no granularity-cycle
    // latency added to summary printing).
    //
    // Tick BEFORE wait (DN-019 lesson): with a wait-first loop, a fast
    // ingest could finish + signal `stop` before the heartbeat thread was
    // scheduled, leaving the first `wait_for_stop` to observe the signal
    // immediately and return without ever printing. Tick-first guarantees
    // the operator gets at least one liveness signal per `interval` even
    // when the run is shorter than the OS thread-startup latency.
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
                "[heartbeat] walked {}, ingested {}, in-flight {}",
                stats.walked.load(Ordering::Relaxed),
                stats.ingested.load(Ordering::Relaxed),
                stats.in_flight.load(Ordering::Relaxed),
            );
        }
        if stop.wait_for_stop(granularity) {
            return;
        }
    }
}

fn apply_outcome(stats: &IngestStats, outcome: &IngestOutcome) {
    match outcome {
        IngestOutcome::Inserted(_) => {
            stats.ingested.fetch_add(1, Ordering::Relaxed);
        }
        IngestOutcome::SupersededPrevious { .. } => {
            stats.superseded.fetch_add(1, Ordering::Relaxed);
        }
        IngestOutcome::AlreadyCatalogued(_) => {
            stats.already_catalogued.fetch_add(1, Ordering::Relaxed);
        }
        IngestOutcome::SkippedNonRaw => {
            stats.skipped_non_raw.fetch_add(1, Ordering::Relaxed);
        }
        IngestOutcome::SkippedHashWindowTooSmall => {
            stats.skipped_too_small.fetch_add(1, Ordering::Relaxed);
        } // R2-T2: `IngestOutcome::NoExifFields` was deleted as dead code (it
          // was defined but never constructed; `ingest_one` increments
          // `stats.no_exif` directly at the point of decision). The enum is
          // no longer `#[non_exhaustive]`, so this match is exhaustive — a new
          // variant added in a later session that lands without a matching
          // counter will be caught at compile time rather than at runtime.
    }
}

/// The plain-function worker (no `Pipeline` trait per Round 1 Theme 2;
/// no `PipelineCtx` either — just the params plus `stats` so the worker
/// can increment fact-derived counters like `mtime_anomalous` / `unknown_camera`
/// at the point of decision).
pub(crate) fn ingest_one(
    path: &Path,
    root: &AbsPath,
    catalog: &Catalog,
    registry: &CameraRegistry,
    seen_unknown: &Mutex<HashSet<(String, String)>>,
    stats: &IngestStats,
) -> Result<IngestOutcome, Error> {
    if !is_raw_extension(path) {
        tracing::info!(path = %path.display(), "skipped non-RAW extension");
        return Ok(IngestOutcome::SkippedNonRaw);
    }

    let canonical = AbsPath::canonicalize_within(root, path)?;
    let metadata = std::fs::metadata(canonical.as_path()).map_err(|e| Error::Io {
        path: canonical.as_path().to_path_buf(),
        op: "stat",
        source: e,
    })?;
    let file_size = metadata.len();
    if file_size == 0 {
        tracing::warn!(path = %canonical.as_path().display(), "skipped: hash window too small (0 bytes)");
        return Ok(IngestOutcome::SkippedHashWindowTooSmall);
    }

    let raw_mtime: i64 = metadata
        .modified()
        .ok()
        .and_then(|s| s.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    let (clamped_mtime, mtime_anomalous_flag) = clamp_mtime(raw_mtime);
    if mtime_anomalous_flag {
        stats.mtime_anomalous.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(
            path = %canonical.as_path().display(),
            raw_mtime,
            clamped_mtime,
            "mtime clamped to allowed range",
        );
    }

    let photo_id =
        PhotoId::derive_with_clamped_mtime(canonical.as_path(), file_size, clamped_mtime)?;
    // R2-T5 fix: gate the "succeeded with zero fields" WARN on parse-actually-
    // succeeded. Pre-fix: every CR3 emitted TWO contradictory WARNs ("parse
    // failed" + "parse succeeded with zero fields") because `unwrap_or_else`
    // substituted an empty `ExifMetadata::default()` after logging "failed",
    // and the unconditional `if exif.is_empty()` check then logged "succeeded
    // with zero fields" on the same file. User's prod trace on 371 real CR3s
    // produced 740 misleading log lines. The counter still bumps in both
    // cases because the §Observability contract says no-exif rows still ingest.
    let (exif, parse_failed) = match parse_cr3_exif(canonical.as_path()) {
        Ok(e) => (e, false),
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %canonical.as_path().display(),
                "EXIF parse failed"
            );
            (ExifMetadata::default(), true)
        }
    };

    if exif.is_empty() {
        // R1.T1 fix: bump the no_exif counter at the point of decision so the
        // §Observability summary reflects reality. The catalog row still
        // inserts with NULL EXIF columns (DN-006 fallback for CR3 if
        // kamadak-exif can't parse the ISO-BMFF container).
        stats.no_exif.fetch_add(1, Ordering::Relaxed);
        if !parse_failed {
            // R2-T5: only fire this WARN when the parse actually succeeded;
            // on the failure path the "EXIF parse failed" WARN above already
            // told the operator everything.
            tracing::warn!(
                path = %canonical.as_path().display(),
                "EXIF parse succeeded but yielded zero fields"
            );
        }
    }

    let camera_id = if let (Some(m), Some(mo)) = (&exif.make, &exif.model) {
        if let Some(profile) = registry.for_exif(m, mo) {
            Some(profile.id())
        } else {
            stats.unknown_camera.fetch_add(1, Ordering::Relaxed);
            let key = (m.clone(), mo.clone());
            let mut seen = seen_unknown.lock().unwrap_or_else(|p| p.into_inner());
            if seen.insert(key) {
                tracing::warn!(make = %m, model = %mo, "unknown camera (first-seen)");
            } else {
                tracing::info!(make = %m, model = %mo, "unknown camera (subsequent)");
            }
            Some(CameraId::Unknown {
                make: m.clone(),
                model: mo.clone(),
            })
        }
    } else {
        None
    };

    let photo = Photo::from_filesystem(
        canonical,
        file_size,
        clamped_mtime,
        mtime_anomalous_flag,
        photo_id,
        camera_id,
        exif,
    )?;

    let now_unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0);

    // Per-photo errors already carry structured path context via
    // `Error::Io { path, op, .. }` / `Error::CatalogInsert { photo_id, .. }`.
    // Earlier drafts had a no-op `ContextForPath` trait here — deleted
    // in R1.T10 as dead abstraction.
    let outcome = catalog.upsert(&photo, now_unix_seconds)?;

    Ok(match outcome {
        UpsertOutcome::Inserted => IngestOutcome::Inserted(photo_id),
        UpsertOutcome::SupersededPrevious { old } => {
            IngestOutcome::SupersededPrevious { new: photo_id, old }
        }
        UpsertOutcome::AlreadyCatalogued => IngestOutcome::AlreadyCatalogued(photo_id),
    })
}

fn is_raw_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|e| RAW_EXTS.contains(&e.as_str()))
}

/// Parse CR3 EXIF via LibRaw (closes DN-006/DN-011). The previous
/// `parse_exif(path)` used `kamadak-exif`, which silently failed on
/// every real Canon R8 CR3; this replacement orchestrates LibRaw's
/// init/open/unpack/close lifecycle inside `photohelper_raw` and
/// converts the typed `RawExif` to `photohelper-core`'s `ExifMetadata`.
///
/// `photohelper_raw::Error` is converted to `photohelper_core::Error::Exif`
/// at this crate boundary so the rest of the CLI sees a single
/// storage-agnostic error type per the R2-T7 strategy.
fn parse_cr3_exif(path: &Path) -> Result<ExifMetadata, Error> {
    let raw = photohelper_raw::exif::read_cr3(path).map_err(|e| Error::Exif {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    Ok(ExifMetadata {
        make: Some(raw.make().to_string()),
        model: Some(raw.model().to_string()),
        capture_time_unix_seconds: raw.capture_time_unix_seconds(),
        width: Some(raw.width().get()),
        height: Some(raw.height().get()),
        orientation: Some(raw.orientation()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_raw_extension_recognizes_cr3() {
        assert!(is_raw_extension(Path::new("/tmp/photo.cr3")));
        assert!(is_raw_extension(Path::new("/tmp/photo.CR3")));
    }

    #[test]
    fn is_raw_extension_rejects_jpg() {
        assert!(!is_raw_extension(Path::new("/tmp/photo.jpg")));
        assert!(!is_raw_extension(Path::new("/tmp/photo")));
    }

    #[test]
    fn ingest_stats_summary_format() {
        let s = IngestStats::new();
        s.walked.store(5, Ordering::Relaxed);
        s.ingested.store(3, Ordering::Relaxed);
        s.skipped_non_raw.store(2, Ordering::Relaxed);
        let line = s.summary_line();
        assert!(line.contains("walked: 5"));
        assert!(line.contains("ingested: 3"));
        assert!(line.contains("skipped (non-RAW): 2"));
        assert!(line.contains("mtime-anomalous: 0"));
        assert!(line.contains("no-exif: 0"));
    }
}
