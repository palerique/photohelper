//! RAW decode pipeline for photohelper.
//!
//! Wraps the LibRaw C library (vendored at `=0.22.1`; see
//! `docs/adr/0002-libraw-lgpl-static-link-mechanics.md` once the
//! Deliverable 2 build-system commit authors it) to extract EXIF metadata
//! and decode Bayer-pattern sensor data from Canon CR3 files. Canon
//! R8 is the first supported body; other Canon bodies + non-Canon
//! formats land per `docs/discovery-notes.md § DN-014`.
//!
//! ## Module layout
//!
//! * [`exif`] — `RawExif` + `read_cr3` (LibRaw EXIF extraction).
//! * [`decode`] — `RawImage` + `read_raw` (LibRaw sensor decode).
//! * `ffi` (private) — the single module that may use `unsafe`. Every
//!   other module in the crate carries `#![forbid(unsafe_code)]` and a
//!   workspace-level `rg` grep gate in `just ci` catches any accidental
//!   `unsafe` outside `ffi.rs`.
//!
//! ## Error model
//!
//! Every fallible function in this crate returns [`Result<T, Error>`],
//! where [`Error`] is a `#[non_exhaustive]` enum spanning EXIF, decode,
//! dimension-validation, sensor-level-validation, path-validation, and
//! bit-depth-validation failure classes. The two boundary causes
//! ([`RawExifCause`], [`RawDecodeCause`]) discriminate sub-classes so
//! callers can route per-class (e.g. `ingest_one` matches on
//! `RawExifCause` variants to increment specific `IngestStats` counters).
//! The Error enum lives at the crate root rather than in a separate
//! `error` module because every public type in `exif` / `decode` returns
//! it; co-locating with the crate-doc keeps the failure surface one
//! `Read` away.
//!
//! The crate boundary stops Error here: the CLI converts
//! `photohelper_raw::Error` to `photohelper_core::Error::Exif { path,
//! source: Box::new(e) }` at the `parse_cr3_exif` call site so
//! `photohelper-core` stays storage-agnostic and free of any LibRaw
//! transitive dependency.

mod ffi;

pub mod decode;
pub mod exif;

use std::path::PathBuf;

/// All errors returned by `photohelper-raw`'s public API.
///
/// The variants split by failure class rather than by LibRaw call site
/// because most LibRaw failures are uninteresting noise (numeric codes
/// the operator cannot act on); the call-site detail is preserved
/// inside [`RawExifCause`] / [`RawDecodeCause`] for log-grep triage.
///
/// `#[non_exhaustive]` so adding a new failure class in a later session
/// is not a breaking change (e.g. session 04+'s develop pipeline may
/// surface new categories around demosaic / color-management failure).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// LibRaw could not extract the EXIF box (or extracted but with
    /// invalid contents). `cause` discriminates the sub-class so
    /// `ingest_one`'s dispatch table can route per-counter
    /// (see `docs/plans/session-02.md § Deliverable 4d`).
    #[error("RAW EXIF unavailable at {path}: {cause}")]
    RawExifUnavailable {
        /// Path of the CR3 (or other RAW) the FFI was called against.
        path: PathBuf,
        /// Sub-class detail for log-grep triage and `--strict` routing.
        cause: RawExifCause,
    },

    /// LibRaw could not decode the Bayer-pattern sensor data (or
    /// produced data that fails post-decode validation like NaN
    /// white-balance or identity color matrix).
    #[error("RAW image decode failed at {path}: {cause}")]
    RawDecodeFailed {
        /// Path of the CR3 (or other RAW) the FFI was called against.
        path: PathBuf,
        /// Sub-class detail for log-grep triage and `--strict` routing.
        cause: RawDecodeCause,
    },

    /// `BayerPlane::new` saw `data.len() != width * height` — the FFI
    /// either short-read the sensor buffer or LibRaw reported wrong
    /// dimensions. Both are corrupt-input signals; the operator should
    /// rerun against a freshly-pulled fixture before suspecting hardware.
    #[error(
        "RAW image dimension mismatch at {path}: declared {declared_pixels}, actual {actual_pixels}"
    )]
    RawImageDimensionMismatch {
        /// Path that produced the mismatch.
        path: PathBuf,
        /// Pixel count derived from LibRaw's reported width × height.
        declared_pixels: u64,
        /// Pixel count actually present in the buffer.
        actual_pixels: u64,
    },

    /// `SensorLevels::new` rejected the black / white levels — either
    /// inverted (`black >= white`), too narrow a dynamic range (less
    /// than 256 steps; nonsense for any sensor), or beyond the declared
    /// bit-depth's representable range.
    #[error("RAW invalid sensor levels at {path}: black={black}, white={white}")]
    RawInvalidLevels {
        /// Path that produced the invalid levels.
        path: PathBuf,
        /// LibRaw-reported black level.
        black: u16,
        /// LibRaw-reported white (saturation) level.
        white: u16,
    },

    /// `RawPath` newtype rejected the input path — interior NUL byte,
    /// non-UTF-8 path on Unix, or other LibRaw-incompatible shape.
    /// `reason` is a short human-readable diagnostic (e.g.
    /// `"interior-nul-byte"`, `"non-utf8-path"`, `"empty-path"`).
    #[error("RAW path validation failed at {path}: {reason}")]
    RawPath {
        /// Path that failed validation (best-effort `Display`).
        path: PathBuf,
        /// Short tag describing the rejection class.
        reason: &'static str,
    },

    /// `SensorBitDepth::new` saw a value outside the `8..=16` accepted
    /// range. LibRaw returns the camera-reported bit depth; out-of-range
    /// suggests a corrupt CR3 EXIF or a body whose firmware reports
    /// something photohelper does not yet model.
    #[error("RAW invalid bit depth at {path}: value={value} (expected 8..=16)")]
    RawInvalidBitDepth {
        /// The RAW file that produced the invalid bit-depth report.
        path: std::path::PathBuf,
        /// LibRaw-reported bit-depth value that fell outside `8..=16`.
        value: u8,
    },
}

