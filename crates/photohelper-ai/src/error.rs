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

    /// `ImageEmbedding::from_raw` received an empty vector.
    #[error("embedding vector is empty (zero dimensions)")]
    EmbeddingEmpty,

    /// `ImageEmbedding::from_f32_le_bytes` received a byte slice whose length is
    /// not a multiple of 4 (cannot represent any sequence of `f32` values).
    #[error("embedding bytes are corrupt: length {len} is not a multiple of 4")]
    EmbeddingCorruptBytes {
        /// The actual byte slice length.
        len: usize,
    },

    /// `ImageEmbedding::from_raw` received a vector whose L2-norm is NaN, Inf,
    /// or outside the expected range [0.99, 1.01].
    ///
    /// The norm must be finite and near-unit; `MobileClip::embed` L2-normalizes
    /// before constructing `ImageEmbedding`, so this error signals model misbehaviour.
    #[error("embedding L2-norm is not finite or out of range [0.99, 1.01]: {norm}")]
    EmbeddingNotNormalized {
        /// The actual L2-norm of the embedding vector.
        norm: f32,
    },

    /// Model emitted an all-zeros embedding vector; L2-normalization would produce NaN.
    #[error("model produced a zero-length embedding vector (all values ≈ 0)")]
    EmbeddingZeroVector,

    /// `ImageEmbedding::cosine_similarity` received embeddings of different dimensions.
    #[error("embedding dimension mismatch: expected {expected}, got {got}")]
    EmbeddingDimMismatch {
        /// Expected dimension (self.dim()).
        expected: usize,
        /// Actual dimension of the other embedding.
        got: usize,
    },

    /// CLIP inference failed for a specific photo.
    #[error("MobileCLIP inference failed for {path}: {source}")]
    MobileClipInferenceFailed {
        /// Source path of the photo being embedded.
        path: PathBuf,
        /// Underlying error (ort runtime or output-validation failure).
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
