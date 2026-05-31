//! Conflict resolution for XMP sidecar writes.
//!
//! The decision table (per plan § Design decisions § Conflict resolution):
//!
//! | `xmp:MetadataDate` | `ph:LastProcessedAt` | Outcome |
//! |---|---|---|
//! | `Some(md)` | `Some(lp)` | `md > lp` → ConflictPreserved; else Overwritten (merged) |
//! | `Some(_)` | `None` | Overwritten (merges existing Lightroom edits on first run) |
//! | `None` | `Some(_)` | if `is_ours` → Overwritten (merged); else ConflictPreserved (logged at debug) |
//! | `None` | `None` | if any `crs:` field exists and not `is_ours` → ConflictPreserved (logged at debug); else Overwritten (merged) |
//!
//! Additionally, physical file modification time (`mtime`) is checked against `ph:LastProcessedAt`
//! with a 2.0-second safety margin to detect external manual edits.
//!
//! `--force` always produces `ForcedOverwrite`.

use crate::error::Error;
use crate::path::SidecarPath;
use crate::reader::read_xmp;
use crate::settings::SidecarSettings;
use crate::writer::write_xmp;

/// Write strategy for conflict resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Safe merge: applies timestamp decision table.
    Safe,
    /// Force overwrite: overwrites existing sidecar unconditionally.
    ForceOverwrite,
}

/// Outcome of a [`merge_and_write`] call. Maps 1:1 to `DevelopStats` counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WriteOutcome {
    /// New sidecar file created (no prior file existed).
    Created,
    /// Existing sidecar overwritten (our timestamp was newer).
    Overwritten,
    /// Existing XMP settings preserved (Lightroom or another tool is newer,
    /// or timestamps are absent and the existing sidecar has `crs:` data).
    ConflictPreserved,
    /// Existing sidecar overwritten unconditionally (`--force`).
    ForcedOverwrite,
}

