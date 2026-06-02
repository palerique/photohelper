//! Export pipeline for photohelper: resize, watermark, JPEG encode.
//!
//! # Shared rendering primitives (D1.0)
//!
//! - [`resize_rgb`] — aspect-ratio-preserving resize into a [`tiny_skia::Pixmap`]
//! - [`render_to_jpeg`] — full pipeline: resize → shadow → marks → JPEG bytes
//! - [`pixmap_to_rgb`] — demultiply tiny-skia RGBA → RGB
//! - [`compress_jpeg`] — MozJPEG encoding → `Vec<u8>`
//! - [`load_source_image`] — raster (JPEG/PNG) + RAW → [`photohelper_core::model::RgbImage`]
//! - [`shadow_alpha_ramp`] — monotonic opacity gradient for the shadow band
//! - [`MarkPlacement`] — geometry validator for corner marks

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

mod isp;
pub use isp::ToneMappingLut;

// ===== D1b constants =====

/// Fraction of image height for Mark1 (top-right watermark).
pub const MARK1_HEIGHT_FRAC: f32 = 0.14;

/// Fraction of image height for Mark2 (bottom-left watermark).
pub const MARK2_HEIGHT_FRAC: f32 = 0.13;

/// Fraction of image dimension (width for x-margin, height for y-margin) for mark placement.
pub const MARK_MARGIN_FRAC: f32 = 0.046;

/// Fraction of image height covered by the shadow gradient band.
pub const SHADOW_BAND_FRAC: f32 = 0.30;

// ===== Existing types (unchanged) =====

/// Strong type for Photo Rating (validated/clamped to -1..=5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rating(i32);

impl Rating {
    /// Create a validated rating.
    pub fn new(val: i32) -> Self {
        Self(val.clamp(-1, 5))
    }

    /// Get raw value.
    pub fn value(&self) -> i32 {
        self.0
    }
}

/// Strong type for NIMA aesthetic score (finite, non-NaN).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct NimaScore(f32);

impl NimaScore {
    /// Create a validated NimaScore.
    pub fn new(val: f32) -> Option<Self> {
        if val.is_finite() && !val.is_nan() {
            Some(Self(val))
        } else {
            None
        }
    }

    /// Get raw value.
    pub fn value(&self) -> f32 {
        self.0
    }
}

/// Metadata passed to the export pipeline.
#[derive(Debug, Clone)]
pub struct ExportMetadata {
    /// Photo rating (-1..=5).
    pub rating: Rating,
    /// NIMA aesthetic score if available.
    pub nima_score: Option<NimaScore>,
}

/// Configurable position for text watermarks (export subcommand).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatermarkPosition {
    /// Bottom-left corner.
    BottomLeft,
    /// Top-right corner.
    TopRight,
    /// Top-left corner.
    TopLeft,
    /// Bottom-right corner.
    BottomRight,
    /// Center of image.
    Center,
}

/// Scale value clamped to \[0.001, 100.0\].
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Scale(f32);

impl Scale {
    /// Clamp `val` into the valid scale range.
    pub fn new(val: f32) -> Self {
        Self(val.clamp(0.001, 100.0))
    }

    /// Raw scale value.
    pub fn value(&self) -> f32 {
        self.0
    }
}

/// A PNG badge preloaded into a pixmap.
#[derive(Clone)]
pub struct PreloadedBadge {
    /// The decoded PNG pixmap.
    pub pixmap: Arc<tiny_skia::Pixmap>,
    /// Optional scale as percentage of image long edge.
    pub scale: Option<Scale>,
}

impl PreloadedBadge {
    /// Load a PNG file and decode it into a pixmap.
    pub fn load(path: &Path, scale: Option<Scale>) -> Result<Self, ExportError> {
        let badge_data = std::fs::read(path).map_err(|e| ExportError::BadgeLoadFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        let badge_pixmap = tiny_skia::Pixmap::decode_png(&badge_data).map_err(|e| {
            ExportError::BadgeLoadFailed {
                path: path.to_path_buf(),
                reason: e.to_string(),
            }
        })?;
        Ok(Self {
            pixmap: Arc::new(badge_pixmap),
            scale,
        })
    }
}

impl std::fmt::Debug for PreloadedBadge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreloadedBadge")
            .field("scale", &self.scale)
            .finish()
    }
}

/// Watermark payload for the export subcommand.
#[derive(Debug, Clone)]
pub enum Watermark {
    /// Text watermark.
    Text(String),
    /// Image badge watermark.
    Image(PreloadedBadge),
}

/// Tone-mapping options for the export ISP.
#[derive(Debug, Clone)]
pub struct ToneMappingOptions {
    /// Exposure adjustment in EV stops.
    pub exposure_ev: f32,
}

impl Default for ToneMappingOptions {
    fn default() -> Self {
        Self { exposure_ev: 0.0 }
    }
}

/// Options for the `export` subcommand pipeline.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Output file path (may be a `.tmp` path for atomic rename).
    pub output_path: PathBuf,
    /// JPEG quality (1..=100).
    pub quality: u8,
    /// Optional long-edge resize limit in pixels (≥16).
    pub long_edge: Option<u32>,
    /// Watermarks keyed by position.
    pub watermarks: std::collections::HashMap<WatermarkPosition, Watermark>,
    /// Overwrite existing outputs.
    pub force: bool,
    /// Tone-mapping settings.
    pub tone_mapping: ToneMappingOptions,
}

// ===== D1b — Geometry types =====

/// Which corner slot a mark occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkSlot {
    /// Top-right corner (mark1).
    Mark1,
    /// Bottom-left corner (mark2).
    Mark2,
}

/// How a mark badge's long dimension is determined.
#[derive(Debug, Clone)]
pub enum BadgeSizeBasis {
    /// Badge long edge = image long edge × scale / 100.
    LongEdge(Scale),
    /// Badge height = image height × fraction.
    Height(f32),
}

/// A single image mark to composite via [`render_to_jpeg`].
#[derive(Debug, Clone)]
pub struct MarkSpec {
    /// Decoded PNG badge.
    pub badge: Arc<tiny_skia::Pixmap>,
    /// How to size the badge relative to the image.
    pub basis: BadgeSizeBasis,
    /// Which corner slot to place the badge.
    pub slot: MarkSlot,
    /// Left/right margin as a fraction of the post-resize image width (0.0–1.0).
    pub margin_x: f32,
    /// Top/bottom margin as a fraction of the post-resize image height (0.0–1.0).
    pub margin_y: f32,
}

/// Shadow gradient specification.
#[derive(Debug, Clone, Copy)]
pub struct ShadowSpec {
    /// Fraction of image height covered by the shadow band (0 < band_frac ≤ 1).
    pub band_frac: f32,
}

/// Render options for [`render_to_jpeg`]. Excludes caller concerns (output path, force).
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Optional long-edge resize limit (≥16). `None` = native size.
    pub long_edge: Option<u32>,
    /// If true, images smaller than `long_edge` are emitted at native size (no upscale).
    pub downscale_only: bool,
    /// JPEG quality (1..=100).
    pub quality: u8,
    /// Optional full-bleed bottom shadow gradient.
    pub shadow: Option<ShadowSpec>,
    /// Image marks to composite in order (Mark1 first, Mark2 second).
    pub marks: Vec<MarkSpec>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            long_edge: None,
            downscale_only: false,
            quality: 80,
            shadow: None,
            marks: vec![],
        }
    }
}

// ===== D1b — Geometry validator =====

/// Geometry error returned by [`MarkPlacement::fit`].
#[derive(Debug, Error, Clone)]
pub enum GeometryError {
    /// The mark cannot fit at its required size within the image.
    #[error("mark {which:?} ({mark_dims:?}) does not fit in image ({target_dims:?})")]
    MarkDoesNotFit {
        /// Which slot failed to place.
        which: MarkSlot,
        /// Requested mark dimensions (w, h).
        mark_dims: (u32, u32),
        /// Image dimensions (w, h).
        target_dims: (u32, u32),
    },
}

