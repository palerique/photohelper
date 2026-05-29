//! NIMA aesthetic scorer.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use ort::value::Tensor;
use photohelper_core::RgbImage;

use crate::error::Error;
use crate::model_bytes::VerifiedModelBytes;

// =====================================================================
// NimaScore — newtype f32 with enforced range [1.0, 10.0]
// =====================================================================

/// An aesthetic score in the range `[1.0, 10.0]` as produced by the NIMA model.
///
/// `Ord` is implemented via `f32::total_cmp`, which is sound because NaN
/// and values outside `[1.0, 10.0]` are rejected at construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NimaScore(f32);

impl NimaScore {
    const MIN: f32 = 1.0;
    const MAX: f32 = 10.0;

    /// Construct from a raw `f32` score.
    ///
    /// # Errors
    ///
    /// Returns `Err(Error::ScoreOutOfRange)` if `v` is NaN, infinite, or
    /// outside `[1.0, 10.0]`.
    pub fn from_f32(v: f32) -> Result<Self, Error> {
        if !v.is_finite() || !(Self::MIN..=Self::MAX).contains(&v) {
            return Err(Error::ScoreOutOfRange {
                value: v,
                min: Self::MIN,
                max: Self::MAX,
            });
        }
        Ok(Self(v))
    }

    /// Construct from a `REAL`-column `f64` value stored in SQLite.
    ///
    /// Casts to `f32` and warns (via `tracing::warn!`) if significant
    /// precision was lost (`((v as f32) as f64 - v).abs() > 1e-6`).
    ///
    /// # Errors
    ///
    /// Returns `Err` if the cast result is outside `[1.0, 10.0]`.
    pub fn from_catalog_f64(v: f64) -> Result<Self, Error> {
        let f32_val = v as f32;
        let round_trip = f64::from(f32_val);
        if (round_trip - v).abs() > 1e-6 {
            tracing::warn!(
                original = v,
                round_trip = round_trip,
                "NimaScore: f64→f32 precision loss > 1e-6; storing rounded value"
            );
        }
        Self::from_f32(f32_val)
    }

    /// Return the inner `f32` value.
    #[must_use]
    pub fn as_f32(self) -> f32 {
        self.0
    }

    /// Return the inner value as `f64` for SQLite `REAL` storage.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        f64::from(self.0)
    }
}

impl Eq for NimaScore {}

impl PartialOrd for NimaScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NimaScore {
    fn cmp(&self, other: &Self) -> Ordering {
        // total_cmp is sound here: NaN is rejected at construction.
        self.0.total_cmp(&other.0)
    }
}

impl std::fmt::Display for NimaScore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.4}", self.0)
    }
}

// =====================================================================
// Nima — ONNX Runtime inference via per-worker thread_local Session
// =====================================================================

thread_local! {
    // One ort::Session per rayon worker thread. Session::run is &mut self
    // (verified in ANL-002 §Threading semantics), so sharing one Session
    // across threads would require a Mutex, serialising all inference.
    // thread_local! gives each rayon worker its own Session for full parallelism.
    static SESS: RefCell<Option<ort::session::Session>> = const { RefCell::new(None) };
}

/// NIMA (Neural Image Assessment) aesthetic scorer.
///
/// Wraps [`VerifiedModelBytes`] and constructs a per-worker
/// `ort::Session` lazily on first use via `thread_local!`.
pub struct Nima {
    bytes: Arc<[u8]>,
    /// Canonical path used in `InferenceFailed` error messages.
    model_path: PathBuf,
}

static_assertions::assert_impl_all!(Nima: Send, Sync);

impl Nima {
    /// Construct a `Nima` scorer from verified model bytes.
    pub fn new(model: &VerifiedModelBytes, model_path: PathBuf) -> Self {
        Self {
            bytes: model.bytes(),
            model_path,
        }
    }

