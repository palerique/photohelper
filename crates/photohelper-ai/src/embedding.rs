//! `ImageEmbedding` — a validated, L2-normalized image embedding vector.

#![forbid(unsafe_code)]

use std::sync::Arc;

use crate::Error;

/// An L2-normalized 512-dim float32 image embedding produced by the CLIP visual encoder.
///
/// # Invariants
///
/// - `dim() > 0` (non-empty)
/// - L2-norm ∈ [0.99, 1.01] (near-unit; baked into the exported ONNX model)
///
/// Cheaply clonable: the inner `Arc<[f32]>` is ref-counted.
#[derive(Debug, Clone)]
pub struct ImageEmbedding(Arc<[f32]>);

impl ImageEmbedding {
    /// Construct from a raw float vector, validating the L2-norm invariant.
    ///
    /// # Errors
    ///
    /// - [`Error::EmbeddingEmpty`] if `vec` is empty
    /// - [`Error::EmbeddingNotNormalized`] if the L2-norm is NaN, Inf, or outside [0.99, 1.01]
    pub fn from_raw(slice: &[f32]) -> Result<Self, Error> {
        if slice.is_empty() {
            return Err(Error::EmbeddingEmpty);
        }
        let norm = l2_norm(slice);
        // NaN comparisons always return false, so check is_finite() first.
        if !norm.is_finite() || !(0.99..=1.01).contains(&norm) {
            return Err(Error::EmbeddingNotNormalized { norm });
        }
        Ok(Self(Arc::from(slice)))
    }

    /// Number of dimensions.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.0.len()
    }

    /// Raw float slice for future use by `threshold_cluster` in `dedup.rs` (D3, not yet
    /// implemented). Currently called only by tests.
    #[must_use]
    #[allow(
        dead_code,
        reason = "called only by tests; will be used by dedup.rs (threshold_cluster, D3) — not yet implemented"
    )]
    pub(crate) fn as_slice(&self) -> &[f32] {
        &self.0
    }

    /// Cosine similarity with `other` in [-1.0, 1.0].
    ///
    /// Since both embeddings are L2-normalized, cosine similarity equals the dot product.
    /// The result is clamped to [-1.0, 1.0] to guard against floating-point overshoot.
    ///
    /// In `threshold_cluster`, add `debug_assert!(all embeddings have equal dim)` before
    /// the O(n²) pair loop — dimension mismatch within a single model is a programming error.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmbeddingDimMismatch`] if `self.dim() != other.dim()`.
    pub fn cosine_similarity(&self, other: &Self) -> Result<f32, Error> {
        if self.dim() != other.dim() {
            return Err(Error::EmbeddingDimMismatch {
                expected: self.dim(),
                got: other.dim(),
            });
        }
        let dot: f32 = self.0.iter().zip(other.0.iter()).map(|(a, b)| a * b).sum();
        Ok(dot.clamp(-1.0, 1.0))
    }

    /// Serialize to little-endian float32 bytes for catalog BLOB storage.
    #[must_use]
    pub fn as_f32_le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.0.len() * 4);
        for &v in self.0.iter() {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Deserialize from little-endian float32 bytes (catalog BLOB).
    ///
    /// Calls [`from_raw`](Self::from_raw) to re-validate the L2-norm invariant.
    ///
    /// # Errors
    ///
    /// - [`Error::EmbeddingEmpty`] if `bytes` is empty
    /// - [`Error::EmbeddingNotNormalized`] if byte-slice length is not a multiple of 4,
    ///   or if the deserialized norm is not finite or outside [0.99, 1.01]
    pub fn from_f32_le_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.is_empty() {
            return Err(Error::EmbeddingEmpty);
        }
        if bytes.len() % 4 != 0 {
            return Err(Error::EmbeddingCorruptBytes { len: bytes.len() });
        }
        // chunks_exact(4) guarantees exactly 4-byte chunks; try_into().ok() cannot fail.
        let vec: Vec<f32> = bytes
            .chunks_exact(4)
            .filter_map(|c| <[u8; 4]>::try_from(c).ok().map(f32::from_le_bytes))
            .collect();
        Self::from_raw(&vec)
    }
}

/// Compute the L2-norm of a float slice.
fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

static_assertions::assert_impl_all!(ImageEmbedding: Send, Sync);

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics on test failures are intentional and expected"
)]
mod tests {
    use super::*;

    fn make_unit_vec(dim: usize, val: f32) -> Vec<f32> {
        let mut v = vec![0.0_f32; dim];
        v[0] = val;
        // L2-normalize so norm == 1.0
        let n = l2_norm(&v);
        v.iter_mut().for_each(|x| *x /= n);
        v
    }

