//! LibRaw C-API FFI shim — the **only** module in `photohelper-raw` that
//! contains `unsafe` code. Defense-in-depth carries this contract three
//! ways: the crate-level `[lints.rust] unsafe_code = allow` is paired
//! with `#![forbid(unsafe_code)]` on every other source file, and `just
//! ci` runs an `rg`-based grep gate that fails the build if any other
//! `crates/photohelper-raw/src/*.rs` ever sprouts an `unsafe { ... }`
//! block.
//!
//! All FFI calls go through LibRaw's documented C-API accessor functions
//! plus a tiny per-photohelper C shim (`cpp/photohelper_libraw_shim.c`).
//! The shim returns small typed values (`int32_t`, `int64_t`, `const
//! char*`) over LibRaw's larger struct types so the Rust side never has
//! to mirror `libraw_iparams_t` / `libraw_image_sizes_t` /
//! `libraw_imgother_t` with `#[repr(C)]` (LibRaw layouts can shift
//! across patch releases; the C shim insulates us).
//!
//! ## Lifetime contract
//!
//! `LibrawGuard` owns the `*mut LibrawData` handle and calls
//! `libraw_close` on drop. Every shim accessor returns a value or a
//! pointer that is only valid until `libraw_close()` runs, so the
//! Rust callers in this module COPY string / buffer data out of LibRaw
//! before the guard drops. Returning a `&str` or `&[u16]` over the FFI
//! boundary would be a use-after-free.

#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString};
use std::num::NonZeroU32;
use std::os::raw::{c_char, c_int, c_uint};
use std::path::{Path, PathBuf};

use photohelper_core::model::{ExifOrientation, RgbImage};

use crate::decode::{
    BayerPlane, CamRgbToXyzD65Matrix, CfaPattern, RawImage, SensorBitDepth, SensorLevels,
    WhiteBalance,
};
use crate::exif::RawExifFields;
use crate::{Error, RawDecodeCause, RawExifCause};

/// Opaque pointer-target for LibRaw's `libraw_data_t`. We never dereference
/// it from Rust — the C shim functions do that work.
#[repr(C)]
pub(crate) struct LibrawData {
    _opaque: [u8; 0],
}

// SAFETY: LibrawData is an opaque pointer to a LibRaw-managed struct.
// LibRaw is not documented as thread-safe per-handle, but the handle
// pointer itself is safely Send between threads — we just must not
// share a single handle across threads concurrently. The Rust
// `LibrawGuard` wraps a single handle and is not Send by default
// (raw pointer), which is exactly the constraint we want.

/// Opaque pointer-target for LibRaw's `libraw_processed_image_t`.
/// Allocated by `libraw_dcraw_make_mem_image` and freed by
/// `libraw_dcraw_clear_mem`. We never dereference it from Rust — the
/// C-shim functions do that work.
#[repr(C)]
pub(crate) struct LibrawProcessedImage {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    // === Lifecycle ===========================================
    fn libraw_init(flags: c_uint) -> *mut LibrawData;
    fn libraw_open_file(lr: *mut LibrawData, path: *const c_char) -> c_int;
    fn libraw_unpack(lr: *mut LibrawData) -> c_int;
    fn libraw_close(lr: *mut LibrawData);

    // === Direct accessors (typed-return; safe to call as-is) =
    fn libraw_get_iwidth(lr: *mut LibrawData) -> c_int;
    fn libraw_get_iheight(lr: *mut LibrawData) -> c_int;
    fn libraw_get_cam_mul(lr: *mut LibrawData, index: c_int) -> f32;
    fn libraw_get_rgb_cam(lr: *mut LibrawData, i: c_int, j: c_int) -> f32;
    fn libraw_get_color_maximum(lr: *mut LibrawData) -> c_int;
    fn libraw_get_raw_width(lr: *mut LibrawData) -> c_int;
    fn libraw_get_raw_height(lr: *mut LibrawData) -> c_int;

    // === New for D1e: dcraw processing pipeline =============
    fn libraw_dcraw_process(lr: *mut LibrawData) -> c_int;
    fn libraw_dcraw_make_mem_image(
        lr: *mut LibrawData,
        errc: *mut c_int,
    ) -> *mut LibrawProcessedImage;
    fn libraw_dcraw_clear_mem(img: *mut LibrawProcessedImage);