/// Sub-class detail for [`Error::RawExifUnavailable`]; one variant per
/// distinguishable failure mode the operator can act on.
///
/// `#[non_exhaustive]` for forward-compat.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RawExifCause {
    /// A LibRaw C-API call returned a non-zero error code. `op` names
    /// the call (`"libraw_open_file"`, `"libraw_unpack"`, etc.) so
    /// operators can discriminate "OOM during open" from "OOM during
    /// unpack" without spelunking through the FFI source.
    #[error("LibRaw call failed (op={op}, code={libraw_code})")]
    LibRawCallFailed {
        /// LibRaw numeric error code (`< 0` per LibRaw conventions).
        libraw_code: i32,
        /// Short tag naming which LibRaw function failed.
        op: &'static str,
    },

    /// LibRaw opened the file successfully but the EXIF box itself is
    /// empty — usually a corrupt CR3 whose ISO-BMFF container is intact
    /// but whose metadata payload was truncated.
    #[error("LibRaw opened file but EXIF fields are absent (corrupt CR3)")]
    ExifFieldsMissing,

    /// LibRaw extracted Make/Model but they don't match any
    /// `CameraProfile` in the registry. `--strict` rejects;
    /// non-strict ingest writes a row with `camera_slug = NULL`.
    #[error(
        "LibRaw reports unsupported format / camera: make={libraw_make:?} model={libraw_model:?}"
    )]
    UnsupportedFormat {
        /// LibRaw-reported make string (raw — may include trailing NUL).
        libraw_make: String,
        /// LibRaw-reported model string.
        libraw_model: String,
    },

    /// A specific EXIF field is present but its raw value is malformed
    /// (e.g. orientation outside `1..=8`; non-UTF-8 bytes in
    /// `make`/`model`; zero `iwidth`/`iheight`).
    #[error("EXIF field '{field}' malformed: raw_value={raw_value:?}")]
    ExifMalformed {
        /// Short tag naming the malformed field.
        field: &'static str,
        /// Best-effort `Display` of the offending raw value.
        raw_value: String,
    },
}

