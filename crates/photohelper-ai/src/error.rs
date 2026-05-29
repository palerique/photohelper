//! Error types for photohelper-ai.

use std::path::PathBuf;

/// All errors from the photohelper-ai crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// SHA-256 of the model file does not match the manifest.
    #[error("model SHA-256 mismatch for {name}: expected {expected}, got {actual}")]
    ModelSha256Mismatch {
        /// Model name (e.g. `"nima_mobilenet_aesthetic"`).
        name: String,
        /// Expected SHA-256 hex string from manifest.toml.
        expected: String,
        /// Actual SHA-256 hex string of the file on disk.
        actual: String,
    },

    /// Failed to read or parse manifest.toml.
    #[error("manifest parse error at {path}: {source}")]
    ManifestParse {
        /// Path to manifest.toml.
        path: PathBuf,
        /// Underlying parse error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// manifest.toml file not found.
    #[error("manifest not found at {path}")]
    ManifestNotFound {
        /// Expected manifest.toml path.
        path: PathBuf,
    },

    /// Failed to load / construct an ort Session from model bytes.
    #[error("model load failed: {source}")]
    ModelLoad {
        /// Underlying ort error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// ort inference failed for a specific photo.
    #[error("inference failed for {path}: {source}")]
    InferenceFailed {
        /// Source path of the photo being scored.
        path: PathBuf,
        /// Underlying error (ort runtime or output-validation failure).
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// `NimaScore::from_f32` received an out-of-range or NaN value.
    #[error("NIMA score out of range [{min}, {max}] or NaN: {value}")]
    ScoreOutOfRange {
        /// The invalid score value.
        value: f32,
        /// Minimum valid value (1.0).
        min: f32,
        /// Maximum valid value (10.0).
        max: f32,
    },
}