    /// Score an RGB image and return its aesthetic score.
    ///
    /// Internally:
    /// 1. Resizes `rgb` to 224×224 via bilinear interpolation.
    /// 2. Applies MobileNet `preprocess_input`: `(pixel / 127.5) - 1.0`.
    /// 3. Runs ort inference (per-worker thread_local Session).
    /// 4. Computes `sum((i+1) * p[i])` over the 10-class softmax output.
    ///
    /// # Errors
    ///
    /// - `ModelLoad` if Session construction fails on the calling thread.
    /// - `InferenceFailed` if ort inference errors or the output is degenerate.
    /// - `ScoreOutOfRange` if the computed mean is outside `[1.0, 10.0]` or NaN.
    pub fn score(&self, rgb: &RgbImage) -> Result<NimaScore, Error> {
        // Resize to 224×224 into a new buffer (bilinear, NOT in-place — &RgbImage
        // is immutable; output size differs from input).
        let resized = bilinear_resize(rgb, 224, 224);

        // Apply MobileNet preprocess_input: pixel / 127.5 - 1.0 → [-1, 1].
        // Output tensor shape: (1, 224, 224, 3) NHWC (TF-origin model).
        let input: Vec<f32> = resized
            .iter()
            .map(|&p| f32::from(p) / 127.5 - 1.0)
            .collect();

        // Run inference via thread-local Session.
        let probs: Vec<f32> = SESS.with(|cell| -> Result<Vec<f32>, Error> {
            let mut guard = cell.borrow_mut();
            // Lazy Session construction — each rayon worker builds its own.
            if guard.is_none() {
                let sess = ort::session::Session::builder()
                    .map_err(|e| Error::ModelLoad {
                        source: Box::new(e),
                    })?
                    .commit_from_memory(&self.bytes)
                    .map_err(|e| Error::ModelLoad {
                        source: Box::new(e),
                    })?;
                *guard = Some(sess);
            }
            // SAFETY(unwrap): guard is Some here — the if-is_none block above
            // either sets *guard = Some(_) or returns Err, so this line is only
            // reached when guard is guaranteed Some.
            #[allow(
                clippy::unwrap_used,
                reason = "guard proven Some: if-is_none branch either inserts Some or returns Err"
            )]
            let sess = guard.as_mut().unwrap();

            // Build input tensor: (1, 224, 224, 3) NHWC f32.
            let input_tensor =
                Tensor::<f32>::from_array(([1_usize, 224, 224, 3], input.into_boxed_slice()))
                    .map_err(|e| Error::InferenceFailed {
                        path: self.model_path.clone(),
                        source: Box::new(e),
                    })?;

            // Get input/output names from the session metadata.
            // Clone to String first so the immutable borrow doesn't outlive the
            // mutable borrow required by sess.run().
            let input_name: String = sess
                .inputs()
                .first()
                .map_or_else(|| "input".to_owned(), |i| i.name().to_owned());
            let output_name: String = sess
                .outputs()
                .first()
                .map_or_else(|| "output".to_owned(), |i| i.name().to_owned());

            // inputs! macro returns Vec<(Cow<str>, SessionInputValue)> directly (not a Result).
            let session_inputs = ort::inputs![input_name.as_str() => input_tensor];

            let outputs = sess
                .run(session_inputs)
                .map_err(|e| Error::InferenceFailed {
                    path: self.model_path.clone(),
                    source: Box::new(e),
                })?;

            let first =
                outputs
                    .get(output_name.as_str())
                    .ok_or_else(|| Error::InferenceFailed {
                        path: self.model_path.clone(),
                        source: format!("output '{output_name}' not found in ort output map")
                            .into(),
                    })?;

            let (_, data) =
                first
                    .try_extract_tensor::<f32>()
                    .map_err(|e| Error::InferenceFailed {
                        path: self.model_path.clone(),
                        source: Box::new(e),
                    })?;

            Ok(data.to_vec())
        })?;

        // Weighted mean: sum((i+1) * p_i) for i in 0..10.
        let mean: f32 = probs
            .iter()
            .enumerate()
            .map(|(i, &p)| (i + 1) as f32 * p)
            .sum();

        NimaScore::from_f32(mean).map_err(|e| Error::InferenceFailed {
            path: self.model_path.clone(),
            source: e.to_string().into(),
        })
    }
}

