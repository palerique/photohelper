//! Conflict resolution for XMP sidecar writes.
//!
//! The 4-case decision table (per plan § Design decisions § Conflict resolution):
//!
//! | `xmp:MetadataDate` | `ph:LastProcessedAt` | Outcome |
//! |---|---|---|
//! | `Some(md)` | `Some(lp)` | `md > lp` → ConflictPreserved; else Overwritten (merged) |
//! | `Some(_)` | `None` | Overwritten (merges existing Lightroom edits on first run) |
//! | `None` | `Some(_)` | ConflictPreserved + warn |
//! | `None` | `None` | if any crs: field exists → ConflictPreserved + warn; else Overwritten (merged) |
//!
//! `--force` always produces `ForcedOverwrite`.

use std::path::Path;

use crate::error::Error;
use crate::reader::read_xmp;
use crate::settings::SidecarSettings;
use crate::writer::write_xmp;

/// Outcome of a [`merge_and_write`] call. Maps 1:1 to `DevelopStats` counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WriteOutcome {
    /// New sidecar file created (no prior file existed).
    Created,
    /// Existing sidecar overwritten (our timestamp was newer).
    Overwritten,
    /// Existing `crs:` settings preserved (Lightroom or another tool is newer,
    /// or timestamps are absent and the existing sidecar has `crs:` data).
    ConflictPreserved,
    /// Existing sidecar overwritten unconditionally (`--force`).
    ForcedOverwrite,
}

/// Write `incoming` settings to `path`, resolving conflicts with any existing
/// sidecar.
///
/// If `force` is `true`, always overwrites and returns `ForcedOverwrite`.
/// Otherwise applies the 4-case timestamp decision table (see module doc).
///
/// # Errors
///
/// - [`Error::Io`] / [`Error::XmlParse`] if reading the existing sidecar fails
///   (other than file-not-found, which is treated as "no prior sidecar").
/// - [`Error::Io`] / [`Error::AtomicWrite`] if writing the new sidecar fails.
pub fn merge_and_write(
    path: &Path,
    incoming: &SidecarSettings,
    force: bool,
) -> Result<WriteOutcome, Error> {
    if force {
        let to_write = if path.exists() {
            match read_xmp(path) {
                Ok(existing) => existing.merge(incoming),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "force: failed to read existing XMP; falling back to direct write");
                    incoming.clone()
                }
            }
        } else {
            incoming.clone()
        };
        write_xmp(path, &to_write)?;
        tracing::info!(path = %path.display(), "develop: XMP sidecar force-overwritten");
        return Ok(WriteOutcome::ForcedOverwrite);
    }

    // No existing sidecar — create new.
    if !path.exists() {
        write_xmp(path, incoming)?;
        tracing::info!(path = %path.display(), "develop: XMP sidecar created");
        return Ok(WriteOutcome::Created);
    }

    // Existing sidecar — read and apply conflict resolution.
    let existing = read_xmp(path)?;

    // Correct conflict detection (Theme A + B fix):
    // - `existing.metadata_date()` = xmp:MetadataDate (Lightroom's write timestamp)
    // - `existing.last_processed_at()` = ph:LastProcessedAt (our last write timestamp)
    // Detect external edit: did a third-party tool write AFTER our last develop pass?
    let lightroom_ts = existing.metadata_date(); // xmp:MetadataDate
    let our_ts = existing.last_processed_at(); // ph:LastProcessedAt (no fallback)

    let outcome = match (lightroom_ts, our_ts) {
        (Some(lr_time), Some(our_time)) => {
            // Both timestamps present: Lightroom has updated xmp:MetadataDate after our write?
            if lr_time > our_time {
                tracing::info!(
                    path = %path.display(),
                    "develop: Lightroom edited after our last write; preserving crs: settings"
                );
                WriteOutcome::ConflictPreserved
            } else {
                // We are newer (or same time) — safe to update.
                tracing::info!(path = %path.display(), "develop: updating XMP sidecar");
                let merged = existing.merge(incoming);
                write_xmp(path, &merged)?;
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
            let merged = existing.merge(incoming);
            write_xmp(path, &merged)?;
            WriteOutcome::Overwritten
        }
        (None, Some(_)) => {
            // Existing sidecar has ph:LastProcessedAt (photohelper-written) but no
            // xmp:MetadataDate — if we own it, we can safely update it. Otherwise,
            // conservatively preserve; the absence of a date is ambiguous.
            let is_ours = existing.photohelper_id().is_some()
                && existing.photohelper_id() == incoming.photohelper_id();
            if is_ours {
                tracing::info!(path = %path.display(), "develop: updating owned XMP sidecar despite missing MetadataDate");
                let merged = existing.merge(incoming);
                write_xmp(path, &merged)?;
                WriteOutcome::Overwritten
            } else {
                tracing::warn!(
                    path = %path.display(),
                    "develop: existing XMP has no xmp:MetadataDate; preserving existing crs: settings"
                );
                WriteOutcome::ConflictPreserved
            }
        }
        (None, None) => {
            // Neither timestamp present — check for any crs: attribute (not just
            // the 6 numeric ones) to avoid overwriting Lightroom settings like
            // crs:WhiteBalance or crs:CameraProfile (Theme B fix).
            let is_ours = existing.photohelper_id().is_some()
                && existing.photohelper_id() == incoming.photohelper_id();
            if existing.has_any_crs_attribute() && !is_ours {
                tracing::warn!(
                    path = %path.display(),
                    "develop: existing XMP has crs: attributes but no timestamps; preserving"
                );
                WriteOutcome::ConflictPreserved
            } else {
                // No crs: attributes or it is ours — safe to overwrite/merge.
                tracing::info!(path = %path.display(), "develop: overwriting XMP sidecar");
                let merged = existing.merge(incoming);
                write_xmp(path, &merged)?;
                WriteOutcome::Overwritten
            }
        }
    };

    Ok(outcome)
}
