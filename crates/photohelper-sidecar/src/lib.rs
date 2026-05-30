//! XMP sidecar reader/writer for photohelper.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//!
//! Produces Lightroom-compatible `.xmp` sidecars (extension-replaced, not
//! appended: `photo.CR3` → `photo.xmp`). Supports `crs:` (Camera Raw) and
//! `ph:` (photohelper) namespaces.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use photohelper_sidecar::SidecarSettings;
//! use photohelper_sidecar::conflict::{merge_and_write, WriteOutcome};
//! use std::path::Path;
//!
//! # fn main() -> Result<(), photohelper_sidecar::Error> {
//! let raw_path = Path::new("/photos/IMG_0001.CR3");
//! let sidecar_path = raw_path.with_extension("xmp");
//!
//! let settings = SidecarSettings::builder()
//!     .exposure(0.5)
//!     .temperature(5500)
//!     .nima_score(7.25)
//!     .build()?;
//!
//! let outcome = merge_and_write(&sidecar_path, &settings, false)?;
//! match outcome {
//!     WriteOutcome::Created => println!("sidecar created"),
//!     WriteOutcome::Overwritten => println!("sidecar updated"),
//!     WriteOutcome::ConflictPreserved => println!("existing edits preserved"),
//!     WriteOutcome::ForcedOverwrite => println!("sidecar force-overwritten"),
//!     _ => {}
//! }
//! # Ok(())
//! # }
//! ```

pub mod conflict;
mod error;
mod reader;
mod settings;
mod writer;

pub use conflict::{WriteOutcome, merge_and_write};
pub use error::Error;
pub use reader::read_xmp;
pub use settings::{SidecarSettings, SidecarSettingsBuilder};
pub use writer::write_xmp;