// =====================================================================
// Bilinear resize helper (~60 LoC; no image crate dep)
// =====================================================================

/// Bilinear downsample an `RgbImage` to `out_w × out_h` pixels.
///
/// Returns a `Vec<u8>` of length `out_w * out_h * 3` in row-major RGB order.
/// `pub(crate)` so `mobileclip.rs` can reuse for CLIP preprocessing (TD-020).
pub(crate) fn bilinear_resize(src: &RgbImage, out_w: u32, out_h: u32) -> Vec<u8> {
    let sw = src.width().get() as f32;
    let sh = src.height().get() as f32;
    let mut dst = vec![0u8; out_w as usize * out_h as usize * 3];

    for dy in 0..out_h {
        let sy = (dy as f32 + 0.5) * sh / out_h as f32 - 0.5;
        let sy0 = (sy.floor() as i32).clamp(0, sh as i32 - 1) as u32;
        let sy1 = (sy0 + 1).min(sh as u32 - 1);
        let wy1 = (sy - sy.floor()).clamp(0.0, 1.0);
        let wy0 = 1.0 - wy1;

        for dx in 0..out_w {
            let sx = (dx as f32 + 0.5) * sw / out_w as f32 - 0.5;
            let sx0 = (sx.floor() as i32).clamp(0, sw as i32 - 1) as u32;
            let sx1 = (sx0 + 1).min(sw as u32 - 1);
            let wx1 = (sx - sx.floor()).clamp(0.0, 1.0);
            let wx0 = 1.0 - wx1;

            // Bilinear interpolation for 3 RGB channels.
            let dst_base = (dy as usize * out_w as usize + dx as usize) * 3;
            let channels: [f32; 3] = [
                bilinear4(
                    pixel_chan(src, sx0, sy0, 0),
                    pixel_chan(src, sx1, sy0, 0),
                    pixel_chan(src, sx0, sy1, 0),
                    pixel_chan(src, sx1, sy1, 0),
                    wx0,
                    wx1,
                    wy0,
                    wy1,
                ),
                bilinear4(
                    pixel_chan(src, sx0, sy0, 1),
                    pixel_chan(src, sx1, sy0, 1),
                    pixel_chan(src, sx0, sy1, 1),
                    pixel_chan(src, sx1, sy1, 1),
                    wx0,
                    wx1,
                    wy0,
                    wy1,
                ),
                bilinear4(
                    pixel_chan(src, sx0, sy0, 2),
                    pixel_chan(src, sx1, sy0, 2),
                    pixel_chan(src, sx0, sy1, 2),
                    pixel_chan(src, sx1, sy1, 2),
                    wx0,
                    wx1,
                    wy0,
                    wy1,
                ),
            ];
            // dst_base..dst_base+3 is always valid: vec is out_w*out_h*3 bytes
            // and dst_base = (row*out_w + col)*3 with row < out_h, col < out_w.
            if let Some(row) = dst.get_mut(dst_base..dst_base + 3) {
                for (r, v) in row.iter_mut().zip(channels) {
                    *r = v.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    dst
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn bilinear4(
    p00: f32,
    p10: f32,
    p01: f32,
    p11: f32,
    wx0: f32,
    wx1: f32,
    wy0: f32,
    wy1: f32,
) -> f32 {
    wx0 * wy0 * p00 + wx1 * wy0 * p10 + wx0 * wy1 * p01 + wx1 * wy1 * p11
}

/// Return one channel value of `src` at `(x, y)` as `f32`.
/// `chan` must be 0, 1, or 2 (enforced by callers iterating over 3 channels).
#[inline]
fn pixel_chan(src: &RgbImage, x: u32, y: u32, chan: usize) -> f32 {
    src.pixel_rgb(x, y)
        .and_then(|p| p.get(chan).copied())
        .map_or(0.0, f32::from)
}