/// Validated placement of a mark badge within a target image.
///
/// Produced by [`MarkPlacement::fit`]; private fields + accessors prevent
/// construction from outside this module (invariant: mark fits in image).
#[derive(Debug, Clone, Copy)]
pub struct MarkPlacement {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl MarkPlacement {
    /// Compute a validated placement for a mark badge in `slot` within `target`.
    ///
    /// - `mark_h = round(H × height_frac).max(1)`
    /// - `scale  = mark_h / mark_original_h`
    /// - `mark_w = round(mark_original_w × scale).max(1)`
    /// - `margin_x = round(W × margin_frac)`, `margin_y = round(H × margin_frac)`
    /// - Origins use `checked_sub` so underflow maps to [`GeometryError::MarkDoesNotFit`].
    ///
    /// `height_frac` must be in `(0, 1]`; enforced by `debug_assert!` in
    /// non-release builds.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the mark cannot fit at its computed size.
    pub fn fit(
        target: (u32, u32),
        mark_dims: (u32, u32),
        height_frac: f32,
        margin_frac: f32,
        slot: MarkSlot,
    ) -> Result<Self, GeometryError> {
        debug_assert!(
            height_frac.is_finite() && height_frac > 0.0 && height_frac <= 1.0,
            "height_frac must be in (0, 1]"
        );
        let (tw, th) = target;
        let (mw, mh) = mark_dims;

        let mark_h = ((th as f32 * height_frac).round() as u32).max(1);
        let scale = mark_h as f32 / (mh as f32).max(1.0);
        let mark_w = ((mw as f32 * scale).round() as u32).max(1);
        let margin_x = (tw as f32 * margin_frac).round() as u32;
        let margin_y = (th as f32 * margin_frac).round() as u32;

        let (x, y) = match slot {
            MarkSlot::Mark1 => {
                // top-right: x = W - margin_x - mark_w; y = margin_y
                let x = tw
                    .checked_sub(margin_x)
                    .and_then(|v| v.checked_sub(mark_w))
                    .ok_or(GeometryError::MarkDoesNotFit {
                        which: MarkSlot::Mark1,
                        mark_dims: (mark_w, mark_h),
                        target_dims: target,
                    })?;
                (x, margin_y)
            }
            MarkSlot::Mark2 => {
                // bottom-left: x = margin_x; y = H - margin_y - mark_h
                let y = th
                    .checked_sub(margin_y)
                    .and_then(|v| v.checked_sub(mark_h))
                    .ok_or(GeometryError::MarkDoesNotFit {
                        which: MarkSlot::Mark2,
                        mark_dims: (mark_w, mark_h),
                        target_dims: target,
                    })?;
                (margin_x, y)
            }
        };

        // Bounds check: mark must not extend past the image edge.
        if x.checked_add(mark_w).is_none_or(|e| e > tw)
            || y.checked_add(mark_h).is_none_or(|e| e > th)
        {
            return Err(GeometryError::MarkDoesNotFit {
                which: slot,
                mark_dims: (mark_w, mark_h),
                target_dims: target,
            });
        }

        Ok(Self {
            x,
            y,
            w: mark_w,
            h: mark_h,
        })
    }

    /// Top-left x pixel of the placed mark.
    #[must_use]
    pub fn x(&self) -> u32 {
        self.x
    }
    /// Top-left y pixel of the placed mark.
    #[must_use]
    pub fn y(&self) -> u32 {
        self.y
    }
    /// Width of the placed mark in pixels.
    #[must_use]
    pub fn w(&self) -> u32 {
        self.w
    }
    /// Height of the placed mark in pixels.
    #[must_use]
    pub fn h(&self) -> u32 {
        self.h
    }
}

// ===== D1a — Source file kind =====

/// Classification of a source image file for [`load_source_image`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// JPEG or PNG raster.
    Raster,
    /// Canon CR3 RAW (decode-tested).
    Cr3,
    /// Other LibRaw-supported RAW (decode-untested; gated by `--allow-untested-raw`).
    UntestedRaw,
}

impl SourceKind {
    /// Classify `path` by file extension (`eq_ignore_ascii_case`).
    ///
    /// Returns `None` for unrecognised or missing extensions.
    #[must_use]
    pub fn classify(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "png" => Some(Self::Raster),
            "cr3" => Some(Self::Cr3),
            "cr2" | "nef" | "arw" | "raf" | "orf" | "rw2" | "dng" => Some(Self::UntestedRaw),
            _ => None,
        }
    }
}

// ===== Error type =====

/// Errors returned by the export pipeline.
#[derive(Debug, Error)]
pub enum ExportError {
    /// Image dimensions are zero or otherwise invalid.
    #[error("invalid image dimensions")]
    InvalidDimensions,

    /// Memory allocation for a pixmap failed.
    #[error("pixmap allocation failed")]
    AllocationFailed,

    /// I/O error (file read/write/rename).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// LibRaw RAW decode error.
    #[error("RAW decode error: {0}")]
    RawDecode(String),

    /// MozJPEG compression error.
    #[error("JPEG encode error: {0}")]
    JpegEncode(String),

    /// Duplicate watermark position in the export pipeline.
    #[error("duplicate watermark position: {0:?}")]
    DuplicateWatermarkPosition(WatermarkPosition),

    /// Failed to load or decode a PNG badge.
    #[error("failed to load badge at {path}: {reason}")]
    BadgeLoadFailed {
        /// Path to the badge file.
        path: PathBuf,
        /// Human-readable failure reason.
        reason: String,
    },

    /// Watermark omitted (does not fit) — legacy export path.
    #[error("watermark omitted: does not fit on image")]
    WatermarkOmitted,

    /// Source file has an unrecognised extension (`skipped_unsupported`).
    #[error("unsupported source file format: {}", path.display())]
    UnsupportedSource {
        /// Path to the source file.
        path: PathBuf,
    },

    /// Non-CR3 RAW file gated by `--allow-untested-raw` (`skipped_unsupported`).
    #[error(
        "untested RAW format (pass --allow-untested-raw to enable): {}",
        path.display()
    )]
    UntestedRawGated {
        /// Path to the RAW file.
        path: PathBuf,
    },

    /// Raster (JPEG/PNG) decode or orientation error (`decode_failed`).
    #[error("raster decode failed at {}: {reason}", path.display())]
    RasterDecodeFailed {
        /// Path to the raster file.
        path: PathBuf,
        /// Human-readable failure reason.
        reason: String,
    },

    /// Geometry error from [`MarkPlacement::fit`] (`mark_doesnt_fit`).
    #[error("geometry error: {0}")]
    Geometry(#[from] GeometryError),
}

// ===== Font system (private; export subcommand only) =====

thread_local! {
    static FONT_SYSTEM: RefCell<cosmic_text::FontSystem> = {
        let mut db = cosmic_text::fontdb::Database::new();
        let font_bytes = include_bytes!("RobotoMono-Regular.ttf");
        db.load_font_data(font_bytes.to_vec());
        RefCell::new(cosmic_text::FontSystem::new_with_locale_and_db("en-US".to_string(), db))
    };
    static SWASH_CACHE: RefCell<cosmic_text::SwashCache> = RefCell::new(cosmic_text::SwashCache::new());
}

// ===== D1a — Source image loader =====

/// Load a source image (raster or RAW) and return a decoded RGB buffer.
///
/// For raster files (JPEG/PNG), EXIF orientation is applied: an unknown
/// orientation tag logs a warning and defaults to identity (no rotation).
///
/// For RAW files:
/// - CR3 is always accepted.
/// - Other LibRaw formats require `allow_untested_raw = true` and pass through
///   a post-decode sanity guard (dimensions ≥ 16×16; 3 channels).
///
/// # Errors
///
/// - [`ExportError::UnsupportedSource`] — unrecognised extension
///   (`skipped_unsupported`, not a failure).
/// - [`ExportError::UntestedRawGated`] — non-CR3 RAW without the flag
///   (`skipped_unsupported`, not a failure).
/// - [`ExportError::RasterDecodeFailed`] — decode or orientation error
///   (`decode_failed`).
/// - [`ExportError::RawDecode`] — LibRaw decode error (`decode_failed`).
pub fn load_source_image(
    path: &Path,
    allow_untested_raw: bool,
) -> Result<photohelper_core::model::RgbImage, ExportError> {
    match SourceKind::classify(path) {
        None => Err(ExportError::UnsupportedSource {
            path: path.to_path_buf(),
        }),
        Some(SourceKind::UntestedRaw) if !allow_untested_raw => {
            Err(ExportError::UntestedRawGated {
                path: path.to_path_buf(),
            })
        }
        Some(SourceKind::Raster) => decode_raster(path),
        Some(SourceKind::Cr3) | Some(SourceKind::UntestedRaw) => {
            // TD-027: untested-raw decode unverified (colour/demosaic) for non-CR3 formats.
            // Sanity guard checks dims+channels only; colour correctness not tested.
            decode_raw_srgb8(
                path,
                matches!(SourceKind::classify(path), Some(SourceKind::UntestedRaw)),
            )
        }
    }
}

/// Decode a JPEG or PNG file and apply EXIF orientation.
fn decode_raster(path: &Path) -> Result<photohelper_core::model::RgbImage, ExportError> {
    let bytes = std::fs::read(path).map_err(|e| ExportError::RasterDecodeFailed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    let img = image::load_from_memory(&bytes).map_err(|e| ExportError::RasterDecodeFailed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;

    // Apply EXIF orientation for JPEG files.
    let img = {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("jpg") | Some("jpeg")) {
            let orientation = jpeg_exif_orientation(&bytes);
            match orientation {
                Some(o) if o != 1 => apply_exif_orientation(img, o, path),
                None => {
                    // No EXIF tag — identity (no warning; common for synthetic images).
                    img
                }
                _ => img,
            }
        } else {
            img
        }
    };

    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());

    if rgb.len() != rgb.width() as usize * rgb.height() as usize * 3 {
        return Err(ExportError::RasterDecodeFailed {
            path: path.to_path_buf(),
            reason: format!("decoded buffer len {} ≠ {}×{}×3", rgb.len(), w, h),
        });
    }

    let nzw = NonZeroU32::new(w).ok_or_else(|| ExportError::RasterDecodeFailed {
        path: path.to_path_buf(),
        reason: "zero-width image".to_string(),
    })?;
    let nzh = NonZeroU32::new(h).ok_or_else(|| ExportError::RasterDecodeFailed {
        path: path.to_path_buf(),
        reason: "zero-height image".to_string(),
    })?;

    photohelper_core::model::RgbImage::new(rgb.into_raw(), nzw, nzh).map_err(|e| {
        ExportError::RasterDecodeFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        }
    })
}