    // === photohelper C shim (EXIF + Bayer-decode inputs) ====
    fn ph_libraw_make(lr: *mut LibrawData) -> *const c_char;
    fn ph_libraw_model(lr: *mut LibrawData) -> *const c_char;
    fn ph_libraw_flip(lr: *mut LibrawData) -> i32;
    fn ph_libraw_timestamp(lr: *mut LibrawData) -> i64;
    fn ph_libraw_filters(lr: *mut LibrawData) -> u32;
    fn ph_libraw_black(lr: *mut LibrawData) -> i32;
    fn ph_libraw_raw_image(lr: *mut LibrawData) -> *const u16;
    fn ph_libraw_raw_image_samples(lr: *mut LibrawData) -> u64;

    // === D1e: processed-image C shim (RgbImage inputs) =====
    fn ph_libraw_img_width(img: *mut LibrawProcessedImage) -> u32;
    fn ph_libraw_img_height(img: *mut LibrawProcessedImage) -> u32;
    fn ph_libraw_img_bits(img: *mut LibrawProcessedImage) -> u16;
    fn ph_libraw_img_colors(img: *mut LibrawProcessedImage) -> u16;
    fn ph_libraw_img_data_size(img: *mut LibrawProcessedImage) -> u32;
    fn ph_libraw_img_data(img: *mut LibrawProcessedImage) -> *mut u8;
}

/// RAII guard around a `libraw_init()`-allocated processor handle.
/// Calls `libraw_close()` on drop so the handle never leaks even on
/// error paths.
struct LibrawGuard {
    handle: *mut LibrawData,
    /// Captured so error-path constructors can name the offending file.
    path: PathBuf,
}

impl LibrawGuard {
    fn open(raw_path: &RawPath) -> Result<Self, Error> {
        // SAFETY: `libraw_init(0)` is documented to return a valid heap
        // pointer or NULL. No preconditions; no shared mutable state.
        let handle = unsafe { libraw_init(0) };
        if handle.is_null() {
            return Err(Error::RawExifUnavailable {
                path: raw_path.path.clone(),
                cause: RawExifCause::LibRawCallFailed {
                    libraw_code: -1,
                    op: "libraw_init",
                },
            });
        }
        Ok(Self {
            handle,
            path: raw_path.path.clone(),
        })
    }
}

impl Drop for LibrawGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: We own this handle (allocated by libraw_init in
            // ::open). LibRaw documents `libraw_close` as the matching
            // teardown; double-free is not possible because Drop runs
            // at most once per Rust struct.
            unsafe { libraw_close(self.handle) };
        }
    }
}

/// Validated path wrapper that holds both the original `PathBuf` (for
/// error messages) and a NUL-terminated C-string view (for `libraw_open_file`).
///
/// Per plan §1a § Path encoding (PR1-T20):
/// * Interior NUL byte → `Error::RawPath { reason: "interior-nul-byte" }`.
/// * Non-UTF-8 path on Unix → `Error::RawPath { reason: "non-utf8-path" }`.
/// * Empty path → `Error::RawPath { reason: "empty-path" }`.
///
/// Windows long-path (`\\?\` prefixing) lands when Windows support
/// lands in v0.2 per DN-013.
#[derive(Debug)]
pub(crate) struct RawPath {
    cstr: CString,
    path: PathBuf,
}

impl RawPath {
    /// Validate `path` for LibRaw consumption.
    ///
    /// # Errors
    ///
    /// See type-level docs for the rejection classes.
    pub(crate) fn new(path: &Path) -> Result<Self, Error> {
        let owned = path.to_path_buf();
        if owned.as_os_str().is_empty() {
            return Err(Error::RawPath {
                path: owned,
                reason: "empty-path",
            });
        }
        let path_str = owned.to_str().ok_or_else(|| Error::RawPath {
            path: owned.clone(),
            reason: "non-utf8-path",
        })?;
        let cstr = CString::new(path_str).map_err(|_| Error::RawPath {
            path: owned.clone(),
            reason: "interior-nul-byte",
        })?;
        Ok(Self { cstr, path: owned })
    }

    /// The underlying `&Path` — for error messages and the `path: &Path`
    /// argument that decode.rs constructors (TD-007 closure) take.
    pub(crate) fn as_path(&self) -> &Path {
        &self.path
    }

    fn as_c_ptr(&self) -> *const c_char {
        self.cstr.as_ptr()
    }
}

