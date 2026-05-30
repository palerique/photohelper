//! Integration tests against the committed Git LFS CR3 fixtures.
//!
//! Plan §Deliverable 3 + § Acceptance 2a: these tests exercise the FFI
//! end-to-end on real Canon EOS R8 sensor data, NOT synthetic stubs.
//! If the developer hasn't run `git lfs install && git lfs pull`, the
//! `fixture_is_real_cr3` helper panics with an actionable message
//! instead of silently passing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{fixture_is_real_cr3, fixture_path};
use photohelper_core::model::ExifOrientation;
use photohelper_raw::decode::{CfaPattern, read_raw, read_raw_rgb};
use photohelper_raw::exif::read_cr3;

// ── Error-path tests (no LFS fixtures required) ────────────────────────────
// These verify that the public entry points return Err for invalid inputs
// rather than panicking or silently succeeding.

#[test]
fn read_cr3_returns_error_for_nonexistent_file() {
    let p = std::path::Path::new("/nonexistent/path/that/does/not/exist.cr3");
    let result = read_cr3(p);
    assert!(
        result.is_err(),
        "read_cr3 on a nonexistent path must return Err, got Ok"
    );
}

#[test]
fn read_cr3_returns_error_for_non_raw_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = dir.path().join("not_a_cr3.cr3");
    std::fs::write(&fake, b"This is not a CR3 file. Just text.").expect("write fake");
    let result = read_cr3(&fake);
    assert!(
        result.is_err(),
        "read_cr3 on a non-RAW file must return Err, got Ok"
    );
}

#[test]
fn read_raw_returns_error_for_non_raw_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = dir.path().join("not_a_cr3.cr3");
    std::fs::write(&fake, b"This is not a CR3 RAW file.").expect("write fake");
    let result = read_raw(&fake);
    assert!(
        result.is_err(),
        "read_raw on a non-RAW file must return Err, got Ok"
    );
}

#[test]
fn read_cr3_extracts_canon_r8_metadata_from_craw_fixture() {
    let p = fixture_is_real_cr3(&fixture_path("CRAW_FULL_FRAME.CR3"));
    let exif = read_cr3(&p).expect("LibRaw must parse a CC0 R8 fixture cleanly");
    assert_eq!(exif.make(), "Canon");
    // LibRaw normalizes the Model differently across CR3 fixtures —
    // some return "Canon EOS R8" (the EXIF tag's full value), others
    // return "EOS R8" (LibRaw's normalized form). Both are valid R8
    // identifications. The CameraRegistry::for_exif normalization in
    // photohelper-cameras handles both shapes.
    assert!(
        exif.model().contains("EOS R8"),
        "got model {:?}",
        exif.model()
    );
    assert_eq!(exif.orientation(), ExifOrientation::Normal);
    assert!(
        exif.capture_time_unix_seconds().is_some(),
        "CC0 R8 fixture has DateTimeOriginal"
    );
    // Sanitized fixture preserves Make / Model / Orientation /
    // CaptureTime / Width / Height. The width/height fields come from
    // LibRaw's iwidth/iheight (post-rotation visible area).
    assert!(exif.width().get() >= 6000, "got width {}", exif.width());
    assert!(exif.height().get() >= 4000, "got height {}", exif.height());
}

#[test]
fn read_cr3_extracts_canon_r8_metadata_from_raw_fixture() {
    let p = fixture_is_real_cr3(&fixture_path("RAW_FULL_FRAME.CR3"));
    let exif = read_cr3(&p).expect("LibRaw must parse a CC0 R8 fixture cleanly");
    assert_eq!(exif.make(), "Canon");
    assert!(
        exif.model().contains("EOS R8"),
        "got model {:?}",
        exif.model()
    );
}

#[test]
fn read_raw_decodes_canon_r8_bayer_plane_from_raw_fixture() {
    let p = fixture_is_real_cr3(&fixture_path("RAW_FULL_FRAME.CR3"));
    let img = read_raw(&p).expect("LibRaw must decode the RAW fixture's Bayer plane");
    let pixels = img.pixels();
    // R8 RAW raw_width/height are ~6188×4120 (visible 6022×4024 plus
    // the 84×48 masked-pixel border).
    assert!(pixels.width().get() >= 6000);
    assert!(pixels.height().get() >= 4000);
    // CFA pattern must be RGGB on Canon R8.
    assert_eq!(img.cfa_pattern(), CfaPattern::Rggb);
    // Sensor levels: black ~ 2047 (14-bit Canon convention),
    // white = 16383 (2^14 - 1), bit_depth = 14.
    let levels = img.levels();
    assert!(
        levels.black() > 0 && levels.black() < 5000,
        "got black {}",
        levels.black()
    );
    assert_eq!(levels.white(), 16383);
    assert_eq!(levels.bit_depth().get(), 14);
    // White balance: cam_mul must have positive R / G1 / B / G2 values.
    let wb = img.as_shot_white_balance();
    assert!(wb.r() > 0.0);
    assert!(wb.g1() > 0.0);
    assert!(wb.b() > 0.0);
    assert!(wb.g2() > 0.0);
    // Color matrix must NOT be identity (LibRaw signals "unloaded" via
    // identity; we'd have errored if it had returned that).
    let cm = img.color_matrix();
    let m = cm.as_array();
    let is_identity = (m[0][0] - 1.0).abs() < 1e-6
        && (m[1][1] - 1.0).abs() < 1e-6
        && (m[2][2] - 1.0).abs() < 1e-6;
    assert!(!is_identity, "color matrix should not be identity");
}

/// Compute mean and population standard deviation of an 8-bit pixel buffer.
fn mean_and_stddev(pixels: &[u8]) -> (f64, f64) {
    let n = pixels.len() as f64;
    let mean = pixels.iter().map(|&b| f64::from(b)).sum::<f64>() / n;
    let variance = pixels
        .iter()
        .map(|&b| {
            let d = f64::from(b) - mean;
            d * d
        })
        .sum::<f64>()
        / n;
    (mean, variance.sqrt())
}

/// D1e integration test — per plan §Deliverable D1e § Integration test.
///
/// Checks two invariants for both CC0 Canon R8 fixtures:
/// 1. Dimension invariant: `pixels.len() == w * h * 3`
/// 2. Content plausibility: `mean ∈ (20, 240)` AND `std_dev > 5`
///    (rules out all-zeros / all-max degenerate output and silent LibRaw
///    copy bugs per plan PR1-T16)
#[test]
fn read_raw_rgb_cc0_fixture() {
    for name in &["CRAW_FULL_FRAME.CR3", "RAW_FULL_FRAME.CR3"] {
        let p = fixture_is_real_cr3(&fixture_path(name));
        let img =
            read_raw_rgb(&p).unwrap_or_else(|e| panic!("read_raw_rgb failed for {name}: {e}"));

        let w = img.width().get() as usize;
        let h = img.height().get() as usize;

        // Dimension invariant: buffer must be exactly w×h×3 bytes.
        assert_eq!(
            img.pixels().len(),
            w * h * 3,
            "fixture={name} dimension invariant failed (w={w}, h={h})"
        );

        // Plausibility: mean and std_dev rule out degenerate all-zero /
        // all-saturated / static output from a broken LibRaw integration.
        let (mean, stddev) = mean_and_stddev(img.pixels());
        assert!(
            mean > 20.0 && mean < 240.0,
            "fixture={name} mean={mean:.2} not in (20, 240)"
        );
        assert!(stddev > 5.0, "fixture={name} stddev={stddev:.2} not > 5.0");
    }
}
