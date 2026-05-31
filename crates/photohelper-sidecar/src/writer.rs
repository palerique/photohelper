#![allow(clippy::format_push_string)]
//! XMP sidecar writer with atomic write semantics.

use std::io::Write as _;

use time::format_description::well_known::Rfc3339;

use crate::error::Error;
use crate::path::SidecarPath;
use crate::settings::SidecarSettings;

/// Write `settings` as an XMP sidecar at `path`.
///
/// **Atomic write**: replaces the extension of `path` to form `<stem>.phdev.{pid}...tmp`
/// first, then renames to `path`. On POSIX systems the rename is atomic; on Windows it is
/// best-effort. If any step fails the temp file is removed and the original
/// (if any) is preserved.
///
/// **Physical mtime alignment**: On successful write, the physical filesystem modification time
/// (`mtime`) is set (best-effort) to match `ph:LastProcessedAt` exactly to prevent false conflict triggers.
/// Failures to align mtime are logged but will not abort the atomic write.
///
/// Core namespaces (such as `crs:` and `xmp:`) are always emitted on `rdf:Description`,
/// while conditional namespaces (such as `dc:` and `lr:`) are declared only when their
/// respective collections are non-empty.
///
/// # Errors
///
/// - [`Error::Validation`] if timestamp formatting fails.
/// - [`Error::Io`] if the temp file cannot be created or written.
/// - [`Error::AtomicWrite`] if the rename step fails.
///
/// # Returns
///
/// Returns an `Error` if XML generation fails or atomic file replacement fails.
#[allow(clippy::format_push_string)]
pub fn write_xmp(path: &SidecarPath, settings: &SidecarSettings) -> Result<(), Error> {
    let xml = render_xmp(settings)?;

    let pid = std::process::id();
    let nonce = uuid::Uuid::new_v4().simple();
    let tmp_path = path.with_extension(format!("phdev.{pid}.{nonce}.tmp"));

    // Write to temp file.
    let mut f = std::fs::File::create(&tmp_path).map_err(|e| Error::Io {
        path: tmp_path.clone(),
        source: e,
    })?;
    if let Err(e) = f.write_all(xml.as_bytes()).and_then(|()| f.sync_all()) {
        drop(f);
        if let Err(clean_err) = std::fs::remove_file(&tmp_path) {
            tracing::warn!(path = %tmp_path.display(), error = %clean_err, "failed to clean up temp file after write error");
        }
        return Err(Error::Io {
            path: tmp_path.clone(),
            source: e,
        });
    }
    drop(f);

    // Align filesystem modification time (mtime) with ph:LastProcessedAt exactly
    // *before* the rename, preventing TOCTOU.
    if let Some(dt) = settings.last_processed_at() {
        let ft = filetime::FileTime::from_unix_time(dt.unix_timestamp(), dt.nanosecond());
        if let Err(e) = filetime::set_file_mtime(&tmp_path, ft) {
            tracing::warn!(path = %tmp_path.display(), error = %e, "failed to set physical file mtime to match last_processed_at");
        }
    }

    // Copy permissions from original file if it exists, to avoid inheriting umask defaults
    match std::fs::metadata(path.as_path()) {
        Ok(metadata) => {
            if let Err(e) = std::fs::set_permissions(&tmp_path, metadata.permissions()) {
                tracing::warn!(path = %tmp_path.display(), error = %e, "failed to copy permissions to temp file");
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to read target permissions");
        }
    }

    // Atomic rename.
    std::fs::rename(&tmp_path, path.as_path()).map_err(|e| {
        // Clean up the temp file on rename failure (best effort).
        if let Err(clean_err) = std::fs::remove_file(&tmp_path) {
            tracing::warn!(path = %tmp_path.display(), error = %clean_err, "failed to clean up temp file after rename error");
        }
        Error::AtomicWrite {
            path: path.to_path_buf(),
            source: e,
        }
    })?;

    Ok(())
}

/// Render XMP as a UTF-8 string.
///
/// `crs:ProcessVersion="11.0"` and `crs:HasSettings="True"` are always written when any `crs:` field is set
/// (required for Camera Raw / Lightroom compatibility).
///
/// # Stop-gap
///
/// TD-022: hand-rolled string template emits only the specific fields photohelper manages.
/// Fields written by Lightroom or other tools (e.g. `crs:CameraProfile`,
/// `crs:ToneCurvePV2012`) are not modeled and will be silently absent on any
/// photohelper overwrite. See TECH-DEBT.md § TD-022.
pub(crate) fn render_xmp(settings: &SidecarSettings) -> Result<String, Error> {
    let mut attrs = String::new();

    let dt = settings.last_processed_at().or(settings.metadata_date());

    // xmp:MetadataDate — always written if dt is set.
    if let Some(d) = dt {
        let iso = d.format(&Rfc3339).map_err(|e| Error::Validation {
            message: format!("could not format timestamp as RFC 3339: {e}"),
        })?;
        attrs.push_str(&format!("\n      xmp:MetadataDate=\"{iso}\""));

        if settings.last_processed_at().is_some() {
            attrs.push_str(&format!("\n      ph:LastProcessedAt=\"{iso}\""));
        }
    }

    // crs: fields (only if at least one is set).
    let has_crs = settings.has_crs_fields();
    if has_crs {
        attrs.push_str("\n      crs:ProcessVersion=\"11.0\"");
        attrs.push_str("\n      crs:HasSettings=\"True\"");
    }
    if let Some(t) = settings.temperature() {
        attrs.push_str(&format!("\n      crs:Temperature=\"{t}\""));
    }
    if let Some(t) = settings.tint() {
        attrs.push_str(&format!("\n      crs:Tint=\"{t}\""));
    }
    if let Some(e) = settings.exposure() {
        attrs.push_str(&format!("\n      crs:Exposure2012=\"{e:.2}\""));
    }
    if let Some(c) = settings.contrast() {
        attrs.push_str(&format!("\n      crs:Contrast2012=\"{c}\""));
    }
    if let Some(h) = settings.highlights() {
        attrs.push_str(&format!("\n      crs:Highlights2012=\"{h}\""));
    }
    if let Some(s) = settings.shadows() {
        attrs.push_str(&format!("\n      crs:Shadows2012=\"{s}\""));
    }

    // ph: fields.
    if let Some(score) = settings.nima_score() {
        attrs.push_str(&format!("\n      ph:NimaScore=\"{score:.4}\""));
    }
    if let Some(id) = settings.dedup_cluster_id() {
        attrs.push_str(&format!("\n      ph:DedupClusterId=\"{id}\""));
    }
    if let Some(pid) = settings.photohelper_id() {
        let escaped = quick_xml::escape::escape(pid);
        attrs.push_str(&format!("\n      ph:PhotohelperId=\"{escaped}\""));
    }

    // Standard rating and label
    if let Some(r) = settings.rating() {
        if r != crate::settings::Rating::Unrated {
            attrs.push_str(&format!("\n      xmp:Rating=\"{}\"", r.as_i32()));
        }
    }
    if let Some(l) = settings.label() {
        let sanitized = sanitize_xml_string(l);
        let escaped = quick_xml::escape::escape(&sanitized);
        attrs.push_str(&format!("\n      xmp:Label=\"{escaped}\""));
    }

    let mut ns_decls = String::new();
    let mut children = String::new();

    let mut render_bag =
        |kws: std::collections::btree_set::Iter<String>, ns_decl: &str, tag: &str, prefix: &str| {
            ns_decls.push_str(ns_decl);
            children.push_str(&format!("      <{prefix}:{tag}>\n        <rdf:Bag>\n"));
            for kw in kws {
                let sanitized = sanitize_xml_string(kw);
                let escaped = quick_xml::escape::escape(&sanitized);
                children.push_str(&format!("          <rdf:li>{escaped}</rdf:li>\n"));
            }
            children.push_str(&format!("        </rdf:Bag>\n      </{prefix}:{tag}>\n"));
        };

    if let Some(kws) = settings.keywords().filter(|k| !k.is_empty()) {
        render_bag(
            kws.iter(),
            "\n      xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
            "subject",
            "dc",
        );
    }

    if let Some(kws) = settings.hierarchical_keywords().filter(|k| !k.is_empty()) {
        render_bag(
            kws.iter(),
            "\n      xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"",
            "hierarchicalSubject",
            "lr",
        );
    }

    let end_tag = if children.is_empty() {
        "\n    />".to_string()
    } else {
        format!(">\n{children}    </rdf:Description>")
    };

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="photohelper">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/"
      xmlns:ph="http://ns.photohelper.dev/1.0/"{ns_decls}{attrs}{end_tag}
  </rdf:RDF>
</x:xmpmeta>
"#
    ))
}

/// Returns true if the character is a valid XML 1.0 character.
pub fn is_valid_xml_char(c: char) -> bool {
    let val = c as u32;
    let is_valid_xml_char = (0x20..=0xD7FF).contains(&val)
        || val == 0x09
        || val == 0x0A
        || val == 0x0D
        || (0xE000..=0xFFFD).contains(&val)
        || (0x10000..=0x10_FFFF).contains(&val);
    let is_noncharacter = (0xFDD0..=0xFDEF).contains(&val) || (val & 0xFFFE) == 0xFFFE;
    is_valid_xml_char && !is_noncharacter
}

/// Returns true if the string contains only valid XML 1.0 characters.
pub fn is_valid_xml_string(s: &str) -> bool {
    s.chars().all(is_valid_xml_char)
}

/// Filters characters according to the XML 1.0 Valid Character specification
/// to prevent serialization of control characters or non-characters that
/// would produce malformed XML.
fn sanitize_xml_string(s: &str) -> String {
    s.chars().filter(|&c| is_valid_xml_char(c)).collect()
}