/// FFI orchestration: open the CR3, unpack metadata, extract every
/// `RawExifFields` member, close the handle. The returned struct owns
/// all of its data — no LibRaw-owned pointers leak out.
pub(crate) fn parse_libraw_fields(raw_path: &RawPath) -> Result<RawExifFields, Error> {
    let guard = LibrawGuard::open(raw_path)?;

    // SAFETY: guard.handle is valid (LibrawGuard::open returned Ok). The
    // CString from RawPath::new is NUL-terminated by CString invariant.
    let rc = unsafe { libraw_open_file(guard.handle, raw_path.as_c_ptr()) };
    if rc != 0 {
        return Err(Error::RawExifUnavailable {
            path: guard.path.clone(),
            cause: RawExifCause::LibRawCallFailed {
                libraw_code: rc,
                op: "libraw_open_file",
            },
        });
    }

    // SAFETY: handle is valid AND libraw_open_file just returned 0,
    // which LibRaw documents as the precondition for libraw_unpack.
    // (`libraw_unpack` populates the metadata structs even for the
    // metadata-only `RawExif` path; it's cheap on CR3 because the
    // EXIF box parses without decoding the pixel data.)
    let rc = unsafe { libraw_unpack(guard.handle) };
    if rc != 0 {
        return Err(Error::RawExifUnavailable {
            path: guard.path.clone(),
            cause: RawExifCause::LibRawCallFailed {
                libraw_code: rc,
                op: "libraw_unpack",
            },
        });
    }

    extract_exif_fields(&guard)
}

fn extract_exif_fields(guard: &LibrawGuard) -> Result<RawExifFields, Error> {
    // Make / Model — copied immediately so they outlive `libraw_close`.
    let make = read_cstr(
        // SAFETY: handle is valid; shim returns the iparams.make pointer
        // which is a NUL-terminated `char[64]` field inside LibRaw's
        // owned iparams struct.
        unsafe { ph_libraw_make(guard.handle) },
        "make",
        &guard.path,
    )?;
    let model = read_cstr(
        // SAFETY: same as above for `model`.
        unsafe { ph_libraw_model(guard.handle) },
        "model",
        &guard.path,
    )?;

    if make.is_empty() {
        return Err(Error::RawExifUnavailable {
            path: guard.path.clone(),
            cause: RawExifCause::ExifFieldsMissing,
        });
    }

    // Orientation — LibRaw's `flip` is a dcraw-derived enum
    // (0=Normal, 3=Rotate180, 5=Rotate90Ccw, 6=Rotate90Cw).
    // SAFETY: handle is valid; shim returns a primitive `i32`.
    let flip = unsafe { ph_libraw_flip(guard.handle) };
    let orientation =
        libraw_flip_to_exif_orientation(flip).ok_or_else(|| Error::RawExifUnavailable {
            path: guard.path.clone(),
            cause: RawExifCause::ExifMalformed {
                field: "orientation",
                raw_value: flip.to_string(),
            },
        })?;

    // Capture-time — LibRaw stores `time_t`, widened to `int64_t` by
    // the shim. A value of 0 means "unknown" (LibRaw's documented
    // sentinel for "no timestamp parsed").
    // SAFETY: handle is valid; shim returns a primitive `i64`.
    let ts = unsafe { ph_libraw_timestamp(guard.handle) };
    let capture_time_unix_seconds = if ts == 0 { None } else { Some(ts) };

    // Width / Height — direct LibRaw C-API accessors; integer return.
    // SAFETY: handle is valid; libraw_get_iwidth/iheight each take only
    // the handle and return `int` (no aliasing / no allocation).
    let (iwidth, iheight) = unsafe {
        (
            libraw_get_iwidth(guard.handle),
            libraw_get_iheight(guard.handle),
        )
    };
    let width = positive_to_nonzero_u32(iwidth, "width", &guard.path)?;
    let height = positive_to_nonzero_u32(iheight, "height", &guard.path)?;

    Ok(RawExifFields {
        make,
        model,
        orientation,
        capture_time_unix_seconds,
        width,
        height,
    })
}