/// Decode a RAW file via LibRaw Srgb8.
fn decode_raw_srgb8(
    path: &Path,
    sanity_check: bool,
) -> Result<photohelper_core::model::RgbImage, ExportError> {
    let img = photohelper_raw::decode::read_raw_rgb(path)
        .map_err(|e| ExportError::RawDecode(e.to_string()))?;

    if sanity_check {
        let w = img.width().get() as usize;
        let h = img.height().get() as usize;
        if w < 16 || h < 16 || img.pixels().len() != w * h * 3 {
            return Err(ExportError::RawDecode(format!(
                "post-decode sanity guard failed for {}: dims {}×{} or non-3-channel output",
                path.display(),
                w,
                h
            )));
        }
    }

    Ok(img)
}

/// Parse JPEG EXIF orientation from raw file bytes. Returns `None` if absent.
fn jpeg_exif_orientation(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut pos = 2usize;
    while pos + 3 < bytes.len() {
        if bytes[pos] != 0xFF {
            break;
        }
        let marker = bytes[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        // Segment length includes the 2 length bytes but not the FF marker byte.
        let seg_len = usize::from(u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]));
        // APP1 = 0xE1; check for "Exif\0\0" header.
        if marker == 0xE1 {
            let start = pos + 4;
            if start + 6 <= bytes.len() {
                let app1 = &bytes[start..];
                if app1.starts_with(b"Exif\0\0") {
                    return tiff_orientation(&app1[6..]);
                }
            }
        }
        // Stop before SOS (start of scan) — data after is not segment-framed.
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        pos = pos.saturating_add(2 + seg_len);
    }
    None
}

/// Extract the orientation tag value from a TIFF blob.
fn tiff_orientation(data: &[u8]) -> Option<u32> {
    if data.len() < 8 {
        return None;
    }
    let le = data.starts_with(b"II");
    if !le && !data.starts_with(b"MM") {
        return None;
    }
    let u16_at = |off: usize| -> Option<u16> {
        let b = data.get(off..off + 2)?;
        if le {
            Some(u16::from_le_bytes([b[0], b[1]]))
        } else {
            Some(u16::from_be_bytes([b[0], b[1]]))
        }
    };
    let u32_at = |off: usize| -> Option<u32> {
        let b = data.get(off..off + 4)?;
        if le {
            Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        } else {
            Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        }
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd_off = u32_at(4)? as usize;
    let count = u16_at(ifd_off)? as usize;
    for i in 0..count {
        let entry = ifd_off + 2 + i * 12;
        let tag = u16_at(entry)?;
        if tag == 0x0112 {
            // Orientation tag: type SHORT (3), value in bytes 8–9 of entry.
            return Some(u32::from(u16_at(entry + 8)?));
        }
    }
    None
}

/// Apply EXIF orientation to a `DynamicImage`. Unknown tags → warn + identity.
fn apply_exif_orientation(
    img: image::DynamicImage,
    orientation: u32,
    path: &Path,
) -> image::DynamicImage {
    match orientation {
        1 => img,
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => {
            tracing::warn!(
                path = %path.display(),
                orientation,
                "unknown EXIF orientation tag; defaulting to identity (no rotation)"
            );
            img
        }
    }
}

// ===== D1.0 — Shared rendering primitives =====

/// Aspect-ratio-preserving resize of an RGB buffer into a [`tiny_skia::Pixmap`].
///
/// If `long_edge` is `None`, the pixmap is filled at native size (no resize).
/// If `downscale_only` is `true` and the image is already smaller than
/// `long_edge`, it is emitted at native size (no upscale).
///
/// # Errors
///
/// Returns [`ExportError::InvalidDimensions`] if `long_edge < 16`, or
/// [`ExportError::AllocationFailed`] if pixmap allocation fails.
pub fn resize_rgb(
    rgb: &[u8],
    w: u32,
    h: u32,
    long_edge: Option<u32>,
    downscale_only: bool,
) -> Result<tiny_skia::Pixmap, ExportError> {
    let Some(limit) = long_edge else {
        // Native size — fill pixmap without rescaling.
        let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or(ExportError::AllocationFailed)?;
        rgba_fill(pixmap.data_mut(), rgb);
        return Ok(pixmap);
    };

    if limit < 16 {
        return Err(ExportError::InvalidDimensions);
    }

    let w_f = w as f32;
    let h_f = h as f32;
    let long_e = w_f.max(h_f);
    let mut scale = limit as f32 / long_e;
    if downscale_only {
        scale = scale.min(1.0);
    }
    let target_w = ((w_f * scale).round() as u32).max(1);
    let target_h = ((h_f * scale).round() as u32).max(1);

    // Source pixmap at native size.
    let mut src_pixmap = tiny_skia::Pixmap::new(w, h).ok_or(ExportError::AllocationFailed)?;
    rgba_fill(src_pixmap.data_mut(), rgb);

    // Target pixmap — bicubic resample.
    let mut pixmap =
        tiny_skia::Pixmap::new(target_w, target_h).ok_or(ExportError::AllocationFailed)?;
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Pattern::new(
            src_pixmap.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bicubic,
            1.0f32,
            tiny_skia::Transform::from_scale(scale, scale),
        ),
        ..Default::default()
    };
    let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, target_w as f32, target_h as f32)
        .ok_or(ExportError::InvalidDimensions)?;
    pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);

    Ok(pixmap)
}

/// D1c — Compute the shadow gradient ramp for a given image height and band fraction.
///
/// Returns a `Vec<u8>` of length `round(band_frac × H)`,
/// where `ramp[0] == 0` (transparent, top of band) and
/// `ramp[band_h − 1] == 255` (fully opaque, bottom row).
/// Returns an empty vec for tiny images where the band would be zero pixels.
///
/// The pinned denominator `(band_h − 1).max(1)` keeps the formula well-defined
/// for one-pixel bands. Use [`SHADOW_BAND_FRAC`] for the standard 30 % band.
pub fn shadow_alpha_ramp(image_h: u32, band_frac: f32) -> Vec<u8> {
    let band_h = (image_h as f32 * band_frac).round() as usize;
    if band_h == 0 {
        return vec![];
    }
    let denom = (band_h - 1).max(1) as f32;
    (0..band_h)
        .map(|i| ((i as f32 / denom) * 255.0).round() as u8)
        .collect()
}

/// Full rendering pipeline: resize → shadow → mark compositing → JPEG bytes.
///
/// # Fast-path
///
/// If `opts.long_edge` is `None`, `opts.shadow` is `None`, and `opts.marks`
/// is empty, the input `rgb` buffer is encoded directly via MozJPEG (no pixmap
/// allocation).
///
/// # Compositing order
///
/// resize → shadow gradient → mark1 → mark2 (caller controls order via
/// `opts.marks`).
///
/// # Errors
///
/// Returns [`ExportError::InvalidDimensions`] / [`ExportError::AllocationFailed`]
/// for geometry or allocation failures; [`ExportError::Geometry`] if a mark
/// cannot fit at its required size; [`ExportError::JpegEncode`] on MozJPEG
/// failure.
pub fn render_to_jpeg(
    rgb: &[u8],
    w: u32,
    h: u32,
    opts: &RenderOptions,
) -> Result<Vec<u8>, ExportError> {
    // Fast-path: no resize, no shadow, no marks — encode directly.
    if opts.long_edge.is_none() && opts.shadow.is_none() && opts.marks.is_empty() {
        return compress_jpeg(rgb, w, h, opts.quality);
    }

    // Step 1: Resize (or fill at native size).
    let mut pixmap = resize_rgb(rgb, w, h, opts.long_edge, opts.downscale_only)?;
    let pw = pixmap.width();
    let ph = pixmap.height();

    // Step 2: Shadow gradient (D1c).
    if let Some(ref shadow) = opts.shadow {
        apply_shadow_gradient(&mut pixmap, shadow.band_frac);
    }

    // Step 3: Mark compositing in caller-specified order.
    for mark in &opts.marks {
        composite_mark_on_pixmap(&mut pixmap, mark, pw, ph)?;
    }

    // Step 4: Demultiply + encode.
    let rgb_out = pixmap_to_rgb(&pixmap);
    compress_jpeg(&rgb_out, pw, ph, opts.quality)
}

