//! XMP sidecar reader/writer for photohelper.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//!
//! Produces Lightroom-compatible `.xmp` sidecars (extension-replaced, not
//! appended: `photo.CR3` → `photo.xmp`). Supports standard namespaces `dc:`
//! (Dublin Core), `lr:` (Lightroom), and `xmp:`, in addition to `crs:`
//! (Camera Raw) and `ph:` (photohelper) namespaces.
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
pub use settings::{Rating, SidecarSettings, SidecarSettingsBuilder};
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

    #[test]
    fn test_parse_nested_elements_for_standard_fields() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="">
      <xmp:Rating>3</xmp:Rating>
      <xmp:Label>Green</xmp:Label>
      <crs:Temperature>5500</crs:Temperature>
      <crs:Tint>10</crs:Tint>
      <crs:Exposure2012>-0.5</crs:Exposure2012>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#;
        let s =
            reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse nested elements");
        assert_eq!(s.rating(), Some(Rating::Three));
        assert_eq!(s.label(), Some("Green"));
        assert_eq!(s.temperature(), Some(5500));
        assert_eq!(s.tint(), Some(10));
        assert_eq!(s.exposure(), Some(-0.5));
    }

    #[test]
    fn test_parse_nested_elements_metadata_and_last_processed() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="">
      <xmp:MetadataDate>2026-05-30T12:00:00Z</xmp:MetadataDate>
      <ph:LastProcessedAt>2026-05-30T15:00:00Z</ph:LastProcessedAt>
      <ph:NimaScore>8.5</ph:NimaScore>
      <ph:DedupClusterId>42</ph:DedupClusterId>
      <ph:PhotohelperId>test-id-999</ph:PhotohelperId>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp"))
            .expect("must parse nested photohelper elements");
        assert!(s.metadata_date().is_some());
        assert!(s.last_processed_at().is_some());
        assert_eq!(s.nima_score(), Some(8.5));
        assert_eq!(s.dedup_cluster_id(), Some(42));
        assert_eq!(s.photohelper_id(), Some("test-id-999"));
    }

    #[test]
    fn test_parse_nested_elements_lenient_malformed() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="">
      <crs:Temperature>not-an-int</crs:Temperature>
      <crs:Exposure2012>not-a-float</crs:Exposure2012>
      <ph:NimaScore>not-a-finite-score</ph:NimaScore>
      <ph:DedupClusterId>-123</ph:DedupClusterId>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp"))
            .expect("must tolerate malformed nested elements");
        assert_eq!(s.temperature(), None);
        assert_eq!(s.exposure(), None);
        assert_eq!(s.nima_score(), None);
        assert_eq!(s.dedup_cluster_id(), None);
    }

    #[test]
    fn test_parse_nested_elements_case_insensitive_namespaces() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:XMPMETA xmlns:x="adobe:ns:meta/">
  <RDF:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <RDF:DESCRIPTION rdf:about="">
      <XMP:Rating>4</XMP:Rating>
      <XMP:Label>Blue</XMP:Label>
      <CRS:Temperature>6000</CRS:Temperature>
      <PH:NimaScore>9.1</PH:NimaScore>
    </RDF:DESCRIPTION>
  </RDF:RDF>