/// Copy a LibRaw-owned NUL-terminated C string into an owned `String`,
/// rejecting NULL pointers + non-UTF-8 content as `ExifMalformed`.
fn read_cstr(ptr: *const c_char, field: &'static str, path: &Path) -> Result<String, Error> {
    if ptr.is_null() {
        return Err(Error::RawExifUnavailable {
            path: path.to_path_buf(),
            cause: RawExifCause::ExifMalformed {
                field,
                raw_value: "<null>".to_string(),
            },
        });
    }
    // SAFETY: `ptr` is non-null and the shim contract guarantees it
    // points to a NUL-terminated C string inside a LibRaw-owned struct
    // (`libraw_iparams_t::make` / `::model` are `char[64]` fields).
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_str()
        .map(|s| s.trim_end_matches('\0').to_string())
        .map_err(|e| Error::RawExifUnavailable {
            path: path.to_path_buf(),
            cause: RawExifCause::ExifMalformed {
                field,
                raw_value: format!("non-utf8 ({} valid bytes)", e.valid_up_to()),
            },
        })
}

fn positive_to_nonzero_u32(
    v: c_int,
    field: &'static str,
    path: &Path,
) -> Result<NonZeroU32, Error> {
    let as_u32 = u32::try_from(v).map_err(|_| Error::RawExifUnavailable {
        path: path.to_path_buf(),
        cause: RawExifCause::ExifMalformed {
            field,
            raw_value: v.to_string(),
        },
    })?;
    NonZeroU32::new(as_u32).ok_or_else(|| Error::RawExifUnavailable {
        path: path.to_path_buf(),
        cause: RawExifCause::ExifMalformed {
            field,
            raw_value: "0".to_string(),
        },
    })
}

/// Map LibRaw's `imgdata.sizes.flip` (dcraw enum) to the EXIF
/// orientation canonical 1..=8 value. Returns `None` for out-of-range
/// values so the caller can surface `ExifMalformed`.
fn libraw_flip_to_exif_orientation(flip: i32) -> Option<ExifOrientation> {
    match flip {
        0 => Some(ExifOrientation::Normal),      // EXIF 1
        3 => Some(ExifOrientation::Rotate180),   // EXIF 3
        5 => Some(ExifOrientation::Rotate90Ccw), // EXIF 8
        6 => Some(ExifOrientation::Rotate90Cw),  // EXIF 6
        _ => None,
    }
}

/// FFI orchestration for Bayer-decode: open + unpack the pixel buffer,
/// extract every `RawImage` member, close the handle. Copies the raw
/// pixel buffer out of LibRaw before the guard drops.
pub(crate) fn parse_libraw_image(raw_path: &RawPath) -> Result<RawImage, Error> {
    let guard = LibrawGuard::open(raw_path).map_err(exif_to_decode_err)?;

    // SAFETY: guard.handle is valid; CString is NUL-terminated.
    let rc = unsafe { libraw_open_file(guard.handle, raw_path.as_c_ptr()) };
    if rc != 0 {
        return Err(Error::RawDecodeFailed {
            path: guard.path.clone(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: rc,
                op: "libraw_open_file",
            },
        });
    }

    // SAFETY: open succeeded; libraw_unpack is the documented next step.
    let rc = unsafe { libraw_unpack(guard.handle) };
    if rc != 0 {
        return Err(Error::RawDecodeFailed {
            path: guard.path.clone(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: rc,
                op: "libraw_unpack",
            },
        });
    }

    extract_raw_image(&guard, raw_path.as_path())
}

