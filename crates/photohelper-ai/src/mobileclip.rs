//! CLIP ViT-B/32 image encoder for deduplication embeddings.
//!
//! Model: `laion/CLIP-ViT-B-32-laion2B-s34B-b79K` (MIT), int8-quantized via
//! `scripts/convert-clip-to-onnx.sh`. Output: 512-dim L2-normalized float32
//! embedding (normalization baked into the exported model).
//!
//! Threading model: `Session::run` is `&mut self` (confirmed in ANL-003);
//! one `Session` per rayon worker via `thread_local!`.

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use ort::value::Tensor;
use photohelper_core::RgbImage;

use crate::embedding::ImageEmbedding;
use crate::error::Error;
use crate::model_bytes::VerifiedModelBytes;
use crate::nima::bilinear_resize;
use tracing;

// CLIP ViT-B/32 channel normalization parameters (CLIP-standard, NOT ImageNet).
// Confirmed in ANL-003 §Preprocessing Parameters. The f32 literals carry the
// published precision; the least-significant digits beyond ~7 are ignored by f32.
#[allow(
    clippy::excessive_precision,
    reason = "CLIP-standard constants; trailing digits beyond f32 precision are rounded by the compiler"
)]
const CLIP_MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
#[allow(
    clippy::excessive_precision,
    reason = "CLIP-standard constants; trailing digits beyond f32 precision are rounded by the compiler"
)]
const CLIP_STD: [f32; 3] = [0.268_629_54, 0.261_302_58, 0.275_777_11];

thread_local! {
    // One ort::Session per rayon worker thread. Session::run is &mut self
    // (verified in ANL-003 §Session::run Receiver Type), so sharing one Session
    // across threads would require a Mutex, serialising all inference.
    static SESS: RefCell<Option<ort::session::Session>> = const { RefCell::new(None) };
}

/// CLIP ViT-B/32 visual encoder: produces 512-dim L2-normalized embeddings.
///
/// Wraps [`VerifiedModelBytes`] and constructs a per-worker
/// `ort::Session` lazily on first use via `thread_local!`.
pub struct MobileClip {
    bytes: Arc<[u8]>,
}

static_assertions::assert_impl_all!(MobileClip: Send, Sync);

impl std::fmt::Debug for MobileClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MobileClip")
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

impl MobileClip {
    /// Construct from verified CLIP model bytes.
    pub fn new(model: &VerifiedModelBytes) -> Self {
        Self {
            bytes: model.bytes(),
        }
    }