    #[test]
    fn from_raw_happy_path() {
        let v = make_unit_vec(512, 1.0);
        let emb = ImageEmbedding::from_raw(&v).unwrap();
        assert_eq!(emb.dim(), 512);
        let norm = l2_norm(emb.as_slice());
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[test]
    fn from_raw_rejects_empty() {
        let err = ImageEmbedding::from_raw(&[]).unwrap_err();
        assert!(matches!(err, Error::EmbeddingEmpty));
    }

    #[test]
    fn from_raw_rejects_unnormalized() {
        // norm = sqrt(4) = 2.0, outside [0.99, 1.01]
        let v = vec![1.0_f32; 4];
        let err = ImageEmbedding::from_raw(&v).unwrap_err();
        assert!(matches!(err, Error::EmbeddingNotNormalized { .. }));
    }

    #[test]
    fn from_raw_rejects_nan_and_inf_norm() {
        // NaN: norm computation produces NaN → fails is_finite() check
        let nan_v = vec![f32::NAN, 0.0, 0.0, 0.0];
        let err = ImageEmbedding::from_raw(&nan_v).unwrap_err();
        assert!(matches!(err, Error::EmbeddingNotNormalized { norm } if !norm.is_finite()));

        // Inf: l2_norm produces Inf → fails is_finite() check
        let inf_v = vec![f32::INFINITY, 0.0, 0.0, 0.0];
        let err2 = ImageEmbedding::from_raw(&inf_v).unwrap_err();
        assert!(matches!(err2, Error::EmbeddingNotNormalized { norm } if !norm.is_finite()));
    }

    #[test]
    fn cosine_similarity_happy_path() {
        let v = make_unit_vec(512, 1.0);
        let a = ImageEmbedding::from_raw(&v).unwrap();
        let b = ImageEmbedding::from_raw(&v).unwrap();
        let sim = a.cosine_similarity(&b).unwrap();
        assert!((sim - 1.0).abs() < 1e-4, "sim={sim}");

        // Orthogonal unit vectors: sim ≈ 0.0
        let mut v2 = vec![0.0_f32; 512];
        v2[1] = 1.0;
        let c = ImageEmbedding::from_raw(&v2).unwrap();
        let sim2 = a.cosine_similarity(&c).unwrap();
        assert!(sim2.abs() < 1e-4, "sim2={sim2}");
    }

    #[test]
    fn cosine_similarity_dim_mismatch() {
        let a = ImageEmbedding::from_raw(&make_unit_vec(512, 1.0)).unwrap();
        let b = ImageEmbedding::from_raw(&make_unit_vec(256, 1.0)).unwrap();
        let err = a.cosine_similarity(&b).unwrap_err();
        assert!(
            matches!(
                err,
                Error::EmbeddingDimMismatch {
                    expected: 512,
                    got: 256
                }
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn from_f32_le_bytes_round_trip() {
        let v = make_unit_vec(512, 1.0);
        let original = ImageEmbedding::from_raw(&v).unwrap();
        let bytes = original.as_f32_le_bytes();
        assert_eq!(bytes.len(), 512 * 4);
        let roundtripped = ImageEmbedding::from_f32_le_bytes(&bytes).unwrap();
        let sim = original.cosine_similarity(&roundtripped).unwrap();
        assert!((sim - 1.0).abs() < 1e-6, "round-trip cosine_sim={sim}");
    }

    #[test]
    fn from_f32_le_bytes_rejects_empty() {
        let err = ImageEmbedding::from_f32_le_bytes(&[]).unwrap_err();
        assert!(matches!(err, Error::EmbeddingEmpty));
    }

    #[test]
    fn from_f32_le_bytes_rejects_non_aligned() {
        let bytes = vec![0u8; 13]; // 13 is not a multiple of 4
        let err = ImageEmbedding::from_f32_le_bytes(&bytes).unwrap_err();
        assert!(matches!(err, Error::EmbeddingCorruptBytes { len: 13 }));
    }

    #[test]
    fn cosine_similarity_antipodal_returns_negative() {
        // Two antipodal unit vectors: sim ≈ -1.0
        let mut v = make_unit_vec(512, 1.0);
        let a = ImageEmbedding::from_raw(&v).unwrap();
        // Negate — norm stays 1.0, all elements flip sign.
        v.iter_mut().for_each(|x| *x = -*x);
        let b = ImageEmbedding::from_raw(&v).unwrap();
        let sim = a.cosine_similarity(&b).unwrap();
        assert!(sim <= -0.99, "antipodal sim must be ≈ -1.0; got {sim}");
        assert!(sim >= -1.0, "cosine_similarity must clamp to [-1.0, 1.0]");
    }
}