fn extract_raw_image(guard: &LibrawGuard, path: &Path) -> Result<RawImage, Error> {
    // Pull raw sensor dimensions + raw_image pointer + sample count.
    // SAFETY: handle is valid; each shim/accessor returns primitives.
    let (raw_w, raw_h, raw_ptr, samples, filters, black, color_max) = unsafe {
        (
            libraw_get_raw_width(guard.handle),
            libraw_get_raw_height(guard.handle),
            ph_libraw_raw_image(guard.handle),
            ph_libraw_raw_image_samples(guard.handle),
            ph_libraw_filters(guard.handle),
            ph_libraw_black(guard.handle),
            libraw_get_color_maximum(guard.handle),
        )
    };

    let raw_w_nz = positive_to_nonzero_u32_decode(raw_w, "raw_width", path)?;
    let raw_h_nz = positive_to_nonzero_u32_decode(raw_h, "raw_height", path)?;

    if raw_ptr.is_null() {
        return Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: 0,
                op: "ph_libraw_raw_image (returned NULL — non-Bayer format?)",
            },
        });
    }
    let expected = u64::from(raw_w_nz.get()) * u64::from(raw_h_nz.get());
    if samples != expected {
        return Err(Error::RawImageDimensionMismatch {
            path: path.to_path_buf(),
            declared_pixels: expected,
            actual_pixels: samples,
        });
    }
    let samples_usize = usize::try_from(samples).map_err(|_| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::LibRawCallFailed {
            libraw_code: 0,
            op: "raw_image_samples exceeds usize",
        },
    })?;
    // SAFETY: raw_ptr is non-null per the check above; the shim returns
    // a pointer into LibRaw's owned `rawdata.raw_image` buffer whose
    // length (in u16 samples) is exactly `raw_w * raw_h`. We copy into
    // a Rust-owned Vec<u16> before guard drops to keep the data live.
    let data: Vec<u16> = unsafe { std::slice::from_raw_parts(raw_ptr, samples_usize) }.to_vec();

    let pixels = BayerPlane::new(path, data, raw_w_nz, raw_h_nz)?;

    let cfa_pattern = cfa_pattern_from_filters(filters).ok_or_else(|| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::LibRawCallFailed {
            libraw_code: 0,
            op: "cfa_pattern_from_filters (unsupported sensor — see DN-014)",
        },
    })?;

    let black_u16 = u16::try_from(black).map_err(|_| Error::RawInvalidLevels {
        path: path.to_path_buf(),
        black: 0,
        white: 0,
    })?;
    let white_u16 = u16::try_from(color_max).map_err(|_| Error::RawInvalidLevels {
        path: path.to_path_buf(),
        black: black_u16,
        white: 0,
    })?;
    let bit_depth = SensorBitDepth::new(bit_depth_from_white(white_u16))?;
    let levels = SensorLevels::new(path, black_u16, white_u16, bit_depth)?;

    // SAFETY: handle is valid; libraw_get_cam_mul returns one float per call.
    let cam_mul = unsafe {
        [
            libraw_get_cam_mul(guard.handle, 0),
            libraw_get_cam_mul(guard.handle, 1),
            libraw_get_cam_mul(guard.handle, 2),
            libraw_get_cam_mul(guard.handle, 3),
        ]
    };
    let as_shot_white_balance = WhiteBalance::from_libraw_cam_mul(path, cam_mul)?;

    let mut rgb_cam = [[0.0_f32; 3]; 3];
    for (i, row) in rgb_cam.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            // SAFETY: handle is valid; libraw_get_rgb_cam returns one
            // float per call; i and j are bounded 0..=2 by the outer
            // 3x3 array iteration.
            *cell = unsafe {
                libraw_get_rgb_cam(
                    guard.handle,
                    i32::try_from(i).unwrap_or(0),
                    i32::try_from(j).unwrap_or(0),
                )
            };
        }
    }
    let color_matrix = CamRgbToXyzD65Matrix::from_libraw_rgb_cam(path, rgb_cam)?;

    Ok(RawImage::new(
        pixels,
        cfa_pattern,
        levels,
        as_shot_white_balance,
        color_matrix,
    ))
}

/// LibrawGuard::open returns a RawExifCause-wrapped Error (because it's
/// the EXIF-default entry point). For decode flows we need a
/// RawDecodeCause-wrapped Error instead. This adapter handles only the
/// init-failure case; every other error is already correctly typed.
fn exif_to_decode_err(e: Error) -> Error {
    match e {
        Error::RawExifUnavailable {
            path,
            cause: RawExifCause::LibRawCallFailed { libraw_code, op },
        } => Error::RawDecodeFailed {
            path,
            cause: RawDecodeCause::LibRawCallFailed { libraw_code, op },
        },
        other => other,
    }
}

fn positive_to_nonzero_u32_decode(
    v: c_int,
    field: &'static str,
    path: &Path,
) -> Result<NonZeroU32, Error> {
    let as_u32 = u32::try_from(v).map_err(|_| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::LibRawCallFailed {
            libraw_code: v,
            op: field,
        },
    })?;
    NonZeroU32::new(as_u32).ok_or_else(|| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::LibRawCallFailed {
            libraw_code: 0,
            op: field,
        },
    })
}