/// Demultiply a tiny-skia RGBA pixmap into a packed 3-channel RGB buffer.
///
/// tiny-skia pre-multiplies alpha; this reverses that for pixels with
/// partial alpha (a ∈ 1..254). Opaque pixels (a = 255) pass through;
/// fully transparent (a = 0) become black.
#[must_use]
pub fn pixmap_to_rgb(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    let data = pixmap.data();
    let n = pixmap.width() as usize * pixmap.height() as usize;
    let mut out = Vec::with_capacity(n * 3);
    let mut idx = 0;
    while idx < data.len() {
        let r = data[idx];
        let g = data[idx + 1];
        let b = data[idx + 2];
        let a = data[idx + 3];
        match a {
            255 => {
                out.push(r);
                out.push(g);
                out.push(b);
            }
            0 => {
                out.push(0);
                out.push(0);
                out.push(0);
            }
            _ => {
                let af = a as f32;
                out.push(((r as f32 / af) * 255.0).round().clamp(0.0, 255.0) as u8);
                out.push(((g as f32 / af) * 255.0).round().clamp(0.0, 255.0) as u8);
                out.push(((b as f32 / af) * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
        idx += 4;
    }
    out
}

/// Compress an RGB buffer to JPEG bytes using MozJPEG inside a panic-safe FFI boundary.
///
/// # Errors
///
/// Returns [`ExportError::JpegEncode`] if MozJPEG fails or panics.
pub fn compress_jpeg(
    rgb_pixels: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> Result<Vec<u8>, ExportError> {
    // SAFETY: MozJPEG FFI bindings may panic; catch_unwind contains any panic.
    let res = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
        let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        comp.set_size(width as usize, height as usize);
        comp.set_quality(quality as f32);
        comp.set_progressive_mode();
        let mut writer = comp
            .start_compress(Vec::new())
            .map_err(|e| format!("start_compress: {e}"))?;
        writer
            .write_scanlines(rgb_pixels)
            .map_err(|e| format!("write_scanlines: {e}"))?;
        writer.finish().map_err(|e| format!("finish: {e}"))
    }));
    match res {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(msg)) => Err(ExportError::JpegEncode(msg)),
        Err(_) => Err(ExportError::JpegEncode("MozJPEG thread panic".to_string())),
    }
}

/// Composite an image badge watermark onto `pixmap` at `pos`.
///
/// The badge is scaled so its long edge equals `scale_pct`% of the image
/// long edge; placement is computed via [`calculate_watermark_position`] with
/// equal `margin_x == margin_y == long_edge × 0.015`.
///
/// # Errors
///
/// Returns [`ExportError::WatermarkOmitted`] if the badge cannot fit.
pub fn draw_image_watermark(
    pixmap: &mut tiny_skia::Pixmap,
    badge: &PreloadedBadge,
    pos: WatermarkPosition,
    target_w: u32,
    target_h: u32,
) -> Result<(), ExportError> {
    let badge_pixmap = &badge.pixmap;
    let long_edge_val = target_w.max(target_h) as f32;
    let scale_pct = badge.scale.map(|s| s.value()).unwrap_or(5.0);
    let target_badge_long = (long_edge_val * (scale_pct / 100.0)).max(1.0);

    let bw = badge_pixmap.width() as f32;
    let bh = badge_pixmap.height() as f32;
    let badge_long = bw.max(bh);
    let sf = target_badge_long / badge_long;

    let final_bw = (bw * sf).max(1.0);
    let final_bh = (bh * sf).max(1.0);

    let padding = (long_edge_val * 0.015).round().max(8.0);
    let (x_pos, y_pos) = calculate_watermark_position(
        pos,
        final_bw,
        final_bh,
        target_w as f32,
        target_h as f32,
        padding,
        padding,
    );

    if x_pos < 0.0
        || y_pos < 0.0
        || (x_pos + final_bw) > target_w as f32
        || (y_pos + final_bh) > target_h as f32
    {
        tracing::warn!("image watermark does not fit on image, omitting");
        return Err(ExportError::WatermarkOmitted);
    }

    blit_badge_at(pixmap, badge_pixmap, x_pos, y_pos, sf)
}

/// Compute the top-left origin of a mark given its size, target image size, and per-axis margins.
///
/// This is the 2-axis version (D1d): `margin_x` and `margin_y` may differ.
/// The export subcommand passes `margin_x == margin_y == (long_edge × 0.015).max(8.0)`.
#[must_use]
pub fn calculate_watermark_position(
    pos: WatermarkPosition,
    width: f32,
    height: f32,
    target_w: f32,
    target_h: f32,
    margin_x: f32,
    margin_y: f32,
) -> (f32, f32) {
    match pos {
        WatermarkPosition::BottomLeft => (margin_x, target_h - margin_y - height),
        WatermarkPosition::TopRight => (target_w - margin_x - width, margin_y),
        WatermarkPosition::TopLeft => (margin_x, margin_y),
        WatermarkPosition::BottomRight => {
            (target_w - margin_x - width, target_h - margin_y - height)
        }
        WatermarkPosition::Center => ((target_w - width) / 2.0, (target_h - height) / 2.0),
    }
}

// ===== Private rendering helpers =====

/// Fill a tiny-skia RGBA data buffer from a packed RGB slice.
fn rgba_fill(dst: &mut [u8], rgb: &[u8]) {
    let mut src = 0;
    let mut d = 0;
    while src < rgb.len() {
        dst[d] = rgb[src];
        dst[d + 1] = rgb[src + 1];
        dst[d + 2] = rgb[src + 2];
        dst[d + 3] = 255;
        src += 3;
        d += 4;
    }
}

/// Apply the shadow gradient to the bottom band of `pixmap` in-place.
///
/// The shadow band covers the bottom `round(H × band_frac)` rows.
/// Each row is darkened by `out = base × (1 − t)` where `t = ramp[row] / 255`.
/// The alpha channel (byte 3) is not modified.
fn apply_shadow_gradient(pixmap: &mut tiny_skia::Pixmap, band_frac: f32) {
    let ph = pixmap.height() as usize;
    let pw = pixmap.width() as usize;
    let ramp = shadow_alpha_ramp(pixmap.height(), band_frac);
    if ramp.is_empty() {
        return;
    }
    let band_start = ph - ramp.len();
    let data = pixmap.data_mut();
    for (band_row, &alpha) in ramp.iter().enumerate() {
        let row = band_start + band_row;
        let t = alpha as f32 / 255.0;
        let factor = 1.0 - t;
        for col in 0..pw {
            let base = (row * pw + col) * 4;
            data[base] = (data[base] as f32 * factor).round() as u8;
            data[base + 1] = (data[base + 1] as f32 * factor).round() as u8;
            data[base + 2] = (data[base + 2] as f32 * factor).round() as u8;
            // alpha channel (base + 3) stays 255.
        }
    }
}

/// Composite one [`MarkSpec`] onto `pixmap` using the new slot-based geometry.
///
/// For `BadgeSizeBasis::Height` marks the validated `MarkPlacement::fit` path
/// is used, ensuring the production path exercises the same guards as the tests.
/// Margins are **fractions** (stored in `MarkSpec.margin_x/margin_y`), computed
/// into pixels against the post-resize `pw`/`ph`.
fn composite_mark_on_pixmap(
    pixmap: &mut tiny_skia::Pixmap,
    mark: &MarkSpec,
    pw: u32,
    ph: u32,
) -> Result<(), ExportError> {
    let bw = mark.badge.width() as f32;
    let bh = mark.badge.height() as f32;

    match &mark.basis {
        BadgeSizeBasis::Height(frac) => {
            // Route through validated MarkPlacement so the production path
            // exercises the same guards as the unit tests.
            let placement = MarkPlacement::fit(
                (pw, ph),
                (mark.badge.width(), mark.badge.height()),
                *frac,
                mark.margin_x, // margin_x is a fraction (0..1)
                mark.slot,
            )
            .map_err(ExportError::Geometry)?;
            let sf = placement.h() as f32 / bh.max(1.0);
            blit_badge_at(
                pixmap,
                &mark.badge,
                placement.x() as f32,
                placement.y() as f32,
                sf,
            )
        }
        BadgeSizeBasis::LongEdge(scale_pct) => {
            // Legacy path used by export_photo re-point.
            // Margins are fractions of post-resize dimensions.
            let long_e = pw.max(ph) as f32;
            let target_long = (long_e * (scale_pct.value() / 100.0)).max(1.0);
            let badge_long = bw.max(bh);
            let sf = target_long / badge_long.max(1.0);
            let mark_w = ((bw * sf).round() as u32).max(1);
            let mark_h = ((bh * sf).round() as u32).max(1);

            let mx = (pw as f32 * mark.margin_x).round() as u32;
            let my = (ph as f32 * mark.margin_y).round() as u32;
            let (x, y) = match mark.slot {
                MarkSlot::Mark1 => {
                    let x = pw
                        .checked_sub(mx)
                        .and_then(|v| v.checked_sub(mark_w))
                        .ok_or(ExportError::Geometry(GeometryError::MarkDoesNotFit {
                            which: MarkSlot::Mark1,
                            mark_dims: (mark_w, mark_h),
                            target_dims: (pw, ph),
                        }))?;
                    (x, my)
                }
                MarkSlot::Mark2 => {
                    let y = ph
                        .checked_sub(my)
                        .and_then(|v| v.checked_sub(mark_h))
                        .ok_or(ExportError::Geometry(GeometryError::MarkDoesNotFit {
                            which: MarkSlot::Mark2,
                            mark_dims: (mark_w, mark_h),
                            target_dims: (pw, ph),
                        }))?;
                    (mx, y)
                }
            };

            if x.checked_add(mark_w).is_none_or(|e| e > pw)
                || y.checked_add(mark_h).is_none_or(|e| e > ph)
            {
                let which = mark.slot;
                return Err(ExportError::Geometry(GeometryError::MarkDoesNotFit {
                    which,
                    mark_dims: (mark_w, mark_h),
                    target_dims: (pw, ph),
                }));
            }

            blit_badge_at(pixmap, &mark.badge, x as f32, y as f32, sf)
        }
    }
}

/// Alpha-blend a badge (scaled by `scale_factor`) into `pixmap` at pixel `(x, y)`.
fn blit_badge_at(
    pixmap: &mut tiny_skia::Pixmap,
    badge: &tiny_skia::Pixmap,
    x: f32,
    y: f32,
    scale_factor: f32,
) -> Result<(), ExportError> {
    let final_w = (badge.width() as f32 * scale_factor).max(1.0).ceil() as u32;
    let final_h = (badge.height() as f32 * scale_factor).max(1.0).ceil() as u32;

    let paint = tiny_skia::Paint {
        shader: tiny_skia::Pattern::new(
            badge.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Bicubic,
            1.0,
            tiny_skia::Transform::from_scale(scale_factor, scale_factor),
        ),
        ..Default::default()
    };

    let mut tmp = tiny_skia::Pixmap::new(final_w, final_h).ok_or(ExportError::AllocationFailed)?;
    let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, final_w as f32, final_h as f32)
        .ok_or(ExportError::InvalidDimensions)?;
    tmp.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);

    let src = tmp.data();
    let dst_w = pixmap.width() as i32;
    let dst_h = pixmap.height() as i32;
    let dst = pixmap.data_mut();
    let base_x = x.round() as i32;
    let base_y = y.round() as i32;

    for row in 0..final_h as i32 {
        for col in 0..final_w as i32 {
            let dx = base_x + col;
            let dy = base_y + row;
            if dx < 0 || dx >= dst_w || dy < 0 || dy >= dst_h {
                continue;
            }
            let si = ((row * final_w as i32 + col) * 4) as usize;
            let di = ((dy * dst_w + dx) * 4) as usize;
            let sa = src[si + 3] as f32;
            let inv_a = 1.0 - sa / 255.0;
            dst[di] = (src[si] as f32 + dst[di] as f32 * inv_a).clamp(0.0, 255.0) as u8;
            dst[di + 1] = (src[si + 1] as f32 + dst[di + 1] as f32 * inv_a).clamp(0.0, 255.0) as u8;
            dst[di + 2] = (src[si + 2] as f32 + dst[di + 2] as f32 * inv_a).clamp(0.0, 255.0) as u8;
            // dst[di + 3] left at 255.
        }
    }

    Ok(())
}

