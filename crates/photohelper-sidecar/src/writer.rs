//! XMP sidecar writer with atomic write semantics.

use std::fmt::Write as FmtWrite;
use std::io::Write as _;
use std::path::Path;

use time::format_description::well_known::Rfc3339;

use crate::error::Error;
use crate::settings::SidecarSettings;

/// Write `settings` as an XMP sidecar at `path`.
///
/// **Atomic write**: writes to `<path>.phdev.{pid}.{thread_id_str}.tmp` first, then renames to
/// `path`. On POSIX systems the rename is atomic; on Windows it is
/// best-effort. If any step fails the temp file is removed and the original
/// (if any) is preserved.
///
/// **Physical mtime alignment**: On successful write, the physical filesystem modification time
/// (`mtime`) is set to match `ph:LastProcessedAt` exactly to prevent false conflict triggers.
///
/// Core namespaces (such as `crs:` and `xmp:`) are always emitted on `rdf:Description`,
/// while conditional namespaces (such as `dc:` and `lr:`) are declared only when their
/// respective collections are non-empty.
///
/// # Errors
///
/// - [`Error::Io`] if the temp file cannot be created or written.
/// - [`Error::AtomicWrite`] if the rename step fails.
pub fn write_xmp(path: &Path, settings: &SidecarSettings) -> Result<(), Error> {
    let xml = render_xmp(settings);

    let pid = std::process::id();
    let thread_id = std::thread::current().id();
    let thread_id_str = format!("{thread_id:?}")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>();
    let tmp_path = path.with_extension(format!("phdev.{pid}.{thread_id_str}.tmp"));

    // Write to temp file.
    let mut f = std::fs::File::create(&tmp_path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    if let Err(e) = f.write_all(xml.as_bytes()).and_then(|()| f.sync_all()) {
        drop(f);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(Error::Io {
            path: path.to_path_buf(),
            source: e,
        });
    }
    drop(f);

    // Atomic rename.
    std::fs::rename(&tmp_path, path).map_err(|e| {
        // Clean up the temp file on rename failure (best effort).
        let _ = std::fs::remove_file(&tmp_path);
        Error::AtomicWrite {
            path: path.to_path_buf(),
            source: e,
        }
    })?;

    // Align filesystem modification time (mtime) with ph:LastProcessedAt exactly.
    if let Some(dt) = settings.last_processed_at() {
        let ft = filetime::FileTime::from_unix_time(dt.unix_timestamp(), dt.nanosecond());
        if let Err(e) = filetime::set_file_mtime(path, ft) {
            tracing::warn!(path = %path.display(), error = %e, "failed to set physical file mtime to match last_processed_at");
        }
    }

    Ok(())
}

/// Render XMP as a UTF-8 string.
///
/// `crs:ProcessVersion="11.0"` is always written when any `crs:` field is set
/// (required for Camera Raw / Lightroom compatibility).
///
/// # Stop-gap
///
/// TD-022: hand-rolled `quick-xml` template emits only the 16 fields photohelper
/// writes. Fields written by Lightroom or other tools (e.g. `crs:CameraProfile`,
/// `crs:ToneCurvePV2012`) are not modeled and will be silently absent on any
/// photohelper overwrite. See TECH-DEBT.md § TD-022.
pub(crate) fn render_xmp(settings: &SidecarSettings) -> String {
    let mut attrs = String::new();

    // xmp:MetadataDate — always written if last_processed_at is set.
    // Use match instead of unwrap_or_default() to avoid writing an empty attribute
    // on format failure (which would corrupt conflict resolution on subsequent reads).
    if let Some(dt) = settings.last_processed_at() {
        match dt.format(&Rfc3339) {
            Ok(iso) => {
                let _ = write!(attrs, "\n      xmp:MetadataDate=\"{iso}\"");
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not format timestamp as RFC 3339; omitting xmp:MetadataDate");
            }
        }
    }

    // crs: fields (only if at least one is set).
    let has_crs = settings.has_crs_fields();
    if has_crs {
        attrs.push_str("\n      crs:ProcessVersion=\"11.0\"");
    }
    if let Some(t) = settings.temperature() {
        let _ = write!(attrs, "\n      crs:Temperature=\"{t}\"");
    }
    if let Some(t) = settings.tint() {
        let _ = write!(attrs, "\n      crs:Tint=\"{t}\"");
    }
    if let Some(e) = settings.exposure() {
        let _ = write!(attrs, "\n      crs:Exposure2012=\"{e:.2}\"");
    }
    if let Some(c) = settings.contrast() {
        let _ = write!(attrs, "\n      crs:Contrast2012=\"{c}\"");
    }
    if let Some(h) = settings.highlights() {
        let _ = write!(attrs, "\n      crs:Highlights2012=\"{h}\"");
    }
    if let Some(s) = settings.shadows() {
        let _ = write!(attrs, "\n      crs:Shadows2012=\"{s}\"");
    }

    // ph: fields.
    if let Some(score) = settings.nima_score() {
        let _ = write!(attrs, "\n      ph:NimaScore=\"{score:.4}\"");
    }
    if let Some(id) = settings.dedup_cluster_id() {
        let _ = write!(attrs, "\n      ph:DedupClusterId=\"{id}\"");
    }
    if let Some(pid) = settings.photohelper_id() {
        let _ = write!(attrs, "\n      ph:PhotohelperId=\"{pid}\"");
    }
    if let Some(dt) = settings.last_processed_at() {
        if let Ok(iso) = dt.format(&Rfc3339) {
            let _ = write!(attrs, "\n      ph:LastProcessedAt=\"{iso}\"");
        }
        // On format failure, xmp:MetadataDate was also omitted above — no need to
        // duplicate the warning here.
    }

    // Standard rating and label
    if let Some(r) = settings.rating() {
        if r != crate::settings::Rating::Unrated {
            let _ = write!(attrs, "\n      xmp:Rating=\"{}\"", r.as_i32());
        }
    }
    if let Some(l) = settings.label() {
        let sanitized = sanitize_xml_string(l);
        let escaped = quick_xml::escape::escape(&sanitized);
        let _ = write!(attrs, "\n      xmp:Label=\"{escaped}\"");
    }

    let mut ns_decls = String::new();
    let mut children = String::new();

    if let Some(kws) = settings.keywords().filter(|k| !k.is_empty()) {
        ns_decls.push_str("\n      xmlns:dc=\"http://purl.org/dc/elements/1.1/\"");
        children.push_str("      <dc:subject>\n        <rdf:Bag>\n");
        for kw in kws {
            let sanitized = sanitize_xml_string(kw);
            let escaped = quick_xml::escape::escape(&sanitized);
            let _ = writeln!(children, "          <rdf:li>{escaped}</rdf:li>");
        }
        children.push_str("        </rdf:Bag>\n      </dc:subject>\n");
    }

    if let Some(kws) = settings.hierarchical_keywords().filter(|k| !k.is_empty()) {
        ns_decls.push_str("\n      xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"");
        children.push_str("      <lr:hierarchicalSubject>\n        <rdf:Bag>\n");
        for kw in kws {
            let sanitized = sanitize_xml_string(kw);
            let escaped = quick_xml::escape::escape(&sanitized);
            let _ = writeln!(children, "          <rdf:li>{escaped}</rdf:li>");
        }
        children.push_str("        </rdf:Bag>\n      </lr:hierarchicalSubject>\n");
    }

    let end_tag = if children.is_empty() {
        "\n    />".to_string()
    } else {
        format!(">\n{children}    </rdf:Description>")
    };

    format!(
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
    )
}

fn sanitize_xml_string(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            let val = c as u32;
            let is_valid_xml_char = (0x20..=0xD7FF).contains(&val)
                || val == 0x09
                || val == 0x0A
                || val == 0x0D
                || (0xE000..=0xFFFD).contains(&val)
                || (0x10000..=0x10_FFFF).contains(&val);
            let is_noncharacter = (0xFDD0..=0xFDEF).contains(&val) || (val & 0xFFFE) == 0xFFFE;
            is_valid_xml_char && !is_noncharacter
        })
        .collect()
}