/// Map LibRaw's `filters` bitmask to a 2x2 Bayer pattern variant. Only
/// the four standard layouts are modeled in v0.1; X-Trans / Foveon /
/// monochrome return `None` for the caller to surface as an error
/// (deferred to a non-Canon `CameraProfile` per DN-014).
///
/// Implements the LIBRAW_COLOR(filters, row, col) macro and reads the
/// 2x2 cell to discriminate the pattern. LibRaw's color codes per the
/// `cdesc` "RGBG" convention are: 0=R, 1=G(top-row), 2=B, 3=G(bottom-row).
/// Canon R8 returns `filters = 0xb4b4b4b4` → RGGB; LibRaw upstream
/// has used several legacy bit-encodings over the years for the same
/// logical pattern, so the bit-shift recipe is more robust than a
/// hardcoded constant match.
fn cfa_pattern_from_filters(filters: u32) -> Option<CfaPattern> {
    let cfa = [
        libraw_color(filters, 0, 0),
        libraw_color(filters, 0, 1),
        libraw_color(filters, 1, 0),
        libraw_color(filters, 1, 1),
    ];
    match cfa {
        [0, 1, 3, 2] => Some(CfaPattern::Rggb),
        [2, 1, 3, 0] => Some(CfaPattern::Bggr),
        [1, 0, 2, 3] => Some(CfaPattern::Grbg),
        [1, 2, 0, 3] => Some(CfaPattern::Gbrg),
        _ => None,
    }
}

/// LIBRAW_COLOR(filters, row, col) — returns the `cdesc` index for the
/// 2x2 mosaic cell at (row, col). Codes per the LibRaw "RGBG" convention.
fn libraw_color(filters: u32, row: u32, col: u32) -> u8 {
    let shift = (((row << 1) & 14) + (col & 1)) << 1;
    ((filters >> shift) & 3) as u8
}

/// FFI orchestration for AHD-demosaiced RGB output: open + unpack +
/// dcraw_process (default 8-bit sRGB) + copy the `w*h*3` byte buffer,
/// close the handle. Returns an owned [`RgbImage`] with no LibRaw-owned
/// pointers surviving.
///
/// `libraw_dcraw_clear_mem` is called after the data copy regardless of
/// whether `extract_rgb_image` returns `Ok` or `Err` — no double-free
/// and no leak on any code path.
pub(crate) fn parse_libraw_rgb_image(raw_path: &RawPath) -> Result<RgbImage, Error> {
    let guard = LibrawGuard::open(raw_path).map_err(exif_to_decode_err)?;
    let path = raw_path.as_path();

    // SAFETY: guard.handle is valid; CString is NUL-terminated.
    let rc = unsafe { libraw_open_file(guard.handle, raw_path.as_c_ptr()) };
    if rc != 0 {
        return Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: rc,
                op: "libraw_open_file",
            },
        });
    }

    // SAFETY: open succeeded; libraw_unpack is the documented next step.
    let rc = unsafe { libraw_unpack(guard.handle) };
    if rc != 0 {
        return Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: rc,
                op: "libraw_unpack",
            },
        });
    }

    // Run the default dcraw pipeline: AHD demosaic (user_qual=3) with
    // output_bps=8 (both LibRaw defaults). No params need setting.
    // SAFETY: handle is valid and libraw_unpack has returned 0.
    let rc = unsafe { libraw_dcraw_process(guard.handle) };
    if rc != 0 {
        return Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: rc,
                op: "libraw_dcraw_process",
            },
        });
    }

    // Allocate a LibRaw-managed heap buffer for the processed image.
    // errc is set to 0 on success or to a LibRaw error code on failure.
    // The returned pointer MUST be freed via libraw_dcraw_clear_mem.
    let mut errc: c_int = 0;
    // SAFETY: handle is valid and libraw_dcraw_process returned 0; errc
    // is a stack-allocated c_int whose raw pointer outlives this call;
    // the returned pointer is LibRaw-managed and valid until
    // libraw_dcraw_clear_mem is called.
    let img_ptr = unsafe { libraw_dcraw_make_mem_image(guard.handle, &raw mut errc) };
    if img_ptr.is_null() {
        return Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: errc,
                op: "libraw_dcraw_make_mem_image",
            },
        });
    }

    // Copy pixels out of the LibRaw-owned buffer; then free unconditionally.
    let result = extract_rgb_image(img_ptr, path);
    // SAFETY: img_ptr is non-null and was returned by libraw_dcraw_make_mem_image;
    // called exactly once here (after extract_rgb_image returns, success or
    // failure) so the buffer is freed without double-free or leak.
    unsafe { libraw_dcraw_clear_mem(img_ptr) };
    result
}

