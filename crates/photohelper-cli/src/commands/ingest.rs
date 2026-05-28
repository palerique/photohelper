//! `photohelper ingest <path>` — walk directory, derive PhotoId, write
//! catalog rows.
//!
//! See `docs/plans/session-01.md` §Deliverables 5 + §Observability contract.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::Context as _;
use rayon::iter::ParallelBridge;
use rayon::iter::ParallelIterator;

use photohelper_cameras::CameraRegistry;
use photohelper_catalog::{Catalog, UpsertOutcome};
use photohelper_core::Error;
use photohelper_core::model::{
    AbsPath, CameraId, ExifMetadata, ExifOrientation, IngestOutcome, Photo, PhotoId, clamp_mtime,
};

use crate::{Cli, IngestArgs, exit_code};

const RAW_EXTS: &[&str] = &["cr3", "cr2", "arw", "nef", "raf", "orf", "rw2", "dng"];

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

    // Heartbeat thread: spawned (handle retained for is_finished check),
    // never joined; reads Arc<AtomicBool> stop flag.
    let stop_flag = Arc::new(AtomicBool::new(false));
    let heartbeat_handle = {
        let stats = Arc::clone(&stats);
        let stop = Arc::clone(&stop_flag);
        thread::spawn(move || heartbeat_loop(&stats, &stop, heartbeat_interval()))
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
    stop_flag.store(true, Ordering::Relaxed);
    // The heartbeat thread observes the flag within `granularity` (100ms)
    // and exits cleanly. We don't .join() — that would add up to
    // HEARTBEAT_INTERVAL latency to summary printing.

    eprintln!("{}", stats.summary_line());

    let walked = stats.walked.load(Ordering::Relaxed);
    let ingested = stats.ingested.load(Ordering::Relaxed);
    let superseded = stats.superseded.load(Ordering::Relaxed);
    let already = stats.already_catalogued.load(Ordering::Relaxed);
    let unknown = stats.unknown_camera.load(Ordering::Relaxed);
    let anomalous = stats.mtime_anomalous.load(Ordering::Relaxed);
    let errored = stats.errored.load(Ordering::Relaxed);

    if args.strict && (unknown > 0 || anomalous > 0 || errored > 0) {
        return Ok(exit_code::EX_STRICT_FAIL);
    }
    if walked > 0 && (ingested + superseded + already) == 0 {
        return Ok(exit_code::EX_USAGE);
    }
    Ok(0)
}

fn heartbeat_loop(stats: &IngestStats, stop: &AtomicBool, interval: Duration) {
    let granularity = Duration::from_millis(100);
    let ticks = (interval.as_millis() / granularity.as_millis()).max(1) as u64;
    let mut counter: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(granularity);
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
        }
        IngestOutcome::NoExifFields => {
            stats.no_exif.fetch_add(1, Ordering::Relaxed);
        }
        // IngestOutcome is `#[non_exhaustive]` so a wildcard is mandatory.
        // A new variant added in a later session that lands without a
        // matching counter would log here as a TODO — see `docs/discovery-notes.md`.
        _ => {
            tracing::warn!("unaccounted IngestOutcome variant; summary counters out of sync");
        }
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
    let exif = parse_exif(canonical.as_path()).unwrap_or_else(|err| {
        tracing::warn!(error = %err, path = %canonical.as_path().display(), "EXIF parse failed");
        ExifMetadata::default()
    });

    if exif.is_empty() {
        // R1.T1 fix: bump the no_exif counter at the point of decision so
        // the §Observability summary reflects reality. The catalog row
        // still inserts with NULL EXIF columns (DN-006 fallback for CR3
        // if kamadak-exif can't parse the ISO-BMFF container).
        stats.no_exif.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(path = %canonical.as_path().display(), "EXIF parse succeeded with zero fields");
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

fn parse_exif(path: &Path) -> Result<ExifMetadata, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        op: "read-prefix",
        source: e,
    })?;
    let mut buf = std::io::BufReader::new(file);
    let reader = exif::Reader::new();
    let exif_data = match reader.read_from_container(&mut buf) {
        Ok(d) => d,
        Err(e) => {
            return Err(Error::Exif {
                path: path.to_path_buf(),
                source: Box::new(e),
            });
        }
    };

    let mut out = ExifMetadata::default();
    for field in exif_data.fields() {
        match field.tag {
            exif::Tag::Make => {
                out.make = field
                    .display_value()
                    .to_string()
                    .trim_matches('"')
                    .to_string()
                    .into();
            }
            exif::Tag::Model => {
                out.model = field
                    .display_value()
                    .to_string()
                    .trim_matches('"')
                    .to_string()
                    .into();
            }
            exif::Tag::PixelXDimension => {
                out.width = field.value.get_uint(0);
            }
            exif::Tag::PixelYDimension => {
                out.height = field.value.get_uint(0);
            }
            exif::Tag::Orientation => {
                if let Some(tag) = field.value.get_uint(0) {
                    if let Ok(orientation) = ExifOrientation::from_tag(i64::from(tag)) {
                        out.orientation = Some(orientation);
                    }
                }
            }
            exif::Tag::DateTimeOriginal => {
                // Best-effort EXIF datetime parsing: "YYYY:MM:DD HH:MM:SS"
                let s = field.display_value().to_string();
                if let Some(secs) = parse_exif_datetime(&s) {
                    out.capture_time_unix_seconds = Some(secs);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn parse_exif_datetime(s: &str) -> Option<i64> {
    // EXIF format: "YYYY:MM:DD HH:MM:SS"
    use time::{Date, Month, PrimitiveDateTime, Time};
    let s = s.trim_matches('"');
    let mut parts = s.split(' ');
    let date = parts.next()?;
    let time_s = parts.next()?;
    let date_parts: Vec<&str> = date.split(':').collect();
    let time_parts: Vec<&str> = time_s.split(':').collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return None;
    }
    let year: i32 = date_parts.first()?.parse().ok()?;
    let month_n: u8 = date_parts.get(1)?.parse().ok()?;
    let day: u8 = date_parts.get(2)?.parse().ok()?;
    let hour: u8 = time_parts.first()?.parse().ok()?;
    let minute: u8 = time_parts.get(1)?.parse().ok()?;
    let second: u8 = time_parts.get(2)?.parse().ok()?;
    let month = Month::try_from(month_n).ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;
    let time = Time::from_hms(hour, minute, second).ok()?;
    let dt = PrimitiveDateTime::new(date, time).assume_utc();
    Some(dt.unix_timestamp())
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