</x:XMPMETA>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp"))
            .expect("must parse case-insensitive namespaces");
        assert_eq!(s.rating(), Some(Rating::Four));
        assert_eq!(s.label(), Some("Blue"));
        assert_eq!(s.temperature(), Some(6000));
        assert_eq!(s.nima_score(), Some(9.1));
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
    fn conflict_missing_last_processed_merges() {
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

        // Incoming has no ph:LastProcessedAt → Overwritten (merged on first run).
        let incoming = SidecarSettings::builder().exposure(0.0).build().unwrap();
        let outcome = merge_and_write(&p, &incoming, false).unwrap();
        assert_eq!(outcome, WriteOutcome::Overwritten);

        // Verify that existing and incoming settings are successfully merged
        let read_back = read_xmp(&p).unwrap();
        assert_eq!(read_back.exposure(), Some(0.0));
        assert_eq!(read_back.temperature(), Some(6000));
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

    #[test]
    fn test_slider_clamping_on_parse() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      crs:Temperature="1500"
      crs:Tint="200"
    />
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse");
        assert_eq!(
            s.temperature(),
            Some(2000),
            "temperature 1500 must clamp to 2000"
        );
        assert_eq!(s.tint(), Some(150), "tint 200 must clamp to 150");

        let xml_high = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      crs:Temperature="60000"
      crs:Tint="-180"
    />
  </rdf:RDF>
</x:xmpmeta>"#;
        let s_high = reader::parse_xmp_str(xml_high, Path::new("test.xmp")).expect("must parse");
        assert_eq!(
            s_high.temperature(),
            Some(50000),
            "temperature 60000 must clamp to 50000"
        );
        assert_eq!(s_high.tint(), Some(-150), "tint -180 must clamp to -150");
    }

    #[test]
    fn test_merge_and_write_empty_color_label_retention() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");

        // Existing sidecar has a label "Red"
        let existing = SidecarSettings::builder().label("Red").build().unwrap();
        write_xmp(&p, &existing).unwrap();

        // Incoming is empty color label
        let incoming = SidecarSettings::builder().label("").build().unwrap();

        let outcome = merge_and_write(&p, &incoming, false).unwrap();
        assert_eq!(outcome, WriteOutcome::Overwritten);

        let read_back = read_xmp(&p).unwrap();
        assert_eq!(
            read_back.label(),
            Some(""),
            "Empty color label must be retained as Some(\"\")"
        );

        // The XML should contain xmp:Label=""
        let xml = std::fs::read_to_string(&p).unwrap();
        assert!(
            xml.contains("xmp:Label=\"\""),
            "XML should explicitly write empty label attribute"
        );
    }

    #[test]
    fn test_precise_keyword_stripping_on_merge() {
        let mut existing_kws = std::collections::BTreeSet::new();
        existing_kws.insert("photohelper".to_string());
        existing_kws.insert("photohelper:old".to_string());
        existing_kws.insert("photohelper|old".to_string());
        existing_kws.insert("cluster:42".to_string());
        existing_kws.insert("nima:good".to_string());
        existing_kws.insert("cluster:notanint".to_string()); // user-defined keyword mimicking pattern but invalid int
        existing_kws.insert("nima:awesome".to_string()); // user-defined keyword mimicking pattern but invalid tier
        existing_kws.insert("my-own-keyword".to_string());

        let existing = SidecarSettings::builder()
            .keywords(existing_kws)
            .build()
            .unwrap();

        let mut incoming_kws = std::collections::BTreeSet::new();
        incoming_kws.insert("photohelper".to_string());
        incoming_kws.insert("cluster:100".to_string());
        incoming_kws.insert("nima:excellent".to_string());

        let incoming = SidecarSettings::builder()
            .keywords(incoming_kws)
            .build()
            .unwrap();

        let merged = existing.merge(&incoming);
        let merged_kws = merged.keywords().unwrap();

        // Valid photohelper ones from existing must be stripped, user ones must be kept
        assert!(merged_kws.contains("my-own-keyword"));
        assert!(merged_kws.contains("cluster:notanint"));
        assert!(merged_kws.contains("nima:awesome"));
        assert!(merged_kws.contains("photohelper"));
        assert!(merged_kws.contains("cluster:100"));
        assert!(merged_kws.contains("nima:excellent"));

        assert!(!merged_kws.contains("photohelper:old"));
        assert!(!merged_kws.contains("photohelper|old"));
        assert!(!merged_kws.contains("cluster:42"));
        assert!(!merged_kws.contains("nima:good"));
    }

    #[test]
    fn test_rating_try_from() {
        use std::convert::TryFrom;
        assert_eq!(Rating::try_from(-1).unwrap(), Rating::Rejected);
        assert_eq!(Rating::try_from(0).unwrap(), Rating::Unrated);
        assert_eq!(Rating::try_from(1).unwrap(), Rating::One);
        assert_eq!(Rating::try_from(2).unwrap(), Rating::Two);
        assert_eq!(Rating::try_from(3).unwrap(), Rating::Three);
        assert_eq!(Rating::try_from(4).unwrap(), Rating::Four);
        assert_eq!(Rating::try_from(5).unwrap(), Rating::Five);
        assert!(Rating::try_from(-2).is_err());
        assert!(Rating::try_from(6).is_err());
    }

    #[test]
    fn test_lenient_parsing_via_xmp() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/"
      xmp:Rating="99.0"
      crs:Exposure2012="not-a-float"
      crs:Temperature="5500"
    />
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse leniently");
        assert_eq!(s.rating(), None);
        assert_eq!(s.exposure(), None);
        assert_eq!(s.temperature(), Some(5500));
    }

    #[test]
    fn test_xml_illegal_control_character_sanitization() {
        let raw = "Hello\x00World\x1F!\tGood\nMorning";
        let s = SidecarSettings::builder().label(raw).build().unwrap();
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        write_xmp(&p, &s).unwrap();

        let read_back = read_xmp(&p).unwrap();
        assert_eq!(read_back.label().unwrap(), "HelloWorld!\tGood\nMorning");
    }

    #[test]
    fn test_write_no_keywords_omits_elements() {
        let s = SidecarSettings::builder().exposure(1.0).build().unwrap();
        let dir = tempdir().unwrap();
        let p = dir.path().join("photo.xmp");
        write_xmp(&p, &s).unwrap();
        let xml = std::fs::read_to_string(&p).unwrap();
        assert!(!xml.contains("subject"));
        assert!(!xml.contains("hierarchicalSubject"));
    }

    #[test]
    fn test_parse_prefix_agnostic_attributes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      Temperature="5200"
      Rating="4"
    />
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse");
        assert_eq!(s.temperature(), Some(5200));
        assert_eq!(s.rating(), Some(Rating::Four));
    }

    #[test]
    fn test_read_non_bag_rdf_containers() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:dc="http://purl.org/dc/elements/1.1/">
      <dc:subject>
        <rdf:Seq>
          <rdf:li>seq-keyword</rdf:li>
        </rdf:Seq>
      </dc:subject>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse Seq");
        assert!(s.keywords().unwrap().contains("seq-keyword"));
    }

    #[test]
    fn test_parse_crs_elements_detects_crs_attr() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
      <crs:Temperature>4800</crs:Temperature>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#;
        let s = reader::parse_xmp_str(xml, Path::new("test.xmp")).expect("must parse nested crs");
        assert_eq!(s.temperature(), Some(4800));
    }
}