// ===== Text watermark helpers (export subcommand only) =====

fn draw_text_watermark(
    pixmap: &mut tiny_skia::Pixmap,
    text: &str,
    pos: WatermarkPosition,
    target_w: u32,
    target_h: u32,
) -> Result<(), ExportError> {
    let long_edge_val = target_w.max(target_h) as f32;
    let font_size = (long_edge_val * 0.02).round().max(12.0);
    let padding = (long_edge_val * 0.015).round().max(8.0);
    let mut omitted = false;

    FONT_SYSTEM.with(|fs_cell| {
        SWASH_CACHE.with(|cache_cell| {
            let mut fs = fs_cell.borrow_mut();
            let mut cache = cache_cell.borrow_mut();
            let mut buf =
                cosmic_text::Buffer::new(&mut fs, cosmic_text::Metrics::new(font_size, font_size));
            buf.set_size(&mut fs, Some(target_w as f32), None);
            buf.set_text(
                &mut fs,
                text,
                cosmic_text::Attrs::new().family(cosmic_text::Family::Monospace),
                cosmic_text::Shaping::Basic,
            );
            buf.shape_until_scroll(&mut fs, false);

            let mut max_w = 0.0f32;
            let mut total_h = 0.0f32;
            let runs: Vec<_> = buf.layout_runs().collect();
            if !runs.is_empty() {
                for run in &runs {
                    if run.line_w > max_w {
                        max_w = run.line_w;
                    }
                }
                let last = &runs[runs.len() - 1];
                total_h = last.line_y + font_size;
            }

            let (x_pos, y_pos) = calculate_watermark_position(
                pos,
                max_w,
                total_h,
                target_w as f32,
                target_h as f32,
                padding,
                padding,
            );

            if x_pos < 0.0
                || y_pos < 0.0
                || (x_pos + max_w) > target_w as f32
                || (y_pos + total_h) > target_h as f32
            {
                tracing::warn!(text, "text watermark does not fit on image, omitting");
                omitted = true;
                return;
            }

            let offset = if font_size < 40.0 { 1 } else { 2 };
            let shadow_color = tiny_skia::Color::from_rgba8(0, 0, 0, 76);
            let text_color = tiny_skia::Color::from_rgba8(255, 255, 255, 178);
            for (dx, dy) in [
                (-offset, -offset),
                (offset, -offset),
                (-offset, offset),
                (offset, offset),
            ] {
                draw_text_at(
                    pixmap,
                    &buf,
                    &mut fs,
                    &mut cache,
                    x_pos + dx as f32,
                    y_pos + dy as f32,
                    shadow_color,
                );
            }
            draw_text_at(pixmap, &buf, &mut fs, &mut cache, x_pos, y_pos, text_color);
        });
    });

    if omitted {
        Err(ExportError::WatermarkOmitted)
    } else {
        Ok(())
    }
}

fn draw_text_at(
    pixmap: &mut tiny_skia::Pixmap,
    buffer: &cosmic_text::Buffer,
    fs: &mut cosmic_text::FontSystem,
    cache: &mut cosmic_text::SwashCache,
    x_pos: f32,
    y_pos: f32,
    color: tiny_skia::Color,
) {
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((x_pos, y_pos), 1.0);
            if let Some(image) = cache.get_image(fs, physical.cache_key) {
                let gx = physical.x + image.placement.left;
                let gy = physical.y - image.placement.top;
                for py in 0..image.placement.height {
                    for px in 0..image.placement.width {
                        let mask = image.data[(py * image.placement.width + px) as usize];
                        if mask == 0 {
                            continue;
                        }
                        blend_pixel(pixmap, gx + px as i32, gy + py as i32, color, mask);
                    }
                }
            }
        }
    }
}

fn blend_pixel(
    pixmap: &mut tiny_skia::Pixmap,
    x: i32,
    y: i32,
    color: tiny_skia::Color,
    mask_alpha: u8,
) {
    if x < 0 || x >= pixmap.width() as i32 || y < 0 || y >= pixmap.height() as i32 {
        return;
    }
    let idx = (y as usize * pixmap.width() as usize + x as usize) * 4;
    let data = pixmap.data_mut();
    let f = (mask_alpha as f32 / 255.0) * color.alpha();
    if f <= 0.0 {
        return;
    }
    let src_r = color.red() * f * 255.0;
    let src_g = color.green() * f * 255.0;
    let src_b = color.blue() * f * 255.0;
    let src_a = f * 255.0;
    let inv = 1.0 - f;
    data[idx] = (src_r + data[idx] as f32 * inv).round().clamp(0.0, 255.0) as u8;
    data[idx + 1] = (src_g + data[idx + 1] as f32 * inv)
        .round()
        .clamp(0.0, 255.0) as u8;
    data[idx + 2] = (src_b + data[idx + 2] as f32 * inv)
        .round()
        .clamp(0.0, 255.0) as u8;
    data[idx + 3] = (src_a + data[idx + 3] as f32 * inv)
        .round()
        .clamp(0.0, 255.0) as u8;
}

// ===== export_photo (re-pointed to use shared primitives) =====

