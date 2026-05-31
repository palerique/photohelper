//! `RawImage` + Bayer-decode companion types.
//!
//! Every type in this module is a strong newtype with private fields,
//! a fallible constructor that enforces the R2-T6 / R3-T5 invariants
//! locked at plan-review, and an accessor surface that never panics
//! on out-of-range inputs (per R2-T5: `BayerPlane::row(y)` returns
//! `Option<&[u16]>`, not `&[u16]`).
//!
//! `RawImage` itself ships as type + accessors in this commit; its
//! infallible constructor + `read_raw` entry point land with the FFI
//! body (Deliverable 1a). Splitting now would force `#[allow(dead_code)]`
//! placeholders on the constructor used in the very next commit.
//!
//! ## R2-T6 invariants enforced at construction
//!
//! * `BayerPlane::new` — buffer length must equal `width * height`.
//! * `SensorBitDepth::new` — value must lie in `8..=16`.
//! * `SensorLevels::new` — `black < white`; dynamic range `>= 256`;
//!   `white` must fit in the declared bit depth.
//! * `WhiteBalance::from_libraw_cam_mul` — reject all-zero
//!   (LibRaw signals "unloaded"); reject NaN / infinite / negative.
//! * `CamRgbToXyzD65Matrix::from_libraw_rgb_cam` — reject identity
//!   (LibRaw signals "unloaded"); reject NaN / infinite.
//!
//! ## TD-007 closure
//!
//! Every constructor takes `path: &Path` as the first argument so the
//! `Error` variant's `path` field carries the real fixture path,
//! not the `PathBuf::new()` stop-gap. The FFI body (1a) passes its
//! `RawPath` newtype's underlying `&Path`.

#![forbid(unsafe_code)]

use std::num::NonZeroU32;
use std::path::Path;

use static_assertions::assert_impl_all;

use photohelper_core::model::RgbImage;

use crate::ffi::{self, RawPath};
use crate::{Error, RawDecodeCause};

/// A decoded Canon CR3 (or other Bayer-pattern RAW) ready for the
/// develop pipeline. Holds the pixel plane, CFA mosaic discriminant,
/// black/white sensor levels, as-shot white balance, and the
/// camera-to-XYZ color matrix.
///
/// **Not `Clone`** by design — the underlying `BayerPlane` holds a
/// `Box<[u16]>` whose size at Canon R8 resolution is ~50 MB. Cloning
/// silently would mask back-pressure issues in the develop pipeline;
/// when a consumer genuinely needs two copies it should construct two
/// `RawImage`s by re-decoding (or, post-v0.1, by `Arc`-sharing the
/// plane explicitly).
///
/// `Send + Sync` asserted at module scope.
#[derive(Debug)]
pub struct RawImage {
    pixels: BayerPlane,
    cfa_pattern: CfaPattern,
    levels: SensorLevels,
    as_shot_white_balance: WhiteBalance,
    color_matrix: CamRgbToXyzD65Matrix,
}

assert_impl_all!(RawImage: Send, Sync);

impl RawImage {
    /// The pixel plane (Bayer-pattern `u16` sensor samples).
    pub fn pixels(&self) -> &BayerPlane {
        &self.pixels
    }

    /// The CFA mosaic pattern. Discriminator for the demosaic algorithm.
    pub fn cfa_pattern(&self) -> CfaPattern {
        self.cfa_pattern
    }

    /// Black and white sensor levels (with bit depth).
    pub fn levels(&self) -> SensorLevels {
        self.levels
    }

    /// As-shot white balance multipliers (R / G1 / B / G2 per Canon's
    /// `cam_mul` layout).
    pub fn as_shot_white_balance(&self) -> WhiteBalance {
        self.as_shot_white_balance
    }

    /// Camera-RGB → XYZ color matrix at the D65 illuminant.
    pub fn color_matrix(&self) -> CamRgbToXyzD65Matrix {
        self.color_matrix
    }

