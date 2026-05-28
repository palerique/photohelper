//! `RawExif` — typed EXIF extract from LibRaw.
//!
//! Six fields the catalog actually persists: Make, Model, Orientation,
//! CaptureTime, Width, Height. Private fields + accessor methods per
//! the strong-type discipline locked at plan-review (PR1-T5 + R2-T5 +
//! R2-T6). External code can hold and inspect a `RawExif` via the
//! accessors but cannot construct one — the FFI module is the sole
//! authoritative source.
//!
//! The validating constructor `RawExif::from_libraw_fields` and the
//! crate-private FFI-crossing builder `RawExifFields` land together
//! with the FFI body (Deliverable 1a). Splitting now would force a
//! `#[allow(dead_code)]` placeholder on items that will be used in the
//! very next commit; co-locating constructor + consumer keeps the
//! diff readable.

#![forbid(unsafe_code)]

use std::num::NonZeroU32;

use photohelper_core::model::ExifOrientation;
use static_assertions::assert_impl_all;

/// Typed EXIF extract for one RAW file.
///
/// External callers obtain `RawExif` via `read_cr3` (added in the FFI
/// body commit) and use the accessor methods to read the individual
/// fields. The struct is exported (`pub`) but cannot be constructed
/// from outside this module — all fields are private and no public
/// constructor exists yet. Once 1a body lands, `RawExif::from_libraw_fields`
/// (crate-private) becomes the sole producer, called only by the FFI
/// module.
///
/// `Send + Sync` is asserted at module scope below so a future field
/// addition that breaks thread-safety fails the build immediately
/// rather than at the first downstream consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawExif {
    make: String,
    model: String,
    orientation: ExifOrientation,
    capture_time_unix_seconds: Option<i64>,
    width: NonZeroU32,
    height: NonZeroU32,
}

assert_impl_all!(RawExif: Send, Sync);

impl RawExif {
    /// Camera make, as reported by LibRaw (e.g. `"Canon"`).
    ///
    /// LibRaw trims trailing NUL bytes from the underlying
    /// `libraw_iparams_t::make` field at the FFI boundary so this
    /// returns a well-formed UTF-8 string without padding.
    pub fn make(&self) -> &str {
        &self.make
    }

    /// Camera model, as reported by LibRaw (e.g. `"EOS R8"`).
    ///
    /// LibRaw normalizes the model name across firmware revisions; the
    /// FFI boundary trims trailing NUL bytes for the same reason as
    /// [`Self::make`].
    pub fn model(&self) -> &str {
        &self.model
    }

    /// EXIF orientation (canonical 1..=8 tag) derived from LibRaw's
    /// `imgdata.sizes.flip` post-rotation value.
    ///
    /// The FFI boundary maps LibRaw's `flip` to the matching
    /// [`ExifOrientation`] variant; out-of-range flip values produce
    /// [`crate::Error::RawExifUnavailable`] with cause
    /// [`crate::RawExifCause::ExifMalformed { field: "orientation" }`].
    pub fn orientation(&self) -> ExifOrientation {
        self.orientation
    }

    /// Capture time as Unix seconds (UTC).
    ///
    /// **UTC assumption**: LibRaw's `imgdata.other.timestamp` is `time_t`
    /// interpreted as wall-clock UTC absent EXIF timezone metadata. CR3
    /// EXIF's `DateTimeOriginal` is naïve wall-clock; the UTC assumption
    /// is the safest default for chronological sorting. DN-016 tracks
    /// per-EXIF-tag timezone recovery for v0.2's develop pipeline.
    ///
    /// `None` when LibRaw could not extract the timestamp (older
    /// firmware revisions or sanitized fixtures).
    pub fn capture_time_unix_seconds(&self) -> Option<i64> {
        self.capture_time_unix_seconds
    }

    /// Post-rotation visible-area width in pixels, sourced from
    /// `libraw_get_iwidth()`. Guaranteed non-zero by the `NonZeroU32`
    /// type — the FFI boundary returns
    /// [`crate::Error::RawExifUnavailable`] with cause
    /// [`crate::RawExifCause::ExifMalformed { field: "width" }`] for
    /// zero values.
    pub fn width(&self) -> NonZeroU32 {
        self.width
    }

    /// Post-rotation visible-area height in pixels, sourced from
    /// `libraw_get_iheight()`. Same non-zero invariant as
    /// [`Self::width`].
    pub fn height(&self) -> NonZeroU32 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// Builds a canonical Canon R8 `RawExif` via same-module access to
    /// the private fields. The FFI-driven constructor (`from_libraw_fields`)
    /// lands in the 1a body commit; until then, tests bypass it with a
    /// direct struct literal — legal here because the test module shares
    /// scope with the type definition.
    ///
    /// Values match the ANL-001 pre-flight spot-check on
    /// `/Users/ph/Pictures/tests/_MG_9625.CR3`.
    fn r8_fixture() -> RawExif {
        RawExif {
            make: "Canon".to_string(),
            model: "EOS R8".to_string(),
            orientation: ExifOrientation::Normal,
            capture_time_unix_seconds: Some(1_741_323_714),
            width: NonZeroU32::new(6022).expect("6022 is non-zero"),
            height: NonZeroU32::new(4024).expect("4024 is non-zero"),
        }
    }

    #[test]
    fn accessors_return_constructor_values() {
        let exif = r8_fixture();
        assert_eq!(exif.make(), "Canon");
        assert_eq!(exif.model(), "EOS R8");
        assert_eq!(exif.orientation(), ExifOrientation::Normal);
        assert_eq!(exif.capture_time_unix_seconds(), Some(1_741_323_714));
        assert_eq!(exif.width().get(), 6022);
        assert_eq!(exif.height().get(), 4024);
    }

    #[test]
    fn rotated_orientation_preserved() {
        let exif = RawExif {
            orientation: ExifOrientation::Rotate90Cw,
            ..r8_fixture()
        };
        assert_eq!(exif.orientation(), ExifOrientation::Rotate90Cw);
    }

    #[test]
    fn none_capture_time_preserved() {
        let exif = RawExif {
            capture_time_unix_seconds: None,
            ..r8_fixture()
        };
        assert_eq!(exif.capture_time_unix_seconds(), None);
    }

    #[test]
    fn clone_eq_roundtrip() {
        let original = r8_fixture();
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn debug_repr_includes_make_and_model() {
        let exif = r8_fixture();
        let dbg = format!("{exif:?}");
        assert!(dbg.contains("Canon"), "make missing from Debug: {dbg}");
        assert!(dbg.contains("EOS R8"), "model missing from Debug: {dbg}");
    }
}