/// Write `incoming` settings to `path`, resolving conflicts with any existing
/// sidecar.
///
/// If `strategy` is `ForceOverwrite`, always overwrites and returns `ForcedOverwrite`.
/// Otherwise applies the timestamp decision table (see module doc).
///
/// On successful write/update, the physical filesystem modification time (`mtime`) is
/// aligned exactly with `ph:LastProcessedAt` to prevent false conflict triggers on subsequent runs.
///
/// # Errors
///
/// - [`Error::Io`] / [`Error::XmlParse`] if reading the existing sidecar fails
///   (other than file-not-found, which is treated as "no prior sidecar").
/// - [`Error::Io`] / [`Error::AtomicWrite`] if writing the new sidecar fails.
pub fn merge_and_write(
    path: &SidecarPath,
    incoming: &SidecarSettings,
    strategy: ConflictStrategy,
) -> Result<WriteOutcome, Error> {
    let existing = match read_xmp(path) {
        Ok(settings) => settings,
        Err(e) => {
            if let Error::Io { source, .. } = &e {
                if source.kind() == std::io::ErrorKind::NotFound {
                    crate::writer::write_xmp_force(path, incoming)?;
                    tracing::info!(path = %path.display(), "develop: XMP sidecar created");
                    return Ok(WriteOutcome::Created);
                }
            }
            if strategy == ConflictStrategy::ForceOverwrite && matches!(e, Error::XmlParse { .. }) {
                tracing::warn!(path = %path.display(), error = %e, "force: failed to parse existing XMP; falling back to direct write");
                crate::writer::write_xmp_force(path, incoming)?;
                tracing::info!(path = %path.display(), "develop: XMP sidecar force-overwritten");
                return Ok(WriteOutcome::ForcedOverwrite);
            }
            return Err(e);
        }
    };

    if strategy == ConflictStrategy::ForceOverwrite {
        let merged = existing.merge(incoming);
        write_xmp(path, &merged)?;
        tracing::info!(path = %path.display(), "develop: XMP sidecar force-overwritten");
        return Ok(WriteOutcome::ForcedOverwrite);
    }

    // Correct conflict detection:
    // - `existing.metadata_date()` = xmp:MetadataDate (Lightroom's write timestamp)
    // - `existing.last_processed_at()` = ph:LastProcessedAt (our last write timestamp)
    // Detect external edit: did a third-party tool write AFTER our last develop pass?
    let lightroom_ts = existing.metadata_date(); // xmp:MetadataDate
    let our_ts = existing.last_processed_at(); // ph:LastProcessedAt (no fallback)

    let mut current_mtime = None;
    let mtime_conflict = if let Some(our_time) = our_ts {
        match path.metadata().and_then(|m| m.modified()) {
            Ok(mtime) => {
                current_mtime = Some(mtime);
                let our_system_time = std::time::SystemTime::from(our_time);
                match mtime.duration_since(our_system_time) {
                    Ok(dur) if dur > std::time::Duration::from_secs_f64(2.1) => {
                        tracing::debug!(
                            path = %path.display(),
                            "develop: file mtime is newer than ph:LastProcessedAt by {}s; preserving manual edits",
                            dur.as_secs_f64()
                        );
                        true
                    }
                    _ => false,
                }
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to retrieve sidecar file mtime; aborting to prevent overwrite"
                );
                return Err(Error::Io {
                    path: path.to_path_buf(),
                    source: e,
                });
            }
        }
    } else {
        false
    };

    let is_ours = existing.photohelper_id().is_some()
        && existing.photohelper_id() == incoming.photohelper_id();

    let outcome = if mtime_conflict {
        tracing::debug!(
            path = %path.display(),
            "develop: external filesystem modification detected; preserving existing XMP settings"
        );
        WriteOutcome::ConflictPreserved
    } else {
        match (lightroom_ts, our_ts) {
            (Some(lr_time), Some(our_time)) => {
                // Both timestamps present: Lightroom has updated xmp:MetadataDate after our write?
                if lr_time > our_time {
                    tracing::debug!(
                        path = %path.display(),
                        "develop: Lightroom edited after our last write; preserving XMP settings"
                    );
                    WriteOutcome::ConflictPreserved
                } else {
                    // We are newer (or same time) — safe to update.
                    tracing::info!(path = %path.display(), "develop: updating XMP sidecar");
                    WriteOutcome::Overwritten
                }
            }
            (Some(_), None) => {
                // Existing sidecar has a date (Lightroom-written) but we have never
                // written ph:LastProcessedAt — this is our first photohelper run.
                tracing::info!(
                    path = %path.display(),
                    "develop: existing XMP has metadata date; merging Lightroom edits with incoming settings (first photohelper run)"
                );
                WriteOutcome::Overwritten
            }
            (None, Some(_)) => {
                // Existing sidecar has ph:LastProcessedAt (photohelper-written) but no
                // xmp:MetadataDate — if we own it, we can safely update it. Otherwise,
                // conservatively preserve; the absence of a date is ambiguous.
                if is_ours {
                    tracing::info!(path = %path.display(), "develop: updating owned XMP sidecar despite missing MetadataDate");
                    WriteOutcome::Overwritten
                } else {
                    tracing::debug!(
                        path = %path.display(),
                        "develop: existing XMP has no xmp:MetadataDate; preserving existing XMP settings"
                    );
                    WriteOutcome::ConflictPreserved
                }
            }
            (None, None) => {
                // Neither timestamp present — check for any crs: attribute (not just
                // the 6 numeric ones) to avoid overwriting Lightroom settings like
                // crs:WhiteBalance or crs:CameraProfile (Theme B fix).
                if existing.has_any_crs_attribute() && !is_ours {
                    tracing::debug!(
                        path = %path.display(),
                        "develop: existing XMP has crs: attributes but no timestamps; preserving"
                    );
                    WriteOutcome::ConflictPreserved
                } else {
                    // No crs: attributes or it is ours — safe to overwrite/merge.
                    tracing::info!(path = %path.display(), "develop: overwriting XMP sidecar");
                    WriteOutcome::Overwritten
                }
            }
        }
    };

    if outcome == WriteOutcome::Overwritten {
        let merged = existing.merge(incoming);
        if let Some(mtime) = current_mtime {
            crate::writer::write_xmp_guarded(path, &merged, mtime)?;
        } else {
            write_xmp(path, &merged)?;
        }
    }

    Ok(outcome)
}