/// Core function to export a single photo.
///
/// Decodes the RAW file via LibRaw (16-bit linear), applies the filmic ISP
/// tone-mapping LUT, then delegates to the shared rendering pipeline.
///
/// Image badge marks at `TopRight`/`BottomLeft` are translated to [`MarkSpec`]
/// and processed via [`render_to_jpeg`]; text watermarks and badges at other
/// positions use the legacy pixmap path.
#[tracing::instrument(skip(options))]
pub fn export_photo(
    options: &ExportOptions,
    source_path: &Path,
    _metadata: &ExportMetadata,
) -> Result<(), ExportError> {
    // 1. Decode RAW to 16-bit linear.
    let processed = photohelper_raw::decode::decode_image(
        source_path,
        photohelper_raw::decode::ProcessOptions::Linear16,
    )
    .map_err(|e| ExportError::RawDecode(e.to_string()))?;

    let linear_img = match processed {
        photohelper_raw::decode::ProcessedImage::Linear16(img) => img,
        _ => {
            return Err(ExportError::RawDecode(
                "expected 16-bit linear image".to_string(),
            ));
        }
    };

    let width = linear_img.width.get();
    let height = linear_img.height.get();
    let channels = linear_img.channels as usize;

    if width == 0 || height == 0 {
        return Err(ExportError::InvalidDimensions);
    }
    if channels < 3 {
        return Err(ExportError::RawDecode(
            "image has fewer than 3 channels".to_string(),
        ));
    }

    // 2. Filmic ISP tone mapping (LUT-accelerated).
    let lut = ToneMappingLut::new(options.tone_mapping.exposure_ev);
    let mut rgb_pixels = Vec::with_capacity((width * height * 3) as usize);
    for pixel in linear_img.data.chunks_exact(channels) {
        rgb_pixels.push(lut.apply(pixel[0]));
        rgb_pixels.push(lut.apply(pixel[1]));
        rgb_pixels.push(lut.apply(pixel[2]));
    }

    // 3. Separate legacy watermarks from the new MarkSpec system.
    let has_text = options
        .watermarks
        .values()
        .any(|w| matches!(w, Watermark::Text(_)));

    // Positions not supported by MarkSlot (TopLeft, BottomRight, Center).
    let has_unsupported_pos = options.watermarks.iter().any(|(pos, w)| {
        matches!(w, Watermark::Image(_))
            && !matches!(
                pos,
                WatermarkPosition::TopRight | WatermarkPosition::BottomLeft
            )
    });

    if has_text || has_unsupported_pos {
        // Legacy path: resize_rgb → draw_text_watermark / draw_image_watermark → compress.
        let tmp_path = &options.output_path;
        if options.long_edge.is_none() && options.watermarks.is_empty() {
            let bytes = compress_jpeg(&rgb_pixels, width, height, options.quality)?;
            std::fs::write(tmp_path, bytes)?;
            return Ok(());
        }
        let mut pixmap = resize_rgb(&rgb_pixels, width, height, options.long_edge, false)?;
        let pw = pixmap.width();
        let ph = pixmap.height();
        for (pos, wm) in &options.watermarks {
            match wm {
                Watermark::Text(text) => {
                    draw_text_watermark(&mut pixmap, text, *pos, pw, ph)?;
                }
                Watermark::Image(badge) => {
                    draw_image_watermark(&mut pixmap, badge, *pos, pw, ph)?;
                }
            }
        }
        let rgb_out = pixmap_to_rgb(&pixmap);
        let bytes = compress_jpeg(&rgb_out, pw, ph, options.quality)?;
        std::fs::write(tmp_path, bytes)?;
        return Ok(());
    }

    // 4. New path: translate image badges at TopRight/BottomLeft to MarkSpec.
    // margin_x/margin_y are fractions (0..1); 0.015 ≈ export's (long_edge × 0.015) / long_edge.
    const EXPORT_MARK_MARGIN_FRAC: f32 = 0.015;
    let marks: Vec<MarkSpec> = options
        .watermarks
        .iter()
        .filter_map(|(pos, wm)| {
            if let Watermark::Image(badge) = wm {
                let slot = match pos {
                    WatermarkPosition::TopRight => MarkSlot::Mark1,
                    WatermarkPosition::BottomLeft => MarkSlot::Mark2,
                    _ => return None,
                };
                Some(MarkSpec {
                    badge: badge.pixmap.clone(),
                    basis: BadgeSizeBasis::LongEdge(badge.scale.unwrap_or(Scale::new(5.0))),
                    slot,
                    margin_x: EXPORT_MARK_MARGIN_FRAC,
                    margin_y: EXPORT_MARK_MARGIN_FRAC,
                })
            } else {
                None
            }
        })
        .collect();

    let render_opts = RenderOptions {
        long_edge: options.long_edge,
        downscale_only: false,
        quality: options.quality,
        shadow: None,
        marks,
    };

    let jpeg_bytes = render_to_jpeg(&rgb_pixels, width, height, &render_opts)?;
    std::fs::write(&options.output_path, jpeg_bytes)?;

    Ok(())
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Existing regression tests (kept with updated call sites) -----

    #[test]
    fn test_rating_clamping() {
        assert_eq!(Rating::new(10).value(), 5);
        assert_eq!(Rating::new(-5).value(), -1);
        assert_eq!(Rating::new(3).value(), 3);
    }

    #[test]
    fn test_nima_score_validation() {
        assert!(NimaScore::new(f32::NAN).is_none());
        assert!(NimaScore::new(f32::INFINITY).is_none());
        assert_eq!(NimaScore::new(6.5).unwrap().value(), 6.5);
    }

    /// RT-C — `pixmap_to_rgb` demultiplication regression (hand-computed expected values).
    #[test]
    fn test_pixel_demultiplication() {
        let mut pixmap = tiny_skia::Pixmap::new(2, 2).unwrap();
        {
            let data = pixmap.data_mut();
            // Pixel 0: opaque red [255, 0, 0, 255]
            data[0] = 255;
            data[1] = 0;
            data[2] = 0;
            data[3] = 255;
            // Pixel 1: half-transparent green [0, 100, 0, 200]
            data[4] = 0;
            data[5] = 100;
            data[6] = 0;
            data[7] = 200;
            // Pixel 2: fully transparent [0, 0, 0, 0]
            data[8] = 0;
            data[9] = 0;
            data[10] = 0;
            data[11] = 0;
            // Pixel 3: opaque white [255, 255, 255, 255]
            data[12] = 255;
            data[13] = 255;
            data[14] = 255;
            data[15] = 255;
        }
        let rgb = pixmap_to_rgb(&pixmap);
        assert_eq!(rgb[0..3], [255, 0, 0]);
        // 100 / 200 * 255 = 127.5 → 128
        assert_eq!(rgb[3..6], [0, 128, 0]);
        assert_eq!(rgb[6..9], [0, 0, 0]);
        assert_eq!(rgb[9..12], [255, 255, 255]);
    }

    #[test]
    fn test_scale_clamping() {
        assert_eq!(Scale::new(150.0).value(), 100.0);
        assert_eq!(Scale::new(0.0).value(), 0.001);
        assert_eq!(Scale::new(-5.0).value(), 0.001);
        assert_eq!(Scale::new(5.0).value(), 5.0);
    }

    /// RT-C — `test_watermark_position_calculation` migrated to 2-axis signature.
    #[test]
    fn test_watermark_position_calculation() {
        // 1×1 target with zero margins: centre of a 10×10 watermark sits at −4.5.
        let (x, y) = calculate_watermark_position(
            WatermarkPosition::Center,
            10.0,
            10.0,
            1.0,
            1.0,
            0.0,
            0.0, // margin_x, margin_y
        );
        assert_eq!(x, -4.5);
        assert_eq!(y, -4.5);
    }

    #[test]
    fn test_tone_mapping_lut() {
        let lut = ToneMappingLut::new(0.0);
        assert_eq!(lut.apply(0), 0);
        assert!(lut.apply(65535) > 230);
        let lut_plus = ToneMappingLut::new(1.0);
        assert!(lut_plus.apply(10000) > lut.apply(10000));
    }

    // ----- D1b — Geometry tests -----

    /// Geometry: Mark1 (top-right) in a landscape image.
    #[test]
    fn test_mark_placement_mark1_landscape() {
        // 1000×600 image, badge 200×100 (landscape logo)
        // mark_h = round(600 * 0.14) = round(84) = 84
        // scale = 84 / 100 = 0.84
        // mark_w = round(200 * 0.84) = round(168) = 168
        // margin_x = round(1000 * 0.046) = round(46) = 46
        // margin_y = round(600 * 0.046) = round(27.6) = 28
        // x = 1000 - 46 - 168 = 786
        // y = 28
        let p = MarkPlacement::fit(
            (1000, 600),
            (200, 100),
            MARK1_HEIGHT_FRAC,
            MARK_MARGIN_FRAC,
            MarkSlot::Mark1,
        )
        .unwrap();
        assert_eq!(p.h(), 84, "mark_h");
        assert_eq!(p.w(), 168, "mark_w");
        assert_eq!(p.x(), 786, "x");
        assert_eq!(p.y(), 28, "y");
    }

    /// Geometry: Mark2 (bottom-left) in a landscape image.
    #[test]
    fn test_mark_placement_mark2_landscape() {
        // 1000×600 image, badge 200×100
        // mark_h = round(600 * 0.13) = round(78) = 78
        // scale = 78 / 100 = 0.78
        // mark_w = round(200 * 0.78) = round(156) = 156
        // margin_x = round(1000 * 0.046) = 46
        // margin_y = round(600 * 0.046) = 28
        // x = 46
        // y = 600 - 28 - 78 = 494
        let p = MarkPlacement::fit(
            (1000, 600),
            (200, 100),
            MARK2_HEIGHT_FRAC,
            MARK_MARGIN_FRAC,
            MarkSlot::Mark2,
        )
        .unwrap();
        assert_eq!(p.h(), 78);
        assert_eq!(p.w(), 156);
        assert_eq!(p.x(), 46);
        assert_eq!(p.y(), 494);
    }

    /// Geometry: Mark2 sits inside the shadow band.
    #[test]
    fn test_mark2_inside_shadow_band() {
        let (tw, th): (u32, u32) = (1000, 600);
        let p = MarkPlacement::fit(
            (tw, th),
            (200, 100),
            MARK2_HEIGHT_FRAC,
            MARK_MARGIN_FRAC,
            MarkSlot::Mark2,
        )
        .unwrap();
        let band_h = (th as f32 * SHADOW_BAND_FRAC).round() as u32;
        assert!(
            p.y() >= th - band_h,
            "mark2 y={} should be within shadow band starting at {}",
            p.y(),
            th - band_h
        );
    }

    /// Geometry: wide-logo badge cannot fit at required height → MarkDoesNotFit.
    #[test]
    fn test_mark_placement_doesnt_fit() {
        // 50×50 image, badge 500×20 (very wide logo):
        // mark_h = round(50 * 0.50) = 25; scale = 25/20 = 1.25
        // mark_w = round(500 * 1.25) = 625 → far exceeds image width 50
        // → checked_sub underflows → MarkDoesNotFit
        let result =
            MarkPlacement::fit((50, 50), (500, 20), 0.50, MARK_MARGIN_FRAC, MarkSlot::Mark1);
        assert!(
            result.is_err(),
            "Expected MarkDoesNotFit for oversized wide badge"
        );
        let GeometryError::MarkDoesNotFit { which, .. } = result.unwrap_err();
        assert_eq!(which, MarkSlot::Mark1);
    }

    // ----- D1c — Shadow gradient tests -----

    /// `shadow_alpha_ramp` endpoints and monotonicity.
    #[test]
    fn test_shadow_alpha_ramp_endpoints() {
        let ramp = shadow_alpha_ramp(100, SHADOW_BAND_FRAC);
        let band_h = (100.0_f32 * SHADOW_BAND_FRAC).round() as usize;
        assert_eq!(ramp.len(), band_h, "ramp length == round(0.30 * H)");
        assert_eq!(ramp[0], 0, "top of band is transparent");
        assert_eq!(ramp[band_h - 1], 255, "bottom row is fully opaque");
        // Monotonic non-decreasing.
        for w in ramp.windows(2) {
            assert!(w[0] <= w[1], "ramp must be monotonically non-decreasing");
        }
    }

    /// Small H with band_h == 0 → empty ramp.
    #[test]
    fn test_shadow_alpha_ramp_tiny_image() {
        // H = 2: band_h = round(0.30 * 2) = 1 → valid ramp of len 1
        let ramp = shadow_alpha_ramp(2, SHADOW_BAND_FRAC);
        assert_eq!(ramp.len(), 1);
        assert_eq!(
            ramp[0], 0,
            "single-element band: row 0 of band = ramp[0] = 0 (no darkening)"
        );
        // H = 1: band_h = round(0.30 * 1) = 0 → empty
        let ramp0 = shadow_alpha_ramp(1, SHADOW_BAND_FRAC);
        assert!(ramp0.is_empty(), "band_h==0 → empty ramp");
    }

    /// Shadow compositing: mid-band row at t=0.5 → base 200 → output 100.
    #[test]
    fn test_shadow_compositing_exact_mid_band() {
        // H = 10, band_h = round(0.30 * 10) = 3
        // ramp: [0, 128, 255]  (denom = 2, ramp[1] = round(1/2 * 255) = 128)
        // t at ramp[1] = 128/255; factor = 1 - 128/255 = 127/255
        // out = round(200 * 127/255) = round(99.608) = 100
        let h = 10u32;
        let w = 4u32;
        let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
        // Fill all pixels with RGB (200, 200, 200, 255).
        {
            let data = pixmap.data_mut();
            for chunk in data.chunks_exact_mut(4) {
                chunk[0] = 200;
                chunk[1] = 200;
                chunk[2] = 200;
                chunk[3] = 255;
            }
        }
        apply_shadow_gradient(&mut pixmap, SHADOW_BAND_FRAC);
        // Band starts at row 10 - 3 = 7. Mid-band row = 7 + 1 = 8.
        let data = pixmap.data();
        let mid_band_row = 8usize;
        let pixel_base = mid_band_row * w as usize * 4;
        // Check alpha stays 255.
        assert_eq!(data[pixel_base + 3], 255, "alpha must remain 255");
        // Check darkening.
        let expected = 100u8;
        assert_eq!(data[pixel_base], expected, "R at mid-band row");
        assert_eq!(data[pixel_base + 1], expected, "G at mid-band row");
        assert_eq!(data[pixel_base + 2], expected, "B at mid-band row");
    }

    /// Row ABOVE the shadow band is bit-identical to the source.
    #[test]
    fn test_shadow_row_above_band_unchanged() {
        let h = 10u32;
        let w = 2u32;
        let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
        {
            let data = pixmap.data_mut();
            for chunk in data.chunks_exact_mut(4) {
                chunk[0] = 123;
                chunk[1] = 45;
                chunk[2] = 67;
                chunk[3] = 255;
            }
        }
        apply_shadow_gradient(&mut pixmap, SHADOW_BAND_FRAC);
        // band_h = round(0.30 * 10) = 3 → starts at row 7.
        // Row 6 (one above band) must be unchanged.
        let data = pixmap.data();
        let row6 = 6 * w as usize * 4;
        assert_eq!(data[row6], 123);
        assert_eq!(data[row6 + 1], 45);
        assert_eq!(data[row6 + 2], 67);
        assert_eq!(data[row6 + 3], 255);
    }

    // ----- D1.0 — resize_rgb downscale-only tests -----

    /// Sub-limit image emitted at native size (no upscale).
    #[test]
    fn test_resize_rgb_no_upscale() {
        // 100×50 image, long_edge limit 800, downscale_only = true → stays 100×50.
        let rgb = vec![128u8; 100 * 50 * 3];
        let pixmap = resize_rgb(&rgb, 100, 50, Some(800), true).unwrap();
        assert_eq!(pixmap.width(), 100);
        assert_eq!(pixmap.height(), 50);
    }

    /// Downscale: 1000×500 with long_edge 200 → 200×100.
    #[test]
    fn test_resize_rgb_downscale() {
        let rgb = vec![0u8; 1000 * 500 * 3];
        let pixmap = resize_rgb(&rgb, 1000, 500, Some(200), false).unwrap();
        assert_eq!(pixmap.width(), 200);
        assert_eq!(pixmap.height(), 100);
    }

    /// Aspect ratio preserved within ±1 pixel.
    #[test]
    fn test_resize_rgb_aspect_ratio_preserved() {
        let rgb = vec![0u8; 100 * 50 * 3];
        let pixmap = resize_rgb(&rgb, 100, 50, Some(20), false).unwrap();
        let w = pixmap.width() as f64;
        let h = pixmap.height() as f64;
        let original_ratio = 100.0_f64 / 50.0;
        let actual_ratio = w / h;
        assert!(
            (original_ratio - actual_ratio).abs() < 0.1,
            "aspect ratio drift"
        );
    }

    // ----- D1a — Source kind classification -----

    #[test]
    fn test_source_kind_classify() {
        use std::path::Path;
        assert_eq!(
            SourceKind::classify(Path::new("a.jpg")),
            Some(SourceKind::Raster)
        );
        assert_eq!(
            SourceKind::classify(Path::new("a.JPEG")),
            Some(SourceKind::Raster)
        );
        assert_eq!(
            SourceKind::classify(Path::new("a.png")),
            Some(SourceKind::Raster)
        );
        assert_eq!(
            SourceKind::classify(Path::new("a.CR3")),
            Some(SourceKind::Cr3)
        );
        assert_eq!(
            SourceKind::classify(Path::new("a.nef")),
            Some(SourceKind::UntestedRaw)
        );
        assert_eq!(
            SourceKind::classify(Path::new("a.arw")),
            Some(SourceKind::UntestedRaw)
        );
        assert_eq!(SourceKind::classify(Path::new("a.txt")), None);
        assert_eq!(SourceKind::classify(Path::new("no_ext")), None);
    }

    // ----- D1.0 — render_to_jpeg RT-C regression -----

    /// RT-C: `render_to_jpeg` with default options — not upscaled, no shadow,
    /// decoded output pixel close to input (128 ± JPEG tolerance).
    #[test]
    fn test_render_to_jpeg_default_no_shadow_no_marks() {
        let w = 8u32;
        let h = 8u32;
        let rgb = vec![128u8; (w * h * 3) as usize];
        let opts = RenderOptions::default();
        let bytes = render_to_jpeg(&rgb, w, h, &opts).unwrap();
        assert!(!bytes.is_empty(), "JPEG bytes must not be empty");
        assert_eq!(bytes[0], 0xFF, "must start with JPEG SOI FF");
        assert_eq!(bytes[1], 0xD8, "must start with JPEG SOI D8");

        // Decode back: dimensions must match and pixel must be near 128.
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(
            decoded.width(),
            w,
            "output width must equal input (no upscale)"
        );
        assert_eq!(
            decoded.height(),
            h,
            "output height must equal input (no upscale)"
        );
        let [r, g, b] = decoded.get_pixel(0, 0).0;
        assert!(
            (r as i32 - 128).abs() <= 8,
            "top-left R={r} must be near 128 (no shadow)"
        );
        assert!(
            (g as i32 - 128).abs() <= 8,
            "top-left G={g} must be near 128"
        );
        assert!(
            (b as i32 - 128).abs() <= 8,
            "top-left B={b} must be near 128"
        );
    }

    /// Portrait geometry: mark1 (top-right) in a portrait image.
    #[test]
    fn test_mark_placement_mark1_portrait() {
        // 600×1000 portrait image, badge 100×200
        // mark_h = round(1000 * 0.14) = 140; scale = 140/200 = 0.7
        // mark_w = round(100 * 0.7) = 70
        // margin_x = round(600 * 0.046) = 28; margin_y = round(1000 * 0.046) = 46
        // x = 600 - 28 - 70 = 502; y = 46
        let p = MarkPlacement::fit(
            (600, 1000),
            (100, 200),
            MARK1_HEIGHT_FRAC,
            MARK_MARGIN_FRAC,
            MarkSlot::Mark1,
        )
        .unwrap();
        assert_eq!(p.h(), 140, "mark_h");
        assert_eq!(p.w(), 70, "mark_w");
        assert_eq!(p.x(), 502, "x");
        assert_eq!(p.y(), 46, "y");
    }

    /// Square geometry: mark2 (bottom-left) in a square image.
    #[test]
    fn test_mark_placement_mark2_square() {
        // 800×800 square image, badge 200×100
        // mark_h = round(800 * 0.13) = round(104) = 104; scale = 104/100 = 1.04
        // mark_w = round(200 * 1.04) = round(208) = 208
        // margin_x = round(800 * 0.046) = round(36.8) = 37
        // margin_y = round(800 * 0.046) = 37
        // y = 800 - 37 - 104 = 659; x = 37
        let p = MarkPlacement::fit(
            (800, 800),
            (200, 100),
            MARK2_HEIGHT_FRAC,
            MARK_MARGIN_FRAC,
            MarkSlot::Mark2,
        )
        .unwrap();
        assert_eq!(p.h(), 104, "mark_h");
        assert_eq!(p.w(), 208, "mark_w");
        assert_eq!(p.x(), 37, "x");
        assert_eq!(p.y(), 659, "y");
    }

    // ----- D1a raster decode tests (RT-J) -----

    /// Truncated JPEG → `RasterDecodeFailed`.
    #[test]
    fn test_load_source_image_truncated_jpeg_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("truncated.jpg");
        // Write only the JPEG SOI header (2 bytes) — not a valid JPEG.
        std::fs::write(&path, [0xFF_u8, 0xD8]).unwrap();
        let result = load_source_image(&path, false);
        assert!(
            matches!(result, Err(ExportError::RasterDecodeFailed { .. })),
            "truncated JPEG must produce RasterDecodeFailed, got: {result:?}"
        );
    }

    /// JPEG with EXIF Orientation=6 (90° CW rotation) → width/height swapped.
    #[test]
    fn test_load_source_image_exif_orientation_6_portrait() {
        // Build a 4×8 JPEG (width=4, height=8) with Orientation=6.
        // After applying orientation=6 (rotate 90° CW), the result should be 8×4 (w=8, h=4).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("portrait.jpg");

        // Create a 4×8 image and save with EXIF orientation=6 embedded in the APP1 marker.
        // We'll create the image using the `image` crate, then manually patch the EXIF bytes.
        let img = image::RgbImage::from_pixel(4, 8, image::Rgb([100u8, 150u8, 200u8]));
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        let mut buf = std::io::Cursor::new(Vec::new());
        dyn_img
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        let base_bytes = buf.into_inner();

        // Build a minimal EXIF APP1 marker with Orientation=6.
        // TIFF LE: II + magic(42) + IFD offset(8)
        // IFD: 1 entry + tag(0x0112) + type(3=SHORT) + count(1) + value(6)
        let mut exif_tiff = vec![
            0x49, 0x49, // "II" — little-endian
            0x2A, 0x00, // TIFF magic 42
            0x08, 0x00, 0x00, 0x00, // IFD offset = 8
            0x01, 0x00, // 1 IFD entry
            0x12, 0x01, // Tag 0x0112 (Orientation)
            0x03, 0x00, // Type SHORT
            0x01, 0x00, 0x00, 0x00, // Count 1
            0x06, 0x00, 0x00, 0x00, // Value 6 (90° CW rotation)
        ];
        let exif_header = b"Exif\x00\x00";
        let mut app1_data: Vec<u8> = exif_header.to_vec();
        app1_data.append(&mut exif_tiff);

        // Compute APP1 segment length (includes the 2 length bytes itself).
        let seg_len = (app1_data.len() + 2) as u16;
        let mut patched: Vec<u8> = vec![0xFF, 0xD8]; // SOI
        patched.push(0xFF);
        patched.push(0xE1); // APP1 marker
        patched.extend_from_slice(&seg_len.to_be_bytes());
        patched.extend_from_slice(&app1_data);
        // Append the rest of the JPEG (skip original SOI).
        patched.extend_from_slice(&base_bytes[2..]);
        std::fs::write(&path, &patched).unwrap();

        let img = load_source_image(&path, false).unwrap();
        // Orientation=6 rotates 90° CW: 4×8 → 8×4 (w=8, h=4).
        assert_eq!(
            img.width().get(),
            8,
            "after orientation=6: width should be original height"
        );
        assert_eq!(
            img.height().get(),
            4,
            "after orientation=6: height should be original width"
        );
    }

    /// JPEG with unknown EXIF orientation tag → identity (no rotation, no error).
    #[test]
    fn test_load_source_image_unknown_exif_orientation_defaults_to_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("odd_orientation.jpg");

        let img_4x8 = image::RgbImage::from_pixel(4, 8, image::Rgb([50u8, 60u8, 70u8]));
        let dyn_img = image::DynamicImage::ImageRgb8(img_4x8);
        let mut buf = std::io::Cursor::new(Vec::new());
        dyn_img
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        let base_bytes = buf.into_inner();

        // Orientation tag = 99 (unknown).
        let exif_header = b"Exif\x00\x00";
        let mut exif_tiff = vec![
            0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x12, 0x01, 0x03, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x63, 0x00, 0x00, 0x00, // Value=99 (unknown)
        ];
        let mut app1_data: Vec<u8> = exif_header.to_vec();
        app1_data.append(&mut exif_tiff);
        let seg_len = (app1_data.len() + 2) as u16;
        let mut patched = vec![0xFF, 0xD8, 0xFF, 0xE1];
        patched.extend_from_slice(&seg_len.to_be_bytes());
        patched.extend_from_slice(&app1_data);
        patched.extend_from_slice(&base_bytes[2..]);
        std::fs::write(&path, &patched).unwrap();

        let result = load_source_image(&path, false);
        assert!(result.is_ok(), "unknown orientation must not error");
        let img = result.unwrap();
        // No rotation applied: 4×8 stays 4×8.
        assert_eq!(img.width().get(), 4);
        assert_eq!(img.height().get(), 8);
    }

    /// Sentinel pixel survives JPEG decode (within compression tolerance).
    /// All pixels are the sentinel color to minimize DCT drift.
    #[test]
    fn test_load_source_image_sentinel_pixel_jpeg() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.jpg");
        // Uniform [180, 90, 45] image — uniform fields compress with minimal drift.
        let img = image::RgbImage::from_pixel(8, 8, image::Rgb([180u8, 90u8, 45u8]));
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        let mut buf = std::io::Cursor::new(Vec::new());
        dyn_img
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        std::fs::write(&path, buf.into_inner()).unwrap();

        let decoded = load_source_image(&path, false).unwrap();
        assert_eq!(decoded.width().get(), 8);
        assert_eq!(decoded.height().get(), 8);
        let pix = decoded.pixel_rgb(0, 0).unwrap();
        // Uniform fields compress cleanly; tolerance ±8.
        assert!((pix[0] as i32 - 180).abs() <= 8, "R={} near 180", pix[0]);
        assert!((pix[1] as i32 - 90).abs() <= 8, "G={} near 90", pix[1]);
        assert!((pix[2] as i32 - 45).abs() <= 8, "B={} near 45", pix[2]);
    }

    /// Sentinel pixel survives PNG decode (exact).
    #[test]
    fn test_load_source_image_sentinel_pixel_png() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sentinel.png");
        let mut img = image::RgbImage::from_pixel(4, 4, image::Rgb([0u8, 0u8, 0u8]));
        img.put_pixel(0, 0, image::Rgb([77u8, 88u8, 99u8]));
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        let mut buf = std::io::Cursor::new(Vec::new());
        dyn_img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        std::fs::write(&path, buf.into_inner()).unwrap();

        let decoded = load_source_image(&path, false).unwrap();
        let pix = decoded.pixel_rgb(0, 0).unwrap();
        assert_eq!(pix, [77, 88, 99], "PNG pixel must survive exactly");
    }
}
