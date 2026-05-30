//! Error type for `photohelper-sidecar`.

use std::path::PathBuf;

/// All errors returned by the `photohelper-sidecar` public API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// I/O failure reading or writing a sidecar file.
    #[error("XMP sidecar I/O failed at {path}: {source}")]
    Io {
        /// Sidecar path that caused the error.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// XML parse failure (malformed document structure, not just a bad field value).
    #[error("XMP parse error in {path}: {message}")]
    XmlParse {
        /// Sidecar path that could not be parsed.
        path: PathBuf,
        /// Human-readable description of the parse error.
        message: String,
    },

    /// A `SidecarSettingsBuilder::build()` validation rule was violated.
    #[error("sidecar settings validation: {message}")]
    Validation {
        /// Human-readable description of the failed rule.
        message: String,
    },

    /// Atomic write failed: the temp file was written but `fs::rename` failed.
    #[error("atomic XMP write failed for {path}: {source}")]
    AtomicWrite {
        /// Target sidecar path.
        path: PathBuf,
        /// Underlying I/O error from the rename step.
        #[source]
        source: std::io::Error,
    },
}