    /// Infallible constructor — every component already enforced its
    /// invariants. Called by `ffi::parse_libraw_image` and never
    /// directly by external code (no public path to construct
    /// validated components from outside this crate).
    pub(crate) fn new(
        pixels: BayerPlane,
        cfa_pattern: CfaPattern,
        levels: SensorLevels,
        as_shot_white_balance: WhiteBalance,
        color_matrix: CamRgbToXyzD65Matrix,
    ) -> Self {
        Self {
            pixels,
            cfa_pattern,
            levels,
            as_shot_white_balance,
            color_matrix,
        }
    }
}

/// Decode the Bayer-pattern sensor data of a CR3 (or other
/// LibRaw-supported RAW) file into a typed [`RawImage`]. Public entry
/// point for the develop pipeline (session 04+).
///
/// Allocates ~50 MB of `Box<[u16]>` per call at Canon R8 resolution
/// (24Mpix × 2 bytes); LibRaw's internal demosaic-prep buffer adds
/// another ~96-200 MB transient. Per-decode peak ~150-250 MB per
/// worker per plan §1c memory pressure SLO.
///
/// # Errors
///
/// See [`crate::Error`] and [`crate::RawDecodeCause`] for the
/// exhaustive failure-class breakdown.
pub fn read_raw(path: &Path) -> Result<RawImage, Error> {
    let raw_path = RawPath::new(path)?;
    ffi::parse_libraw_image(&raw_path)
}

/// Options for processing the image with `libraw_dcraw_process`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessOptions {
    /// Standard 8-bit sRGB demosaic (used by AI pipeline).
    Srgb8,
    /// 16-bit linear demosaic (used by Export pipeline).
    Linear16,
}

/// A decoded, processed image buffer with verified spatial bounds.
#[derive(Debug)]
pub struct ImageBuffer<T> {
    /// The raw pixel data.
    pub data: Vec<T>,
    /// The width of the image.
    pub width: NonZeroU32,
    /// The height of the image.
    pub height: NonZeroU32,
    /// Number of color channels (e.g. 3 for RGB).
    pub channels: u8,
}

assert_impl_all!(ImageBuffer<u8>: Send, Sync);
assert_impl_all!(ImageBuffer<u16>: Send, Sync);

/// Represents the possible outputs of `decode_image`.
#[derive(Debug)]
pub enum ProcessedImage {
    /// 8-bit per channel output (usually sRGB).
    Rgb8(ImageBuffer<u8>),
    /// 16-bit per channel output (usually linear).
    Linear16(ImageBuffer<u16>),
}

/// Decode a Canon CR3 (or other LibRaw-supported RAW) file via the
/// AHD demosaic pipeline and return a processed image based on `options`.
///
/// Calls `libraw_dcraw_process` then `libraw_dcraw_make_mem_image`. The
/// resulting pixel buffer is copied into Rust-owned memory before the LibRaw
/// handle is released; no raw pointers survive the call.
///
/// # Errors
///
/// Returns [`crate::Error::RawDecodeFailed`] if LibRaw produces
/// unexpected output or memory allocation fails. Other [`crate::Error`]
/// variants cover path-validation and LibRaw API failures.
pub fn decode_image(path: &Path, options: ProcessOptions) -> Result<ProcessedImage, Error> {
    let raw_path = RawPath::new(path)?;
    ffi::parse_processed_image(&raw_path, options)
}

/// Compatibility wrapper for the AI culling pipeline (session 04 §D1e).
/// It calls `decode_image` with `ProcessOptions::Srgb8` and maps the
/// `ImageBuffer<u8>` into an `RgbImage`.
pub fn read_raw_rgb(path: &Path) -> Result<RgbImage, Error> {
    match decode_image(path, ProcessOptions::Srgb8)? {
        ProcessedImage::Rgb8(buf) => {
            RgbImage::new(buf.data, buf.width, buf.height).map_err(|_| Error::RawDecodeFailed {
                path: path.to_path_buf(),
                cause: RawDecodeCause::LibRawCallFailed {
                    libraw_code: 0,
                    op: "pixel buffer length != width*height*3",
                },
            })
        }
        ProcessedImage::Linear16(_) => Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::RgbConversionFailed {
                bits: 16,
                colors: 3,
            },
        }),
    }
}