/// Copy pixel data out of the LibRaw-owned processed-image buffer and
/// construct an [`RgbImage`].
///
/// Does NOT call `libraw_dcraw_clear_mem` — the caller (`parse_libraw_rgb_image`)
/// is responsible for freeing `img_ptr` exactly once after this returns.
fn extract_rgb_image(img_ptr: *mut LibrawProcessedImage, path: &Path) -> Result<RgbImage, Error> {
    // Read all numeric fields from the processed image in one unsafe block.
    // SAFETY: img_ptr is non-null (caller checked); the shim functions
    // access individual primitive fields of the LibRaw-managed struct.
    let (bits, colors, width, height, data_size, data_ptr) = unsafe {
        (
            ph_libraw_img_bits(img_ptr),
            ph_libraw_img_colors(img_ptr),
            ph_libraw_img_width(img_ptr),
            ph_libraw_img_height(img_ptr),
            ph_libraw_img_data_size(img_ptr),
            ph_libraw_img_data(img_ptr),
        )
    };

    // Validate 8-bit 3-channel (sRGB) format — any other shape is unexpected.
    if bits != 8 || colors != 3 {
        return Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::RgbConversionFailed { bits, colors },
        });
    }

    if data_ptr.is_null() || data_size == 0 {
        return Err(Error::RawDecodeFailed {
            path: path.to_path_buf(),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: 0,
                op: "ph_libraw_img_data returned null or zero size",
            },
        });
    }

    let data_size_usize = usize::try_from(data_size).map_err(|_| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::LibRawCallFailed {
            libraw_code: 0,
            op: "ph_libraw_img_data_size exceeds usize",
        },
    })?;

    // SAFETY: data_ptr is non-null (checked above); data_size is the byte
    // count of the LibRaw-owned pixel buffer (w*h*3 for 8-bit RGB). We
    // copy immediately into a Vec<u8> before the caller frees img_ptr.
    let pixels: Vec<u8> = unsafe { std::slice::from_raw_parts(data_ptr, data_size_usize) }.to_vec();

    let width_nz = NonZeroU32::new(width).ok_or_else(|| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::LibRawCallFailed {
            libraw_code: 0,
            op: "ph_libraw_img_width returned zero",
        },
    })?;
    let height_nz = NonZeroU32::new(height).ok_or_else(|| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::LibRawCallFailed {
            libraw_code: 0,
            op: "ph_libraw_img_height returned zero",
        },
    })?;

    // RgbImage::new validates pixels.len() == width * height * 3.
    RgbImage::new(pixels, width_nz, height_nz).map_err(|_| Error::RawDecodeFailed {
        path: path.to_path_buf(),
        cause: RawDecodeCause::RgbConversionFailed { bits, colors },
    })
}