/// Sub-class detail for [`Error::RawDecodeFailed`]; one variant per
/// distinguishable decode-path failure mode.
///
/// `#[non_exhaustive]` for forward-compat. Session 04+'s develop
/// pipeline may add demosaic-specific causes (see TD-006).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RawDecodeCause {
    /// A LibRaw C-API call returned a non-zero error code during decode
    /// (`libraw_unpack`, `libraw_dcraw_process`, etc.).
    #[error("LibRaw call failed (op={op}, code={libraw_code})")]
    LibRawCallFailed {
        /// LibRaw numeric error code (`< 0` per LibRaw conventions).
        libraw_code: i32,
        /// Short tag naming which LibRaw function failed.
        op: &'static str,
    },

    /// LibRaw decoded the file but the as-shot white-balance buffer
    /// (`cam_mul`) is all zeros — LibRaw signals "unloaded" / "use
    /// embedded only" this way for some camera profiles.
    #[error("White balance unloaded by LibRaw (all-zero cam_mul)")]
    WhiteBalanceUnloaded,

    /// LibRaw decoded the file but the as-shot white-balance buffer
    /// contains NaN, infinite, or negative values — physically
    /// impossible. Usually a corrupt CR3.
    #[error("White balance invalid (NaN or negative values): {values:?}")]
    WhiteBalanceInvalid {
        /// LibRaw `cam_mul` buffer as `[R, G1, B, G2]` (Canon order).
        values: [f32; 4],
    },

    /// LibRaw decoded the file but the cam→XYZ color matrix is the
    /// identity matrix — LibRaw signals "no matrix data available" this
    /// way. Without a real matrix, color management downstream is
    /// undefined.
    #[error("Color matrix unloaded by LibRaw (identity matrix)")]
    ColorMatrixUnloaded,

    /// LibRaw decoded the file but the cam→XYZ color matrix contains
    /// NaN or infinite entries. Usually a corrupt CR3.
    #[error("Color matrix invalid (NaN entries)")]
    ColorMatrixInvalid,

    /// `libraw_dcraw_make_mem_image` returned a processed image whose bit
    /// depth or channel count is not the expected 8-bit 3-channel sRGB.
    /// `bits` must be 8 and `colors` must be 3; any other values indicate
    /// LibRaw produced output in an unexpected format (16-bit output_bps,
    /// RGBA, etc.).
    #[error(
        "RGB conversion produced unexpected format: bits={bits}, colors={colors} \
         (expected bits=8, colors=3)"
    )]
    RgbConversionFailed {
        /// LibRaw-reported bits per sample in the processed image.
        bits: u16,
        /// LibRaw-reported channel count in the processed image.
        colors: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_exif_unavailable_display_includes_path_and_cause() {
        let err = Error::RawExifUnavailable {
            path: PathBuf::from("/tmp/photo.cr3"),
            cause: RawExifCause::ExifFieldsMissing,
        };
        let rendered = err.to_string();
        assert!(
            rendered.contains("/tmp/photo.cr3"),
            "path not in display: {rendered}"
        );
        assert!(
            rendered.contains("EXIF fields are absent"),
            "cause not in display: {rendered}"
        );
    }

    #[test]
    fn raw_decode_failed_display_includes_libraw_code_and_op() {
        let err = Error::RawDecodeFailed {
            path: PathBuf::from("/tmp/photo.cr3"),
            cause: RawDecodeCause::LibRawCallFailed {
                libraw_code: -7,
                op: "libraw_unpack",
            },
        };
        let rendered = err.to_string();
        assert!(
            rendered.contains("libraw_unpack"),
            "op not in display: {rendered}"
        );
        assert!(rendered.contains("-7"), "code not in display: {rendered}");
    }

    #[test]
    fn raw_invalid_bit_depth_display_names_value() {
        let err = Error::RawInvalidBitDepth {
            path: std::path::PathBuf::from("/test.cr3"),
            value: 7,
        };
        let rendered = err.to_string();
        assert!(
            rendered.contains("value=7"),
            "value not in display: {rendered}"
        );
        assert!(
            rendered.contains("8..=16"),
            "range hint not in display: {rendered}"
        );
    }

    #[test]
    fn raw_image_dimension_mismatch_display_includes_counts() {
        let err = Error::RawImageDimensionMismatch {
            path: PathBuf::from("/tmp/photo.cr3"),
            declared_pixels: 24_000_000,
            actual_pixels: 23_999_999,
        };
        let rendered = err.to_string();
        assert!(
            rendered.contains("24000000"),
            "declared not in display: {rendered}"
        );
        assert!(
            rendered.contains("23999999"),
            "actual not in display: {rendered}"
        );
    }

    #[test]
    fn white_balance_invalid_display_includes_values() {
        let err = Error::RawDecodeFailed {
            path: PathBuf::from("/tmp/photo.cr3"),
            cause: RawDecodeCause::WhiteBalanceInvalid {
                values: [f32::NAN, 1.0, 1.0, 1.0],
            },
        };
        let rendered = err.to_string();
        assert!(rendered.contains("NaN"), "NaN not in display: {rendered}");
    }

    #[test]
    fn error_implements_std_error_and_send_sync() {
        fn assert_traits<T: std::error::Error + Send + Sync + 'static>() {}
        assert_traits::<Error>();
        assert_traits::<RawExifCause>();
        assert_traits::<RawDecodeCause>();
    }
}