use static_assertions::assert_impl_all;
assert_impl_all!(SidecarSettings: Send, Sync);

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;
    use time::OffsetDateTime;

    fn now() -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }

    fn past() -> OffsetDateTime {
        now() - time::Duration::hours(1)
    }

    fn future() -> OffsetDateTime {
        now() + time::Duration::hours(1)
    }

    // ── SidecarSettings builder validation ───────────────────────────────

    #[test]
    fn temperature_out_of_range_rejected() {
        let err = SidecarSettings::builder()
            .temperature(60_000)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Validation { .. }));
    }

    #[test]
    fn exposure_out_of_range_rejected() {
        let err = SidecarSettings::builder()
            .exposure(10.0)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Validation { .. }));
    }

    #[test]
    fn tint_out_of_range_rejected() {
        let err = SidecarSettings::builder().tint(200).build().unwrap_err();
        assert!(matches!(err, Error::Validation { .. }));
    }

    #[test]
    fn int_crs_field_boundary_rejected() {
        let err = SidecarSettings::builder()
            .contrast(101)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Validation { .. }));
        let err2 = SidecarSettings::builder()
            .highlights(-101)
            .build()
            .unwrap_err();
        assert!(matches!(err2, Error::Validation { .. }));
    }

    #[test]
    fn valid_settings_build_succeeds() {
        let s = SidecarSettings::builder()
            .temperature(5500)
            .tint(-10)
            .exposure(0.5)
            .contrast(20)
            .highlights(-30)
            .shadows(40)
            .nima_score(7.25)
            .dedup_cluster_id(3)
            .photohelper_id("abc123")
            .last_processed_at(now())
            .build()
            .expect("valid settings must build");
        assert_eq!(s.temperature(), Some(5500));
        assert_eq!(s.nima_score(), Some(7.25));
    }

    // ── Sidecar path convention ───────────────────────────────────────────

    #[test]
    fn sidecar_path_for_cr3_replaces_extension() {
        let raw = Path::new("/photos/IMG_0001.CR3");
        let sidecar = raw.with_extension("xmp");
        assert_eq!(sidecar, Path::new("/photos/IMG_0001.xmp"));
    }

    // ── Write → read round-trip ───────────────────────────────────────────

    #[test]
    fn write_and_read_roundtrip_all_fields() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        let dt = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();

        let original = SidecarSettings::builder()
            .temperature(5500)
            .tint(-10)
            .exposure(0.5)
            .contrast(20)
            .highlights(-30)
            .shadows(40)
            .nima_score(7.25)
            .dedup_cluster_id(3)
            .photohelper_id("abc123def456ghi789jkl012mno345pqr678stu90")
            .last_processed_at(dt)
            .build()
            .unwrap();

        write_xmp(&p, &original).expect("write must succeed");
        let read_back = read_xmp(&p).expect("read must succeed");

        assert_eq!(read_back.temperature(), original.temperature());
        assert_eq!(read_back.tint(), original.tint());
        assert!((read_back.exposure().unwrap() - 0.5).abs() < 0.01);
        assert_eq!(read_back.contrast(), original.contrast());
        assert_eq!(read_back.highlights(), original.highlights());
        assert_eq!(read_back.shadows(), original.shadows());
        assert!((read_back.nima_score().unwrap() - 7.25).abs() < 0.001);
        assert_eq!(read_back.dedup_cluster_id(), Some(3));
        assert_eq!(
            read_back.photohelper_id(),
            Some("abc123def456ghi789jkl012mno345pqr678stu90")
        );
        // Timestamps round-trip through Rfc3339 — compare at second precision.
        let ts = read_back.last_processed_at().unwrap().unix_timestamp();
        assert_eq!(ts, 1_700_000_000);
    }

    #[test]
    fn write_with_only_ph_namespace() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        let s = SidecarSettings::builder().nima_score(5.5).build().unwrap();
        write_xmp(&p, &s).unwrap();
        let xml = std::fs::read_to_string(&p).unwrap();
        assert!(xml.contains("ph:NimaScore"));
        assert!(!xml.contains("crs:Temperature"));
        assert!(!xml.contains("crs:ProcessVersion"));
    }

    #[test]
    fn write_with_only_crs_namespace() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        let s = SidecarSettings::builder().exposure(1.0).build().unwrap();
        write_xmp(&p, &s).unwrap();
        let xml = std::fs::read_to_string(&p).unwrap();
        assert!(xml.contains("crs:Exposure2012"));
        assert!(xml.contains("crs:ProcessVersion=\"11.0\""));
        assert!(!xml.contains("ph:NimaScore"));
    }

    #[test]
    fn lightroom_compatible_output() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        let s = SidecarSettings::builder()
            .temperature(5000)
            .build()
            .unwrap();
        write_xmp(&p, &s).unwrap();
        let xml = std::fs::read_to_string(&p).unwrap();
        assert!(xml.contains("xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\""));
        assert!(xml.contains("xmlns:ph=\"http://ns.photohelper.dev/1.0/\""));
        assert!(xml.contains("xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\""));
        assert!(xml.contains("xmlns:x=\"adobe:ns:meta/\""));
    }

    // ── XMP reader ────────────────────────────────────────────────────────

    #[test]
    fn read_unknown_fields_ignored() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      crs:Temperature="5500"
      crs:UnknownFutureField="42"
      xyz:SomethingElse="hello"
    />
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse");
        assert_eq!(s.temperature(), Some(5500));
    }

    #[test]
    fn read_malformed_temperature_warns_and_returns_none() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      crs:Temperature="not-a-number"
      crs:Tint="-10"
    />
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse (lenient)");
        assert_eq!(s.temperature(), None, "malformed temp must be None");
        assert_eq!(s.tint(), Some(-10), "valid tint must still be read");
    }

    #[test]
    fn read_malformed_xml_returns_parse_error() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bad.xmp");
        std::fs::write(&p, b"<this></that>").unwrap();
        let result = read_xmp(&p);
        assert!(result.is_err(), "malformed XML must return Err");
    }

    #[test]
    fn read_minimal_xmp() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" />
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("minimal must parse");
        assert!(s.is_empty());
    }

    // ── Conflict resolution ───────────────────────────────────────────────

    #[test]
    fn conflict_preserve_newer_lightroom_edit() {
        // Simulates: photohelper wrote ph:LastProcessedAt=past, then Lightroom
        // edited and updated xmp:MetadataDate to future (but left ph:LastProcessedAt).
        // Correct detection: xmp:MetadataDate(future) > ph:LastProcessedAt(past) → ConflictPreserved.
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");

        // Write an existing sidecar with our timestamps (past write).
        let existing = SidecarSettings::builder()
            .exposure(1.0)
            .last_processed_at(past()) // ph:LastProcessedAt = past (our write time)
            .build()
            .unwrap();
        write_xmp(&p, &existing).unwrap();

        // Simulate Lightroom editing: update xmp:MetadataDate to a future value
        // while leaving ph:LastProcessedAt at the past value.
        // Replace the existing xmp:MetadataDate line with a future date.
        let raw = std::fs::read_to_string(&p).unwrap();
        // Use a partial string match that doesn't depend on the exact timestamp.
        let lightroom_edited = {
            // Find and replace xmp:MetadataDate="..." with the future date.
            let start = raw
                .find("xmp:MetadataDate=\"")
                .expect("xmp:MetadataDate in sidecar");
            let end = raw[start..]
                .find('"') // opening "
                .map(|i| start + i + 1)
                .and_then(|j| raw[j..].find('"').map(|k| j + k + 1))
                .expect("closing quote for xmp:MetadataDate");
            format!(
                "{}xmp:MetadataDate=\"2099-01-01T00:00:00Z\"{}",
                &raw[..start],
                &raw[end..]
            )
        };
        std::fs::write(&p, lightroom_edited).unwrap();

        let incoming = SidecarSettings::builder()
            .exposure(0.0)
            .last_processed_at(now())
            .build()
            .unwrap();
        let outcome = merge_and_write(&p, &incoming, false).unwrap();
        assert_eq!(outcome, WriteOutcome::ConflictPreserved);
        // Existing value preserved.
        let check = read_xmp(&p).unwrap();
        assert!((check.exposure().unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn conflict_overwrite_older_lightroom_edit() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");

        let existing = SidecarSettings::builder()
            .exposure(1.0)
            .last_processed_at(past())
            .build()
            .unwrap();
        write_xmp(&p, &existing).unwrap();

        let incoming = SidecarSettings::builder()
            .exposure(0.5)
            .last_processed_at(now())
            .build()
            .unwrap();
        let outcome = merge_and_write(&p, &incoming, false).unwrap();
        assert_eq!(outcome, WriteOutcome::Overwritten);
        let check = read_xmp(&p).unwrap();
        assert!((check.exposure().unwrap() - 0.5).abs() < 0.01);
    }

    #[test]
    fn conflict_force_overwrite() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");

        let existing = SidecarSettings::builder()
            .exposure(1.0)
            .last_processed_at(future())
            .build()
            .unwrap();
        write_xmp(&p, &existing).unwrap();

        let incoming = SidecarSettings::builder()
            .exposure(0.0)
            .last_processed_at(past())
            .build()
            .unwrap();
        let outcome = merge_and_write(&p, &incoming, true).unwrap();
        assert_eq!(outcome, WriteOutcome::ForcedOverwrite);
        let check = read_xmp(&p).unwrap();
        assert!((check.exposure().unwrap() - 0.0).abs() < 0.01);
    }

    #[test]
    fn conflict_missing_metadata_date_preserves() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        // Write existing sidecar with no timestamp but with crs: fields.
        let existing = SidecarSettings::builder().exposure(1.0).build().unwrap();
        write_xmp(&p, &existing).unwrap();

        let incoming = SidecarSettings::builder()
            .exposure(0.0)
            .last_processed_at(now())
            .build()
            .unwrap();
        // Existing has no timestamp (MetadataDate absent); conservative: preserve.
        let outcome = merge_and_write(&p, &incoming, false).unwrap();
        assert_eq!(outcome, WriteOutcome::ConflictPreserved);
    }

    #[test]
    fn conflict_missing_last_processed_preserves() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        // Existing has a MetadataDate (simulating Lightroom-written sidecar).
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/"
      xmp:MetadataDate="2026-01-01T12:00:00Z"
      crs:Temperature="6000"
    />
  </rdf:RDF>
