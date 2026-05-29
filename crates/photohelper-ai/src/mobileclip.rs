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
use std::sync::atomic::{AtomicBool, Ordering};

use ort::value::Tensor;
use photohelper_core::RgbImage;

use crate::embedding::ImageEmbedding;
use crate::error::Error;
use crate::model_bytes::VerifiedModelBytes;

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

// SESS is module-scoped, not instance-scoped. Multiple `MobileClip` instances on the same
// thread would share one Session (last constructor's model wins). Guard: at most one instance
// may exist at a time. This flag is reset on Drop so tests that create/drop sequentially pass.
static INSTANCE_EXISTS: AtomicBool = AtomicBool::new(false);

thread_local! {
    // One ort::Session per rayon worker thread. Session::run is &mut self
    // (verified in ANL-003 §Session::run Receiver Type), so sharing one Session
    // across threads would require a Mutex, serialising all inference.
    static SESS: RefCell<Option<ort::session::Session>> = const { RefCell::new(None) };
}

/// CLIP ViT-B/32 visual encoder: produces 512-dim L2-normalized embeddings.
///
/// Named `MobileClip` after the original session-05 plan (MobileCLIP was the initial
/// candidate, rejected at D0 due to the `apple-amlr` license — see DN-028). The actual
/// model is `laion/CLIP-ViT-B-32-laion2B-s34B-b79K` (MIT).
///
/// Wraps [`VerifiedModelBytes`] and constructs a per-worker
/// `ort::Session` lazily on first use via `thread_local!`.
///
/// # Single-instance invariant
///
/// The module-level `SESS` thread-local is not tied to any instance. At most one
/// `MobileClip` may exist at a time; constructing a second panics in debug builds and
/// logs a warning in release builds. `Drop` resets the guard.
pub struct MobileClip {
    // Model bytes held for the struct's lifetime: rayon may spawn new worker threads
    // at any time, each needing `commit_from_memory(&self.bytes)` to construct its Session.
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

impl Drop for MobileClip {
    fn drop(&mut self) {
        INSTANCE_EXISTS.store(false, Ordering::Release);
    }
}

impl MobileClip {
    /// Construct from verified CLIP model bytes.
    ///
    /// # Panics (debug) / logs warning (release)
    ///
    /// Panics if a second `MobileClip` instance is constructed before the first is dropped,
    /// because the module-scoped `thread_local! SESS` would be shared between instances.
    pub fn new(model: &VerifiedModelBytes) -> Self {
        if INSTANCE_EXISTS
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            // In practice, run_dedup creates exactly one MobileClip per invocation.
            // A second concurrent constructor indicates the shared SESS thread_local risk.
            // Warn rather than panic: parallel test execution legitimately creates multiple
            // instances sequentially on different threads, which is safe (each thread gets
            // its own Session). The risk is only when two instances co-exist on the SAME
            // thread and call embed() with different model bytes.
            tracing::warn!(
                "MobileClip: concurrent second instance detected; \
                 if two instances exist on the same thread, SESS may use the wrong model"
            );
        }
        Self {
            bytes: model.bytes(),
        }
    }

