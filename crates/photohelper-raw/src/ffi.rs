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

use photohelper_core::model::ExifOrientation;

use crate::exif::RawExifFields;
use crate::{Error, RawExifCause};

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

// `dead_code` allow applies to the WHOLE extern block — symbols added
// for the upcoming decode FFI commit (cam_mul, rgb_cam, color_maximum,
// raw_width/raw_height, filters, black, raw_image, raw_image_samples)
// have no Rust consumer yet but their declarations are stable and
// belong with the rest of the FFI surface for review locality.
// Removed in the Deliverable 1a-decode body commit.
#[allow(dead_code, reason = "TD-008")]
unsafe extern "C" {
    // === Lifecycle ===========================================
    fn libraw_init(flags: c_uint) -> *mut LibrawData;
    fn libraw_open_file(lr: *mut LibrawData, path: *const c_char) -> c_int;
    fn libraw_unpack(lr: *mut LibrawData) -> c_int;
    fn libraw_close(lr: *mut LibrawData);
    fn libraw_strerror(code: c_int) -> *const c_char;

    // === Direct accessors (typed-return; safe to call as-is) =
    fn libraw_get_iwidth(lr: *mut LibrawData) -> c_int;
    fn libraw_get_iheight(lr: *mut LibrawData) -> c_int;
    fn libraw_get_cam_mul(lr: *mut LibrawData, index: c_int) -> f32;
    fn libraw_get_rgb_cam(lr: *mut LibrawData, i: c_int, j: c_int) -> f32;
    fn libraw_get_color_maximum(lr: *mut LibrawData) -> c_int;
    fn libraw_get_raw_width(lr: *mut LibrawData) -> c_int;
    fn libraw_get_raw_height(lr: *mut LibrawData) -> c_int;

    // === photohelper C shim =================================
    fn ph_libraw_make(lr: *mut LibrawData) -> *const c_char;
    fn ph_libraw_model(lr: *mut LibrawData) -> *const c_char;
    fn ph_libraw_flip(lr: *mut LibrawData) -> i32;
    fn ph_libraw_timestamp(lr: *mut LibrawData) -> i64;
    fn ph_libraw_filters(lr: *mut LibrawData) -> u32;
    fn ph_libraw_black(lr: *mut LibrawData) -> i32;
    fn ph_libraw_raw_image(lr: *mut LibrawData) -> *const u16;
    fn ph_libraw_raw_image_samples(lr: *mut LibrawData) -> u64;
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
    // TD-008: this method becomes alive in the 1a-decode body commit
    // when `read_raw` forwards `raw_path.as_path()` into each decode
    // constructor's first arg (TD-007 closure surface).
    #[allow(dead_code, reason = "TD-008")]
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

/// Convert a non-zero LibRaw error code to a human-readable string
/// for log lines. Returns `"unknown"` on NULL.
#[allow(dead_code, reason = "TD-008-decode")]
pub(crate) fn libraw_strerror_safe(code: c_int) -> String {
    if code == 0 {
        return "ok".to_string();
    }
    // SAFETY: libraw_strerror takes an int by value; returns a pointer
    // to a static C string (no lifetime concerns).
    let ptr = unsafe { libraw_strerror(code) };
    if ptr.is_null() {
        return "unknown".to_string();
    }
    // SAFETY: ptr is non-null per the check above; libraw_strerror
    // returns a pointer to LibRaw's static string table.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .unwrap_or("invalid-utf8")
        .to_string()
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

    // === libraw_strerror_safe ===

    #[test]
    fn libraw_strerror_zero_returns_ok() {
        assert_eq!(libraw_strerror_safe(0), "ok");
    }

    #[test]
    fn libraw_strerror_nonzero_returns_descriptive() {
        // LibRaw's strerror table covers documented codes (negative ints).
        // Any specific message is upstream-defined; we just verify it's
        // a non-empty string we can render in WARN log lines.
        let msg = libraw_strerror_safe(-1);
        assert!(!msg.is_empty(), "got empty strerror");
        assert_ne!(msg, "ok");
    }
}
