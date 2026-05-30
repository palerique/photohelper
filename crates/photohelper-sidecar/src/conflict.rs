//! Conflict resolution for XMP sidecar writes.
//!
//! The 4-case decision table (per plan § Design decisions § Conflict resolution):
//!
//! | `xmp:MetadataDate` | `ph:LastProcessedAt` | Outcome |
//! |---|---|---|
//! | `Some(md)` | `Some(lp)` | `md > lp` → ConflictPreserved; else Overwritten |
//! | `Some(_)` | `None` | ConflictPreserved (first photohelper run) |
//! | `None` | `Some(_)` | ConflictPreserved + warn |
//! | `None` | `None` | if any crs: field exists → ConflictPreserved + warn; else Created |
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
        write_xmp(path, incoming)?;
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
    let existing_ts = existing.last_processed_at(); // xmp:MetadataDate or ph:LastProcessedAt
    let incoming_ts = incoming.last_processed_at();

    let outcome = match (existing_ts, incoming_ts) {
        (Some(md), Some(lp)) => {
            if md > lp {
                tracing::info!(
                    path = %path.display(),
                    "develop: existing XMP is newer; preserving crs: settings"
                );
                WriteOutcome::ConflictPreserved
            } else {
                tracing::info!(path = %path.display(), "develop: updating XMP sidecar");
                write_xmp(path, incoming)?;
                WriteOutcome::Overwritten
            }
        }
        (Some(_), None) => {
            // Existing file has a timestamp but this is our first photohelper run.
            tracing::info!(
                path = %path.display(),
                "develop: existing XMP has metadata date; preserving (first photohelper run)"
            );
            WriteOutcome::ConflictPreserved
        }
        (None, Some(_)) => {
            // Existing file has no timestamp but we do — conservative preserve.
            tracing::warn!(
                path = %path.display(),
                "develop: existing XMP has no metadata date; preserving existing crs: settings"
            );
            WriteOutcome::ConflictPreserved
        }
        (None, None) => {
            // Neither has a timestamp.
            if existing.has_crs_fields() {
                tracing::warn!(
                    path = %path.display(),
                    "develop: existing XMP has crs: fields but no timestamps; preserving"
                );
                WriteOutcome::ConflictPreserved
            } else {
                // No crs: fields and no timestamps — safe to overwrite.
                tracing::info!(path = %path.display(), "develop: creating XMP sidecar (no prior crs:)");
                write_xmp(path, incoming)?;
                WriteOutcome::Created
            }
        }
    };

    Ok(outcome)
}