</x:xmpmeta>"#;
        std::fs::write(&p, xml).unwrap();

        // Incoming has no ph:LastProcessedAt → ConflictPreserved.
        let incoming = SidecarSettings::builder().exposure(0.0).build().unwrap();
        let outcome = merge_and_write(&p, &incoming, false).unwrap();
        assert_eq!(outcome, WriteOutcome::ConflictPreserved);
    }

    // ── Atomic write ──────────────────────────────────────────────────────

    #[test]
    #[cfg(unix)]
    fn write_xmp_to_readonly_dir_returns_io_error() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempdir().unwrap();
        let readonly_dir = dir.path().join("ro");
        std::fs::create_dir(&readonly_dir).unwrap();
        std::fs::set_permissions(&readonly_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let p = readonly_dir.join("photo.xmp");
        let s = SidecarSettings::builder().exposure(1.0).build().unwrap();
        let result = write_xmp(&p, &s);
        assert!(result.is_err(), "write to read-only dir must fail");
        // No .phdev.tmp left.
        let tmp = readonly_dir.join("photo.phdev.tmp");
        assert!(!tmp.exists(), ".phdev.tmp must not persist after failure");
    }

    #[test]
    fn write_xmp_atomic_no_partial_on_io_error() {
        // Use a path in a non-existent directory — will fail at File::create.
        let p = Path::new("/nonexistent/path/photo.xmp");
        let s = SidecarSettings::builder().exposure(1.0).build().unwrap();
        let result = write_xmp(p, &s);
        assert!(result.is_err(), "write to nonexistent dir must fail");
    }
}