/// The raw Bayer-pattern sensor buffer plus its dimensions, with the
/// `data.len() == width * height` invariant enforced by the constructor.
///
/// All accessors are fallible — `row(y)` and `pixel(x, y)` return
/// `Option` rather than panicking on out-of-bounds. The iterator API
/// `rows()` is the preferred consumer for session 04's demosaic.
#[derive(Debug)]
pub struct BayerPlane {
    data: Box<[u16]>,
    width: NonZeroU32,
    height: NonZeroU32,
}

assert_impl_all!(BayerPlane: Send, Sync);

impl BayerPlane {
    /// Construct a `BayerPlane` from a raw pixel buffer.
    ///
    /// `path` is recorded into the `Error` variant on dimension mismatch
    /// so operator-facing log lines name the offending fixture (TD-007
    /// closure).
    ///
    /// # Errors
    ///
    /// Returns [`Error::RawImageDimensionMismatch`] if
    /// `data.len() != width * height`.
    pub(crate) fn new(
        path: &Path,
        data: Vec<u16>,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<Self, Error> {
        let expected = u64::from(width.get()) * u64::from(height.get());
        let actual = data.len() as u64;
        if actual != expected {
            return Err(Error::RawImageDimensionMismatch {
                path: path.to_path_buf(),
                declared_pixels: expected,
                actual_pixels: actual,
            });
        }
        Ok(Self {
            data: data.into_boxed_slice(),
            width,
            height,
        })
    }

    /// Width in pixels (non-zero by type).
    pub fn width(&self) -> NonZeroU32 {
        self.width
    }

    /// Height in pixels (non-zero by type).
    pub fn height(&self) -> NonZeroU32 {
        self.height
    }

    /// Pixel row `y` as a slice, or `None` if `y >= height`.
    pub fn row(&self, y: u32) -> Option<&[u16]> {
        if y >= self.height.get() {
            return None;
        }
        let w = self.width.get() as usize;
        let start = (y as usize).checked_mul(w)?;
        let end = start.checked_add(w)?;
        self.data.get(start..end)
    }

    /// Single pixel at `(x, y)`, or `None` if either coordinate is out
    /// of range.
    pub fn pixel(&self, x: u32, y: u32) -> Option<u16> {
        let row = self.row(y)?;
        row.get(x as usize).copied()
    }

    /// Iterator over each pixel row. Preferred over `row(y)` indexing
    /// for sequential demosaic consumers — avoids one bounds check per
    /// row.
    pub fn rows(&self) -> impl Iterator<Item = &[u16]> {
        let w = self.width.get() as usize;
        self.data.chunks_exact(w)
    }
}

/// CFA mosaic pattern discriminator. Only the four 2x2 Bayer variants
/// are modeled in v0.1; X-Trans, Foveon, and monochrome sensors are
/// deferred per DN-014.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CfaPattern {
    /// Red, Green, Green, Blue.
    Rggb,
    /// Blue, Green, Green, Red.
    Bggr,
    /// Green, Red, Blue, Green.
    Grbg,
    /// Green, Blue, Red, Green.
    Gbrg,
}

/// Sensor bit depth, constrained to `8..=16` via the constructor.
///
/// LibRaw returns the camera-reported bit depth via
/// `libraw_get_color_maximum()` (effectively). Out-of-range values
/// suggest a corrupt CR3 EXIF or a body whose firmware reports something
/// photohelper doesn't yet model — both produce
/// [`Error::RawInvalidBitDepth`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensorBitDepth(u8);

assert_impl_all!(SensorBitDepth: Send, Sync);

impl SensorBitDepth {
    /// Construct a `SensorBitDepth` from a raw bit count.
    ///
    /// # Errors
    ///
    /// Returns [`Error::RawInvalidBitDepth`] if `bits` is outside
    /// `8..=16`.
    pub(crate) fn new(path: &std::path::Path, bits: u8) -> Result<Self, Error> {
        if !(8..=16).contains(&bits) {
            return Err(Error::RawInvalidBitDepth {
                path: path.to_path_buf(),
                value: bits,
            });
        }
        Ok(Self(bits))
    }