/// Derive bit depth from the white (saturation) level. Result is
/// the count of significant bits, e.g. 14 for `white = 16383` (Canon
/// R8). Always in `1..=16` for non-zero `white`.
fn bit_depth_from_white(white: u16) -> u8 {
    if white == 0 {
        return 0;
    }
    16 - white.leading_zeros() as u8
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    // === RawPath ===

    #[test]
    fn raw_path_accepts_canonical_ascii_path() {
        let p = Path::new("/tmp/photo.cr3");
        let rp = RawPath::new(p).expect("valid ASCII path");
        assert_eq!(rp.as_path(), p);
    }

    #[test]
    fn raw_path_rejects_empty_path() {
        let err = RawPath::new(Path::new("")).unwrap_err();
        match err {
            Error::RawPath { reason, .. } => assert_eq!(reason, "empty-path"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn raw_path_rejects_interior_nul_byte() {
        // PathBuf can hold a NUL byte on Unix; CString::new() rejects it.
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let with_nul = OsStr::from_bytes(b"/tmp/has\0nul.cr3");
        let err = RawPath::new(Path::new(with_nul)).unwrap_err();
        match err {
            Error::RawPath { reason, .. } => assert_eq!(reason, "interior-nul-byte"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn raw_path_rejects_non_utf8_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        // 0xFF is invalid UTF-8.
        let bytes = b"/tmp/\xff.cr3";
        let non_utf8 = OsStr::from_bytes(bytes);
        let err = RawPath::new(Path::new(non_utf8)).unwrap_err();
        match err {
            Error::RawPath { reason, .. } => assert_eq!(reason, "non-utf8-path"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn raw_path_accepts_unicode() {
        let p = Path::new("/tmp/photo-🦀.cr3");
        let rp = RawPath::new(p).expect("UTF-8 emoji path");
        assert_eq!(rp.as_path(), p);
    }

    // === libraw_flip_to_exif_orientation ===

    #[test]
    fn flip_zero_maps_to_normal() {
        assert_eq!(
            libraw_flip_to_exif_orientation(0),
            Some(ExifOrientation::Normal)
        );
    }

    #[test]
    fn flip_three_maps_to_rotate180() {
        assert_eq!(
            libraw_flip_to_exif_orientation(3),
            Some(ExifOrientation::Rotate180)
        );
    }

    #[test]
    fn flip_five_maps_to_rotate90_ccw() {
        assert_eq!(
            libraw_flip_to_exif_orientation(5),
            Some(ExifOrientation::Rotate90Ccw)
        );
    }

    #[test]
    fn flip_six_maps_to_rotate90_cw() {
        assert_eq!(
            libraw_flip_to_exif_orientation(6),
            Some(ExifOrientation::Rotate90Cw)
        );
    }

    #[test]
    fn flip_out_of_range_returns_none() {
        for flip in [-1, 1, 2, 4, 7, 8, 99] {
            assert_eq!(
                libraw_flip_to_exif_orientation(flip),
                None,
                "flip {flip} should be rejected"
            );
        }
    }

    // === cfa_pattern_from_filters ===
    // RGGB constant 0xb4b4b4b4 verified empirically against
    // `libraw_get_iparams(lr)->filters` on the Canon R8 fixture
    // `_MG_9625.CR3` (see ANL-001 pre-flight). The other three
    // constants derived via the LIBRAW_COLOR bit-shift recipe.

    #[test]
    fn cfa_filters_canon_r8_recognized_as_rggb() {
        assert_eq!(
            cfa_pattern_from_filters(0xb4b4_b4b4),
            Some(CfaPattern::Rggb)
        );
    }

    #[test]
    fn cfa_filters_bggr_recognized() {
        // BGGR codes: [B=2, G=1, G=3, R=0] -> filters byte = 0x36
        assert_eq!(
            cfa_pattern_from_filters(0x3636_3636),
            Some(CfaPattern::Bggr)
        );
    }

    #[test]
    fn cfa_filters_grbg_recognized() {
        // GRBG codes: [G=1, R=0, B=2, G=3] -> filters byte = 0xE1
        assert_eq!(
            cfa_pattern_from_filters(0xe1e1_e1e1),
            Some(CfaPattern::Grbg)
        );
    }

    #[test]
    fn cfa_filters_gbrg_recognized() {
        // GBRG codes: [G=1, B=2, R=0, G=3] -> filters byte = 0xC9
        assert_eq!(
            cfa_pattern_from_filters(0xc9c9_c9c9),
            Some(CfaPattern::Gbrg)
        );
    }

    #[test]
    fn libraw_color_2x2_extraction() {
        // RGGB (0xb4): expect [R=0, G=1, G=3, B=2]
        let f = 0xb4_u32;
        assert_eq!(libraw_color(f, 0, 0), 0);
        assert_eq!(libraw_color(f, 0, 1), 1);
        assert_eq!(libraw_color(f, 1, 0), 3);
        assert_eq!(libraw_color(f, 1, 1), 2);
    }

    #[test]
    fn cfa_filters_unknown_returns_none() {
        for filters in [0u32, 0xDEAD_BEEF, 0x1234_5678] {
            assert_eq!(cfa_pattern_from_filters(filters), None);
        }
    }

    // === bit_depth_from_white ===

    #[test]
    fn bit_depth_canonical_camera_values() {
        assert_eq!(bit_depth_from_white(255), 8);
        assert_eq!(bit_depth_from_white(1023), 10);
        assert_eq!(bit_depth_from_white(4095), 12);
        assert_eq!(bit_depth_from_white(16383), 14); // Canon R8
        assert_eq!(bit_depth_from_white(65535), 16);
    }

    #[test]
    fn bit_depth_partial_values() {
        // Any value with 14 high bit set is 14-bit
        assert_eq!(bit_depth_from_white(8192), 14);
        assert_eq!(bit_depth_from_white(16000), 14);
    }
}
