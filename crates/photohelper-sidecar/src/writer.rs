//! XMP sidecar writer with atomic write semantics.

use std::fmt::Write as FmtWrite;
use std::io::Write as _;
use std::path::Path;

use time::format_description::well_known::Rfc3339;

use crate::error::Error;
use crate::settings::SidecarSettings;

/// Write `settings` as an XMP sidecar at `path`.
///
/// **Atomic write**: writes to `<path>.phdev.tmp` first, then renames to
/// `path`. On POSIX systems the rename is atomic; on Windows it is
/// best-effort. If any step fails the temp file is removed and the original
/// (if any) is preserved.
///
/// All namespace declarations are always emitted on `rdf:Description`
/// regardless of which fields are set, to keep the XML well-formed.
///
/// # Errors
///
/// - [`Error::Io`] if the temp file cannot be created or written.
/// - [`Error::AtomicWrite`] if the rename step fails.
pub fn write_xmp(path: &Path, settings: &SidecarSettings) -> Result<(), Error> {
    let xml = render_xmp(settings);

    let tmp_path = path.with_extension("phdev.tmp");

    // Write to temp file.
    let mut f = std::fs::File::create(&tmp_path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    f.write_all(xml.as_bytes()).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    // fsync to ensure durability before rename.
    f.sync_all().map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    drop(f);

    // Atomic rename.
    std::fs::rename(&tmp_path, path).map_err(|e| {
        // Clean up the temp file on rename failure (best effort).
        let _ = std::fs::remove_file(&tmp_path);
        Error::AtomicWrite {
            path: path.to_path_buf(),
            source: e,
        }
    })
}

/// Render XMP as a UTF-8 string.
///
/// `crs:ProcessVersion="11.0"` is always written when any `crs:` field is set
/// (required for Camera Raw / Lightroom compatibility).
pub(crate) fn render_xmp(settings: &SidecarSettings) -> String {
    let mut attrs = String::new();

    // xmp:MetadataDate — always written if last_processed_at is set.
    if let Some(dt) = settings.last_processed_at() {
        let iso = dt.format(&Rfc3339).unwrap_or_default();
        write!(attrs, "\n      xmp:MetadataDate=\"{iso}\"").unwrap_or(());
    }

    // crs: fields (only if at least one is set).
    let has_crs = settings.has_crs_fields();
    if has_crs {
        attrs.push_str("\n      crs:ProcessVersion=\"11.0\"");
    }
    if let Some(t) = settings.temperature() {
        write!(attrs, "\n      crs:Temperature=\"{t}\"").unwrap_or(());
    }
    if let Some(t) = settings.tint() {
        write!(attrs, "\n      crs:Tint=\"{t}\"").unwrap_or(());
    }
    if let Some(e) = settings.exposure() {
        write!(attrs, "\n      crs:Exposure2012=\"{e:.2}\"").unwrap_or(());
    }
    if let Some(c) = settings.contrast() {
        write!(attrs, "\n      crs:Contrast2012=\"{c}\"").unwrap_or(());
    }
    if let Some(h) = settings.highlights() {
        write!(attrs, "\n      crs:Highlights2012=\"{h}\"").unwrap_or(());
    }
    if let Some(s) = settings.shadows() {
        write!(attrs, "\n      crs:Shadows2012=\"{s}\"").unwrap_or(());
    }

    // ph: fields.
    if let Some(score) = settings.nima_score() {
        write!(attrs, "\n      ph:NimaScore=\"{score:.4}\"").unwrap_or(());
    }
    if let Some(id) = settings.dedup_cluster_id() {
        write!(attrs, "\n      ph:DedupClusterId=\"{id}\"").unwrap_or(());
    }
    if let Some(pid) = settings.photohelper_id() {
        write!(attrs, "\n      ph:PhotohelperId=\"{pid}\"").unwrap_or(());
    }
    if let Some(dt) = settings.last_processed_at() {
        let iso = dt.format(&Rfc3339).unwrap_or_default();
        write!(attrs, "\n      ph:LastProcessedAt=\"{iso}\"").unwrap_or(());
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="photohelper">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/"
      xmlns:ph="http://ns.photohelper.dev/1.0/"{attrs}
    />
  </rdf:RDF>
</x:xmpmeta>
"#
    )
}