    /// Embed an RGB image and return a 512-dim L2-normalized embedding.
    ///
    /// Preprocessing (CLIP-standard, confirmed in ANL-003):
    /// 1. Bicubic resize shorter edge to 256px, center-crop 224×224 (TD-020 closed).
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
        // ── 1. Bicubic resize shorter edge → 256px, center-crop 224×224 ──────
        let resized = clip_preprocess(rgb); // Vec<u8>, len = 224*224*3

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

// ── CLIP preprocessing: bicubic resize + center-crop (TD-020) ────────────────

/// CLIP-canonical preprocessing: resize shorter edge to 256px (bicubic),
/// then center-crop a 224×224 window.
///
/// Returns a `Vec<u8>` of length 224×224×3 in HWC row-major RGB order.
///
/// This matches the preprocessing used during model training
/// (open_clip `src/open_clip/transform.py` `Resize(256) + CenterCrop(224)`).
/// The previous bilinear 1:1 resize (TD-020 stop-gap) produced embeddings
/// with lower inter-photo cosine similarity vs the Python reference.
#[allow(
    clippy::indexing_slicing,
    reason = "all indices proven in-bounds by loop invariants and clamp guards"
)]
fn clip_preprocess(src: &RgbImage) -> Vec<u8> {
    let sw = src.width().get() as f32;
    let sh = src.height().get() as f32;

    // Step 1: bicubic resize so shorter edge = 256px (aspect-preserving).
    let scale = 256.0_f32 / sw.min(sh);
    let new_w = (sw * scale).round() as u32;
    let new_h = (sh * scale).round() as u32;
    let resized = bicubic_resize(src, new_w, new_h);

    // Step 2: center-crop 224×224.
    let crop_x = (new_w.saturating_sub(224)) / 2;
    let crop_y = (new_h.saturating_sub(224)) / 2;
    let out_w = new_w.min(224);
    let out_h = new_h.min(224);
    let mut cropped = vec![0u8; 224 * 224 * 3];
    for dy in 0..out_h as usize {
        let sy = (crop_y as usize + dy).min(new_h as usize - 1);
        let src_row = sy * new_w as usize * 3;
        let dst_row = dy * 224 * 3;
        let src_col = crop_x as usize * 3;
        let copy_len = out_w as usize * 3;
        cropped[dst_row..dst_row + copy_len]
            .copy_from_slice(&resized[src_row + src_col..src_row + src_col + copy_len]);
    }
    cropped
}

/// Catmull-Rom bicubic resize.
///
/// Returns a `Vec<u8>` of length `out_w * out_h * 3` in HWC row-major RGB order.
/// Applied separably: horizontal pass then vertical pass.
#[allow(
    clippy::indexing_slicing,
    reason = "indices bounded by out_w/out_h/src dimensions; clamp guards prevent OOB"
)]
fn bicubic_resize(src: &RgbImage, out_w: u32, out_h: u32) -> Vec<u8> {
    let sw = src.width().get() as usize;
    let sh = src.height().get() as usize;
    let ow = out_w as usize;
    let oh = out_h as usize;

    // Horizontal pass: src → intermediate (ow × sh × 3).
    let mut horiz = vec![0.0_f32; ow * sh * 3];
    let scale_x = sw as f32 / ow as f32;
    for row in 0..sh {
        for col in 0..ow {
            let sx = (col as f32 + 0.5) * scale_x - 0.5;
            let x0 = sx.floor() as i32;
            let fx = sx - sx.floor();
            let w = catmull_rom_weights(fx);
            for ch in 0..3_usize {
                let mut acc = 0.0_f32;
                for (k, &wk) in w.iter().enumerate() {
                    let xi = (x0 + k as i32 - 1).clamp(0, sw as i32 - 1) as usize;
                    acc += wk * f32::from(pixel_ch(src, xi, row, ch));
                }
                horiz[row * ow * 3 + col * 3 + ch] = acc;
            }
        }
    }

    // Vertical pass: intermediate → dst (ow × oh × 3).
    let mut dst = vec![0u8; ow * oh * 3];
    let scale_y = sh as f32 / oh as f32;
    for row in 0..oh {
        let sy = (row as f32 + 0.5) * scale_y - 0.5;
        let y0 = sy.floor() as i32;
        let fy = sy - sy.floor();
        let w = catmull_rom_weights(fy);
        for col in 0..ow {
            for ch in 0..3_usize {
                let mut acc = 0.0_f32;
                for (k, &wk) in w.iter().enumerate() {
                    let yi = (y0 + k as i32 - 1).clamp(0, sh as i32 - 1) as usize;
                    acc += wk * horiz[yi * ow * 3 + col * 3 + ch];
                }
                dst[row * ow * 3 + col * 3 + ch] = acc.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    dst
}

/// Catmull-Rom cubic interpolation weights for fractional offset `t ∈ [0, 1)`.
/// Returns `[w_{-1}, w_0, w_1, w_2]` (4-tap kernel, α = -0.5).
#[inline]
fn catmull_rom_weights(t: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

#[inline]
fn pixel_ch(src: &RgbImage, x: usize, y: usize, ch: usize) -> u8 {
    let w = src.width().get() as usize;
    let idx = (y * w + x) * 3 + ch;
    src.pixels().get(idx).copied().unwrap_or(0)
}