    /// The wrapped bit-depth value (always in `8..=16`).
    pub fn get(&self) -> u8 {
        self.0
    }
}

/// Black and white sensor levels with the declared bit depth.
///
/// `SensorLevels::new` enforces three invariants jointly:
/// `black < white`, dynamic range `(white - black) >= 256` (any sensor
/// reporting less is reporting nonsense), and `white` must fit within
/// the bit depth's representable range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensorLevels {
    black: u16,
    white: u16,
    bit_depth: SensorBitDepth,
}

assert_impl_all!(SensorLevels: Send, Sync);

impl SensorLevels {
    /// Construct a `SensorLevels` from LibRaw-reported values.
    ///
    /// `path` is recorded into the `Error` variant on any invariant
    /// violation so operator-facing log lines name the offending
    /// fixture (TD-007 closure).
    ///
    /// # Errors
    ///
    /// Returns [`Error::RawInvalidLevels`] when any of the three
    /// invariants is violated: inverted levels (`black >= white`), too
    /// narrow a dynamic range, or `white` exceeds what `bit_depth` can
    /// represent.
    pub(crate) fn new(
        path: &Path,
        black: u16,
        white: u16,
        bit_depth: SensorBitDepth,
    ) -> Result<Self, Error> {
        if black >= white {
            return Err(Error::RawInvalidLevels {
                path: path.to_path_buf(),
                black,
                white,
            });
        }
        // Dynamic-range floor: at least 256 distinguishable steps; less
        // means LibRaw fed us a bogus pair (black=0, white=1, etc.).
        if (white - black) < 256 {
            return Err(Error::RawInvalidLevels {
                path: path.to_path_buf(),
                black,
                white,
            });
        }
        // Bit-depth bound: white must fit in the declared bit depth.
        // `1u32 << bit_depth.get()` is well-defined because the
        // constructor capped `bit_depth.get()` at 16 (no overflow).
        let max_for_depth = (1u32 << bit_depth.get()) - 1;
        if u32::from(white) > max_for_depth {
            return Err(Error::RawInvalidLevels {
                path: path.to_path_buf(),
                black,
                white,
            });
        }
        Ok(Self {
            black,
            white,
            bit_depth,
        })
    }

    /// Black-level raw sample value (sensor zero).
    pub fn black(&self) -> u16 {
        self.black
    }

    /// White-level raw sample value (sensor saturation).
    pub fn white(&self) -> u16 {
        self.white
    }

    /// Declared bit depth (`8..=16`).
    pub fn bit_depth(&self) -> SensorBitDepth {
        self.bit_depth
    }
}

/// As-shot white balance multipliers in LibRaw's `cam_mul` ordering.
///
/// **Canon ordering note**: LibRaw documents `cam_mul` as `R / G1 / B / G2`
/// on Canon bodies, NOT `R / G / G / B` (RGGB). G1 and G2 are the two
/// Bayer greens; treating them as a single value loses one stop of
/// precision for sensors that distinguish the diagonal greens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhiteBalance {
    r: f32,
    g1: f32,
    b: f32,
    g2: f32,
}

assert_impl_all!(WhiteBalance: Send, Sync);

impl WhiteBalance {
    /// Construct a `WhiteBalance` from LibRaw's `cam_mul[4]` buffer.
    ///
    /// `path` is recorded into the `Error` variant so operator-facing
    /// log lines name the offending fixture (TD-007 closure).
    ///
    /// # Errors
    ///
    /// Returns [`Error::RawDecodeFailed`] with cause
    /// [`RawDecodeCause::WhiteBalanceUnloaded`] when `cam_mul` is all-zero
    /// (LibRaw's "unloaded" signal), or with cause
    /// [`RawDecodeCause::WhiteBalanceInvalid`] when any element is NaN,
    /// infinite, negative, or when R/G1/B channels are zero (G2 may be zero
    /// for 3-channel Canon sensors that don't report a second green multiplier).
    pub(crate) fn from_libraw_cam_mul(path: &Path, cam_mul: [f32; 4]) -> Result<Self, Error> {
        let [r, g1, b, g2] = cam_mul;
        if cam_mul.iter().all(|x| *x == 0.0) {
            return Err(Error::RawDecodeFailed {
                path: path.to_path_buf(),
                cause: RawDecodeCause::WhiteBalanceUnloaded,
            });
        }
        // Reject NaN/Inf/negative in any channel, and zero in R/G1/B specifically.
        // G2 (index 3) may legitimately be 0.0 on 3-channel RGGB sensors.
        if cam_mul.iter().any(|x| !x.is_finite() || *x < 0.0) || r == 0.0 || g1 == 0.0 || b == 0.0 {
            return Err(Error::RawDecodeFailed {
                path: path.to_path_buf(),
                cause: RawDecodeCause::WhiteBalanceInvalid { values: cam_mul },
            });
        }
        Ok(Self { r, g1, b, g2 })
    }

