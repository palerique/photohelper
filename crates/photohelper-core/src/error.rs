//! Photohelper error enum.
//!
//! `#[non_exhaustive]`; no `#[from]` derives — every call site maps explicitly
//! via `.map_err(|e| Error::Io { path, op: "...", source: e })`. This
//! discipline prevents `?`-bubbling routing errors to the wrong variant.
//!
//! `CatalogOpen` / `CatalogInsert` carry boxed `dyn Error` sources to keep
//! `photohelper-core` storage-agnostic (the catalog crate boxes its
//! `rusqlite::Error` when constructing these variants).

use std::io;
use std::path::PathBuf;

use crate::model::{CameraId, PhotoId};

/// Boxed source error used by storage-agnostic variants.
pub type BoxedSourceError = Box<dyn std::error::Error + Send + Sync>;

/// All photohelper errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// IO failure with structured context. `op` tags include `"canonicalize"`,
    /// `"canonicalize-nul-check"`, `"read-prefix"`, `"stat"`, `"mkdir-p"`,
    /// `"file-lock"` (R1.T10), and `"lock-file-create"` (R2-T11).
    #[error("IO error at {path} during {op}: {source}")]
    Io {
        /// Path that triggered the error.
        path: PathBuf,
        /// Short tag describing the operation.
        op: &'static str,
        /// Underlying IO error.
        source: io::Error,
    },

    /// EXIF parsing failed.
    #[error("EXIF parse error at {path}: {source}")]
    Exif {
        /// Path whose EXIF could not be parsed.
        path: PathBuf,
        /// Underlying EXIF lib error.
        source: BoxedSourceError,
    },

    /// File too small to derive a meaningful `PhotoId` (e.g., zero bytes).
    #[error("hash window too small for {path} (file size = {len})")]
    HashWindowTooSmall {
        /// Path being hashed.
        path: PathBuf,
        /// File size in bytes.
        len: u64,
    },

    /// Could not open the catalog SQLite connection.
    #[error("could not open catalog at {path}: {source}")]
    CatalogOpen {
        /// Catalog DB path.
        path: PathBuf,
        /// Underlying catalog-backend error (boxed to keep core storage-agnostic).
        source: BoxedSourceError,
    },

    /// Per-photo catalog insert failure.
    #[error("could not insert photo {photo_id}: {source}")]
    CatalogInsert {
        /// Photo whose insert failed.
        photo_id: PhotoId,
        /// Underlying catalog-backend error.
        source: BoxedSourceError,
    },

    /// `--catalog <path>` pointed at an existing directory.
    #[error("catalog path is a directory: {path}")]
    CatalogPathIsDirectory {
        /// The directory path.
        path: PathBuf,
    },

    /// `--catalog <path>` pointed at an existing file whose first 16 bytes
    /// don't match `"SQLite format 3\0"`.
    #[error("catalog path is not a SQLite database: {path}")]
    CatalogPathNotSqlite {
        /// The non-SQLite file path.
        path: PathBuf,
    },

    /// File-lock on `<parent>/.photohelper/catalog.db.lock` could not be
    /// acquired within the retry budget.
    #[error("catalog lock held at {path} (tried {attempts} times over {total_ms}ms)")]
    CatalogLockHeld {
        /// Lock file path.
        path: PathBuf,
        /// Number of attempts made.
        attempts: u32,
        /// Total wall-clock time in milliseconds.
        total_ms: u64,
    },

    /// Catalog DB has a newer schema version than this binary supports.
    #[error(
        "catalog schema version {found} is newer than supported version \
         {expected}; update photohelper or use --catalog with a fresh path"
    )]
    CatalogSchemaTooNew {
        /// Version found in the DB.
        found: i64,
        /// Highest version this binary handles.
        expected: i64,
    },

    /// A worker panicked while holding the catalog mutex. Catalog is dead;
    /// every subsequent operation also returns this error.
    #[error("catalog mutex poisoned at {path}; a worker panicked mid-write")]
    CatalogPoisoned {
        /// Catalog DB path.
        path: PathBuf,
    },

    /// A walked path canonicalized to outside the ingestion root.
    #[error("path escapes ingestion root: {path} not under {root}")]
    PathEscapesRoot {
        /// The offending path.
        path: PathBuf,
        /// The ingestion root.
        root: PathBuf,
    },

    /// A `CameraProfile` stub method was called before session 02 wired up
    /// the real per-ISO noise model / color matrix / sensor layout.
    #[error("camera profile method {method} not yet implemented for {camera_id}")]
    CameraProfileNotImplemented {
        /// Method name.
        method: &'static str,
        /// Camera that lacks the data.
        camera_id: CameraId,
    },

    /// EXIF orientation tag value was outside the valid 1..=8 range.
    /// R1.T11 fix: previously routed through `Error::Exif { path:
    /// PathBuf::new() }` which used an empty sentinel path. The
    /// dedicated variant carries the offending tag and no path —
    /// the caller (a single site in the EXIF parser) attaches its
    /// own path context if needed.
    #[error("invalid EXIF orientation tag: {tag} (valid range 1..=8)")]
    InvalidExifOrientationTag {
        /// The out-of-range tag value found in the EXIF data.
        tag: i64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_for_path_escapes_root_mentions_both_paths() {
        let err = Error::PathEscapesRoot {
            path: PathBuf::from("/etc/passwd"),
            root: PathBuf::from("/photos"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/etc/passwd"));
        assert!(msg.contains("/photos"));
    }

    #[test]
    fn error_display_for_catalog_lock_held_mentions_attempts_and_ms() {
        let err = Error::CatalogLockHeld {
            path: PathBuf::from("/tmp/catalog.db.lock"),
            attempts: 12,
            total_ms: 60_000,
        };
        let msg = format!("{err}");
        assert!(msg.contains("12"));
        assert!(msg.contains("60000"));
    }
}