    /// Embed an RGB image and return a 512-dim L2-normalized embedding.
    ///
    /// Preprocessing (CLIP-standard, confirmed in ANL-003):
    /// 1. Bilinear resize to 224×224 (TD-020: bicubic center-crop deferred).
    /// 2. Convert u8 → f32 in [0.0, 1.0].
    /// 3. Normalize per channel: `(pixel - CLIP_MEAN[c]) / CLIP_STD[c]`.
    /// 4. Transpose HWC → CHW; add batch dim → NCHW [1, 3, 224, 224].
    /// 5. Run ort inference (per-worker thread_local Session).
    /// 6. L2-norm is baked into the model; verify via `ImageEmbedding::from_raw`.
    ///
    /// # Errors
    ///
    /// - `ModelLoad` if Session construction fails on the calling thread.
    /// - `EmbeddingZeroVector` if the model outputs an all-zeros vector.
    /// - `EmbeddingNotNormalized` if the output norm is not near 1.0.
    /// - `MobileClipInferenceFailed` if ort inference errors.
    pub fn embed(&self, rgb: &RgbImage, path: &Path) -> Result<ImageEmbedding, Error> {
        // ── 1. Resize to 224×224 (HWC u8 buffer) ────────────────────────────
        let resized = bilinear_resize(rgb, 224, 224); // Vec<u8>, len = 224*224*3

        // ── 2+3. Float conversion + CLIP normalization ────────────────────────
        // ── 4. Transpose HWC → CHW into NCHW [1, 3, 224, 224] ────────────────
        // All index computations are in-bounds by construction:
        //   hwc_idx  = row*W*C + col*C + ch < H*W*C = resized.len()
        //   chw_idx  = ch*H*W + row*W + col < C*H*W = input.len()
        //   ch       ∈ 0..3 = CLIP_MEAN.len() = CLIP_STD.len()
        #[allow(
            clippy::indexing_slicing,
            reason = "bounds provably safe by loop invariants: hwc/chw indices < vec lengths; ch < 3"
        )]
        let input = {
            let (h, w, c) = (224_usize, 224_usize, 3_usize);
            let mut buf = vec![0.0_f32; c * h * w];
            for row in 0..h {
                for col in 0..w {
                    for ch in 0..c {
                        let hwc_idx = row * w * c + col * c + ch;
                        let chw_idx = ch * h * w + row * w + col;
                        let raw = f32::from(resized[hwc_idx]) / 255.0;
                        buf[chw_idx] = (raw - CLIP_MEAN[ch]) / CLIP_STD[ch];
                    }
                }
            }
            buf
        };

        // ── 5. Run ort inference ──────────────────────────────────────────────
        let raw_emb: Vec<f32> = SESS.with(|cell| -> Result<Vec<f32>, Error> {
            let mut guard = cell.borrow_mut();
            if guard.is_none() {
                // If construction fails, guard stays None; the next embed() call on
                // this thread retries. For deterministic failures, this produces N
                // retries (one per photo). tracing::error! surfaces the root cause.
                let sess = ort::session::Session::builder()
                    .map_err(|e| {
                        tracing::error!("CLIP model builder failed: {e}; this worker will retry on each photo");
                        Error::ModelLoad { source: Box::new(e) }
                    })?
                    .commit_from_memory(&self.bytes)
                    .map_err(|e| {
                        tracing::error!("CLIP commit_from_memory failed: {e}; this worker will retry on each photo");
                        Error::ModelLoad { source: Box::new(e) }
                    })?;
                *guard = Some(sess);
            }
            // SAFETY(unwrap): guard is Some here — the if-is_none block above
            // either sets *guard = Some(_) or returns Err.
            #[allow(
                clippy::unwrap_used,
                reason = "guard proven Some: if-is_none branch either inserts Some or returns Err"
            )]
            let sess = guard.as_mut().unwrap();

            // Input tensor: (1, 3, 224, 224) NCHW f32.
            let input_tensor =
                Tensor::<f32>::from_array(([1_usize, 3, 224, 224], input.into_boxed_slice()))
                    .map_err(|e| Error::MobileClipInferenceFailed {
                        path: path.to_path_buf(),
                        source: Box::new(e),
                    })?;

            // Extract names before calling run() (avoids borrow conflict).
            let input_name: String = sess
                .inputs()
                .first()
                .map_or_else(|| "pixel_values".to_owned(), |i| i.name().to_owned());
            let output_name: String = sess
                .outputs()
                .first()
                .map_or_else(|| "image_embeds".to_owned(), |i| i.name().to_owned());

            let session_inputs = ort::inputs![input_name.as_str() => input_tensor];
            let outputs =
                sess.run(session_inputs)
                    .map_err(|e| Error::MobileClipInferenceFailed {
                        path: path.to_path_buf(),
                        source: Box::new(e),
                    })?;

            let first = outputs.get(output_name.as_str()).ok_or_else(|| {
                Error::MobileClipInferenceFailed {
                    path: path.to_path_buf(),
                    source: format!("output '{output_name}' not found in ort output map").into(),
                }
            })?;

            let (_, data) = first.try_extract_tensor::<f32>().map_err(|e| {
                Error::MobileClipInferenceFailed {
                    path: path.to_path_buf(),
                    source: Box::new(e),
                }
            })?;

            Ok(data.to_vec())
        })?;

        // ── 6. Validate and wrap ──────────────────────────────────────────────
        // The int8 ONNX model has L2-normalization baked in. Verify via from_raw.
        // Check for zero vector before normalizing (model failure mode → EmbeddingZeroVector).
        let norm_sq: f32 = raw_emb.iter().map(|x| x * x).sum();
        if norm_sq < f32::EPSILON {
            return Err(Error::EmbeddingZeroVector);
        }
        // from_raw validates norm ∈ [0.99, 1.01]; rejects NaN/Inf.
        ImageEmbedding::from_raw(&raw_emb).map_err(|e| Error::MobileClipInferenceFailed {
            path: path.to_path_buf(),
            source: e.to_string().into(),
        })
    }
}