    /// Red multiplier.
    pub fn r(&self) -> f32 {
        self.r
    }

    /// First green (G1) multiplier.
    pub fn g1(&self) -> f32 {
        self.g1
    }

    /// Blue multiplier.
    pub fn b(&self) -> f32 {
        self.b
    }

    /// Second green (G2) multiplier.
    pub fn g2(&self) -> f32 {
        self.g2
    }
}

/// Camera-RGB → XYZ color matrix at the D65 illuminant.
///
/// Direction is encoded in the type name (per R2-T6): "from Cam RGB,
/// to XYZ, at D65 illuminant." v0.1 ships as-shot only;
/// per-illuminant matrices (D55, A, etc.) deferred per DN-017.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CamRgbToXyzD65Matrix {
    m: [[f32; 3]; 3],
}

assert_impl_all!(CamRgbToXyzD65Matrix: Send, Sync);

impl CamRgbToXyzD65Matrix {
    /// Construct from LibRaw's `rgb_cam[3][3]` matrix.
    ///
    /// `path` is recorded into the `Error` variant so operator-facing
    /// log lines name the offending fixture (TD-007 closure).
    ///
    /// # Errors
    ///
    /// Returns [`Error::RawDecodeFailed`] with cause
    /// [`RawDecodeCause::ColorMatrixUnloaded`] when LibRaw returned the
    /// identity matrix (its "unloaded" signal — without a real matrix,
    /// color management downstream is undefined), or with cause
    /// [`RawDecodeCause::ColorMatrixInvalid`] when any entry is NaN,
    /// infinite, or when any row is all-zero (all-zero matrix produces a
    /// completely black channel after color management).
    pub(crate) fn from_libraw_rgb_cam(path: &Path, rgb_cam: [[f32; 3]; 3]) -> Result<Self, Error> {
        let is_identity = rgb_cam.iter().enumerate().all(|(i, row)| {
            row.iter().enumerate().all(|(j, &val)| {
                let expected = if i == j { 1.0 } else { 0.0 };
                (val - expected).abs() < 1e-6
            })
        });
        if is_identity {
            return Err(Error::RawDecodeFailed {
                path: path.to_path_buf(),
                cause: RawDecodeCause::ColorMatrixUnloaded,
            });
        }
        if rgb_cam.iter().flatten().any(|x| !x.is_finite()) {
            return Err(Error::RawDecodeFailed {
                path: path.to_path_buf(),
                cause: RawDecodeCause::ColorMatrixInvalid,
            });
        }
        // Reject all-zero rows: a row of all zeros produces a black output channel.
        if rgb_cam
            .iter()
            .any(|row| row.iter().all(|&v| v.abs() < 1e-6))
        {
            return Err(Error::RawDecodeFailed {
                path: path.to_path_buf(),
                cause: RawDecodeCause::ColorMatrixInvalid,
            });
        }
        Ok(Self { m: rgb_cam })
    }

    /// The underlying 3x3 matrix as `[[f32; 3]; 3]`.
    pub fn as_array(&self) -> &[[f32; 3]; 3] {
        &self.m
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )]

    use std::path::PathBuf;

    use super::*;
    use crate::Error;

    fn p() -> PathBuf {
        PathBuf::from("/tmp/test.cr3")
    }

    fn nz(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("non-zero")
    }

    // --- BayerPlane ---

    #[test]
    fn bayer_plane_new_accepts_matching_dimensions() {
        let plane = BayerPlane::new(&p(), vec![0u16; 6 * 4], nz(6), nz(4)).expect("matching");
        assert_eq!(plane.width().get(), 6);
        assert_eq!(plane.height().get(), 4);
    }

    #[test]
    fn bayer_plane_new_rejects_dimension_mismatch() {
        let err = BayerPlane::new(&p(), vec![0u16; 23], nz(6), nz(4)).unwrap_err();
        match err {
            Error::RawImageDimensionMismatch {
                path,
                declared_pixels,
                actual_pixels,
            } => {
                assert_eq!(path, p());
                assert_eq!(declared_pixels, 24);
                assert_eq!(actual_pixels, 23);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn bayer_plane_row_returns_none_past_height() {
        let plane = BayerPlane::new(&p(), vec![0u16; 6 * 4], nz(6), nz(4)).unwrap();
        assert!(plane.row(4).is_none(), "y == height must be out of bounds");
        assert!(plane.row(99).is_none(), "y >> height must be out of bounds");
    }

    #[test]
    fn bayer_plane_pixel_returns_none_past_width() {
        let plane = BayerPlane::new(&p(), vec![0u16; 6 * 4], nz(6), nz(4)).unwrap();
        assert!(
            plane.pixel(6, 0).is_none(),
            "x == width must be out of bounds"
        );
        assert!(
            plane.pixel(0, 4).is_none(),
            "y == height must be out of bounds"
        );
    }

    #[test]
    fn bayer_plane_row_and_pixel_return_data() {
        let data: Vec<u16> = (0..24).collect();
        let plane = BayerPlane::new(&p(), data, nz(6), nz(4)).unwrap();
        assert_eq!(plane.row(0).unwrap(), &[0, 1, 2, 3, 4, 5]);
        assert_eq!(plane.row(3).unwrap(), &[18, 19, 20, 21, 22, 23]);
        assert_eq!(plane.pixel(2, 1), Some(8));
    }

    #[test]
    fn bayer_plane_rows_iterates_in_order() {
        let data: Vec<u16> = (0..12).collect();
        let plane = BayerPlane::new(&p(), data, nz(4), nz(3)).unwrap();
        let collected: Vec<Vec<u16>> = plane.rows().map(<[u16]>::to_vec).collect();
        assert_eq!(
            collected,
            vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![8, 9, 10, 11]]
        );
    }

    // --- SensorBitDepth ---

    #[test]
    fn sensor_bit_depth_accepts_canonical_values() {
        for v in [8u8, 10, 12, 14, 16] {
            assert!(
                SensorBitDepth::new(&p(), v).is_ok(),
                "bit depth {v} must be valid"
            );
        }
    }

    #[test]
    fn sensor_bit_depth_rejects_below_8() {
        for v in [0u8, 1, 7] {
            let err = SensorBitDepth::new(&p(), v).unwrap_err();
            match err {
                Error::RawInvalidBitDepth { path: _, value } => assert_eq!(value, v),
                other => panic!("unexpected variant for {v}: {other:?}"),
            }
        }
    }

    #[test]
    fn sensor_bit_depth_rejects_above_16() {
        for v in [17u8, 32, u8::MAX] {
            let err = SensorBitDepth::new(&p(), v).unwrap_err();
            match err {
                Error::RawInvalidBitDepth { path: _, value } => assert_eq!(value, v),
                other => panic!("unexpected variant for {v}: {other:?}"),
            }
        }
    }

    // --- SensorLevels ---

    #[test]
    fn sensor_levels_accepts_valid_pair() {
        let depth = SensorBitDepth::new(&p(), 14).unwrap();
        SensorLevels::new(&p(), 1024, 16383, depth).expect("valid R8-like levels");
    }

    #[test]
    fn sensor_levels_rejects_inverted_pair() {
        let depth = SensorBitDepth::new(&p(), 14).unwrap();
        let err = SensorLevels::new(&p(), 5000, 5000, depth).unwrap_err();
        match err {
            Error::RawInvalidLevels { path, black, white } => {
                assert_eq!(path, p());
                assert_eq!(black, 5000);
                assert_eq!(white, 5000);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn sensor_levels_rejects_too_narrow_range() {
        let depth = SensorBitDepth::new(&p(), 14).unwrap();
        let err = SensorLevels::new(&p(), 1000, 1100, depth).unwrap_err();
        assert!(matches!(err, Error::RawInvalidLevels { .. }));
    }

    #[test]
    fn sensor_levels_rejects_white_exceeding_bit_depth() {
        // 8-bit max representable = 255; white = 1000 fails.
        let depth = SensorBitDepth::new(&p(), 8).unwrap();
        let err = SensorLevels::new(&p(), 0, 1000, depth).unwrap_err();
        assert!(matches!(err, Error::RawInvalidLevels { .. }));
    }

    // --- WhiteBalance ---

    #[test]
    fn white_balance_accepts_typical_canon_cam_mul() {
        // Approximate as-shot WB for Canon R8 daylight (5500K).
        let wb =
            WhiteBalance::from_libraw_cam_mul(&p(), [2.1, 1.0, 1.4, 1.0]).expect("valid cam_mul");
        assert_eq!(wb.r(), 2.1);
        assert_eq!(wb.g1(), 1.0);
        assert_eq!(wb.b(), 1.4);
        assert_eq!(wb.g2(), 1.0);
    }

    #[test]
    fn white_balance_rejects_all_zero_as_unloaded() {
        let err = WhiteBalance::from_libraw_cam_mul(&p(), [0.0, 0.0, 0.0, 0.0]).unwrap_err();
        match err {
            Error::RawDecodeFailed {
                cause: RawDecodeCause::WhiteBalanceUnloaded,
                path,
            } => assert_eq!(path, p()),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn white_balance_rejects_nan() {
        let err = WhiteBalance::from_libraw_cam_mul(&p(), [f32::NAN, 1.0, 1.0, 1.0]).unwrap_err();
        match err {
            Error::RawDecodeFailed {
                cause: RawDecodeCause::WhiteBalanceInvalid { values },
                ..
            } => {
                assert!(values[0].is_nan());
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn white_balance_rejects_negative() {
        let err = WhiteBalance::from_libraw_cam_mul(&p(), [-1.0, 1.0, 1.0, 1.0]).unwrap_err();
        assert!(matches!(
            err,
            Error::RawDecodeFailed {
                cause: RawDecodeCause::WhiteBalanceInvalid { .. },
                ..
            }
        ));
    }

    #[test]
    fn white_balance_rejects_infinite() {
        let err =
            WhiteBalance::from_libraw_cam_mul(&p(), [f32::INFINITY, 1.0, 1.0, 1.0]).unwrap_err();
        assert!(matches!(
            err,
            Error::RawDecodeFailed {
                cause: RawDecodeCause::WhiteBalanceInvalid { .. },
                ..
            }
        ));
    }

    // --- CamRgbToXyzD65Matrix ---

    #[test]
    fn color_matrix_accepts_typical_canon_rgb_cam() {
        // Approximate Canon-to-XYZ matrix (the canonical R8 matrix is
        // calibrated; values here are illustrative).
        let m = [
            [0.4124, 0.3576, 0.1805],
            [0.2126, 0.7152, 0.0722],
            [0.0193, 0.1192, 0.9505],
        ];
        let cm = CamRgbToXyzD65Matrix::from_libraw_rgb_cam(&p(), m).expect("valid matrix");
        assert_eq!(cm.as_array(), &m);
    }

    #[test]
    fn color_matrix_rejects_identity_as_unloaded() {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let err = CamRgbToXyzD65Matrix::from_libraw_rgb_cam(&p(), identity).unwrap_err();
        match err {
            Error::RawDecodeFailed {
                cause: RawDecodeCause::ColorMatrixUnloaded,
                path,
            } => assert_eq!(path, p()),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn color_matrix_rejects_nan_entry() {
        let m = [
            [0.4124, 0.3576, 0.1805],
            [0.2126, f32::NAN, 0.0722],
            [0.0193, 0.1192, 0.9505],
        ];
        let err = CamRgbToXyzD65Matrix::from_libraw_rgb_cam(&p(), m).unwrap_err();
        assert!(matches!(
            err,
            Error::RawDecodeFailed {
                cause: RawDecodeCause::ColorMatrixInvalid,
                ..
            }
        ));
    }

    #[test]
    fn color_matrix_rejects_infinite_entry() {
        let m = [
            [0.4124, 0.3576, 0.1805],
            [0.2126, f32::INFINITY, 0.0722],
            [0.0193, 0.1192, 0.9505],
        ];
        let err = CamRgbToXyzD65Matrix::from_libraw_rgb_cam(&p(), m).unwrap_err();
        assert!(matches!(
            err,
            Error::RawDecodeFailed {
                cause: RawDecodeCause::ColorMatrixInvalid,
                ..
            }
        ));
    }

    #[test]
    fn color_matrix_rejects_all_zero_row() {
        let m = [
            [0.4124, 0.3576, 0.1805],
            [0.0, 0.0, 0.0], // all-zero row produces a black channel
            [0.0193, 0.1192, 0.9505],
        ];
        let err = CamRgbToXyzD65Matrix::from_libraw_rgb_cam(&p(), m).unwrap_err();
        assert!(matches!(
            err,
            Error::RawDecodeFailed {
                cause: RawDecodeCause::ColorMatrixInvalid,
                ..
            }
        ));
    }

    #[test]
    fn white_balance_rejects_zero_red_channel() {
        // R=0 is physically impossible; G2=0 is OK for 3-channel sensors.
        let err = WhiteBalance::from_libraw_cam_mul(&p(), [0.0, 1.0, 1.4, 0.0]).unwrap_err();
        assert!(matches!(
            err,
            Error::RawDecodeFailed {
                cause: RawDecodeCause::WhiteBalanceInvalid { .. },
                ..
            }
        ));
    }

    #[test]
    fn white_balance_accepts_zero_g2_for_3channel_sensor() {
        // G2=0 is normal for Canon bodies that only report 3 multipliers.
        let wb = WhiteBalance::from_libraw_cam_mul(&p(), [2.1, 1.0, 1.4, 0.0])
            .expect("3-channel WB (G2=0) must be valid");
        assert!((wb.g2() - 0.0).abs() < f32::EPSILON);
    }

    // --- CfaPattern ---

    #[test]
    fn cfa_pattern_variants_are_distinct() {
        let all = [
            CfaPattern::Rggb,
            CfaPattern::Bggr,
            CfaPattern::Grbg,
            CfaPattern::Gbrg,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // --- RawImage (accessors via private-field literal) ---

    fn r8_image() -> RawImage {
        let depth = SensorBitDepth::new(&p(), 14).expect("14-bit");
        RawImage {
            pixels: BayerPlane::new(&p(), vec![0u16; 4], nz(2), nz(2)).expect("2x2"),
            cfa_pattern: CfaPattern::Rggb,
            levels: SensorLevels::new(&p(), 1024, 16383, depth).expect("valid levels"),
            as_shot_white_balance: WhiteBalance::from_libraw_cam_mul(&p(), [2.1, 1.0, 1.4, 1.0])
                .expect("valid wb"),
            color_matrix: CamRgbToXyzD65Matrix::from_libraw_rgb_cam(
                &p(),
                [
                    [0.4124, 0.3576, 0.1805],
                    [0.2126, 0.7152, 0.0722],
                    [0.0193, 0.1192, 0.9505],
                ],
            )
            .expect("valid matrix"),
        }
    }

    #[test]
    fn raw_image_accessors_return_constructor_values() {
        let img = r8_image();
        assert_eq!(img.cfa_pattern(), CfaPattern::Rggb);
        assert_eq!(img.levels().black(), 1024);
        assert_eq!(img.levels().white(), 16383);
        assert_eq!(img.levels().bit_depth().get(), 14);
        assert_eq!(img.as_shot_white_balance().r(), 2.1);
        assert_eq!(img.pixels().width().get(), 2);
        assert_eq!(img.pixels().height().get(), 2);
        assert_eq!(img.color_matrix().as_array()[1][1], 0.7152);
    }
}
