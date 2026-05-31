#![allow(clippy::format_push_string)]
//! XMP sidecar writer with atomic write semantics.

use quick_xml::events::{BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};
use std::io::Write as _;
use std::path::Path;
use time::format_description::well_known::Rfc3339;

use crate::error::Error;

#[allow(dead_code)]
const DEFAULT_XMP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<?xpacket begin="\u{feff}" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="photohelper">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

use crate::path::SidecarPath;
use crate::settings::SidecarSettings;

#[derive(Debug)]
enum WriterState {
    SeekingDescription { depth: usize },
    InsideDescription { unmanaged_depth: usize },
    InjectionComplete,
}

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
/// **Update Behavior**: Namespaces are preserved as-is on updates. Only managed attributes and tags
/// (like `crs:Exposure2012`, `dc:subject`, etc.) are altered or injected. All third-party XML elements,
/// including elements with unknown tags, are preserved transparently.
///
/// # Errors
///
/// - [`Error::Validation`] if timestamp formatting fails.
/// - [`Error::Io`] if the temp file cannot be created or written.
/// - [`Error::AtomicWrite`] if the rename step fails.
/// - [`Error::XmlParse`] if the existing sidecar XML is structurally invalid or abruptly truncated.
/// - [`Error::MissingRdfDescription`] if the sidecar ends without containing an `rdf:Description`.
///
/// # Returns
///
/// Returns an `Error` if XML generation fails or atomic file replacement fails.
pub fn write_xmp(path: &SidecarPath, settings: &SidecarSettings) -> Result<(), Error> {
    write_xmp_impl(path, settings, false, None)
}

pub(crate) fn write_xmp_force(path: &SidecarPath, settings: &SidecarSettings) -> Result<(), Error> {
    write_xmp_impl(path, settings, true, None)
}

pub(crate) fn write_xmp_guarded(
    path: &SidecarPath,
    settings: &SidecarSettings,
    expected_mtime: std::time::SystemTime,
) -> Result<(), Error> {
    write_xmp_impl(path, settings, false, Some(expected_mtime))
}

fn write_xmp_impl(
    path: &SidecarPath,
    settings: &SidecarSettings,
    force_creation: bool,
    expected_mtime: Option<std::time::SystemTime>,
) -> Result<(), Error> {
    let target_path = path.as_path();
    let is_new = force_creation || !target_path.exists();

    let xml_content = if is_new {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="photohelper">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" />
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
            .to_string()
    } else {
        match std::fs::read_to_string(target_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                r#"<?xml version="1.0" encoding="UTF-8"?>
<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="photohelper">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about="" />
  </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
                    .to_string()
            }
            Err(e) => {
                return Err(Error::Io {
                    path: target_path.to_path_buf(),
                    source: e,
                });
            }
        }
    };

    let mut reader = Reader::from_str(&xml_content);
    reader.config_mut().trim_text(false);

    if is_new {
        if let Some(parent) = target_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(Error::Io {
                    path: target_path.to_path_buf(),
                    source: e,
                });
            }
        }
    }

    let canonical_target = target_path
        .canonicalize()
        .unwrap_or_else(|_| target_path.to_path_buf());
    let parent_dir = canonical_target.parent().unwrap_or_else(|| Path::new("."));

    #[allow(unused_variables)]
    let current_mtime = std::fs::metadata(target_path)
        .and_then(|m| m.modified())
        .ok();
    let mut temp_file = tempfile::Builder::new()
        .prefix("phdev.")
        .suffix(".tmp")
        .tempfile_in(parent_dir)
        .map_err(|e| Error::Io {
            path: target_path.to_path_buf(),
            source: e,
        })?;

    let mut writer = Writer::new(temp_file.as_file_mut());

    let mut state = WriterState::SeekingDescription { depth: 0 };
    let mut drop_depth: usize = 0;
    let mut buf = Vec::new();

    loop {
        buf.clear();
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| Error::XmlParse {
                path: target_path.to_path_buf(),
                message: e.to_string(),
                state: format!("{state:?}"),
            })?;

        match event {
            Event::Eof => {
                if !matches!(state, WriterState::InjectionComplete) {
                    return Err(Error::MissingRdfDescription {
                        path: target_path.to_path_buf(),
                    });
                }
                break;
            }
            Event::Start(ref e) => {
                if drop_depth > 0 {
                    drop_depth += 1;
                    continue;
                }
                match state {
                    WriterState::SeekingDescription { depth } => {
                        let new_depth = depth + 1;
                        let name_bytes = e.name().into_inner();
                        let name_str = String::from_utf8_lossy(name_bytes);
                        let is_desc = name_str.eq_ignore_ascii_case("rdf:Description")
                            || name_str.eq_ignore_ascii_case("rdf:description");

                        if is_desc {
                            let modified_tag = process_attributes_empty(e, settings, target_path)?;
                            writer
                                .write_event(Event::Start(modified_tag))
                                .map_err(|err| Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                })?;
                            state = WriterState::InsideDescription { unmanaged_depth: 0 };
                        } else {
                            writer.write_event(Event::Start(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                            state = WriterState::SeekingDescription { depth: new_depth };
                        }
                    }
                    WriterState::InsideDescription { unmanaged_depth } => {
                        let name_bytes = e.name().into_inner();
                        #[allow(unused_variables)]
                        let tag_name = String::from_utf8_lossy(name_bytes);
                        if is_managed_tag(name_bytes) {
                            drop_depth += 1;
                        } else {
                            writer.write_event(Event::Start(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                            state = WriterState::InsideDescription {
                                unmanaged_depth: unmanaged_depth + 1,
                            };
                        }
                    }
                    WriterState::InjectionComplete => {
                        writer.write_event(Event::Start(e.clone())).map_err(|err| {
                            Error::XmlParse {
                                path: target_path.to_path_buf(),
                                message: err.to_string(),
                                state: format!("{state:?}"),
                            }
                        })?;
                    }
                }
            }
            Event::Empty(ref e) => {
                if drop_depth > 0 {
                    continue; // do NOT increase drop_depth for Empty elements
                }
                match state {
                    WriterState::SeekingDescription { depth: _ } => {
                        let name_bytes = e.name().into_inner();
                        let name_str = String::from_utf8_lossy(name_bytes);
                        let is_desc = name_str.eq_ignore_ascii_case("rdf:Description")
                            || name_str.eq_ignore_ascii_case("rdf:description");
                        if is_desc {
                            // Expand to Start, inject, End
                            let modified_tag = process_attributes_empty(e, settings, target_path)?;
                            writer
                                .write_event(Event::Start(modified_tag))
                                .map_err(|err| Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                })?;
                            inject_managed_children(&mut writer, settings, target_path)?;
                            writer
                                .write_event(Event::End(BytesEnd::new(name_str.to_string())))
                                .map_err(|err| Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                })?;
                            state = WriterState::InjectionComplete;
                        } else {
                            writer.write_event(Event::Empty(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                        }
                    }
                    WriterState::InsideDescription { unmanaged_depth: _ } => {
                        let name_bytes = e.name().into_inner();
                        #[allow(unused_variables)]
                        let tag_name = String::from_utf8_lossy(name_bytes);
                        if !is_managed_tag(name_bytes) {
                            writer.write_event(Event::Empty(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                        }
                    }
                    WriterState::InjectionComplete => {
                        writer.write_event(Event::Empty(e.clone())).map_err(|err| {
                            Error::XmlParse {
                                path: target_path.to_path_buf(),
                                message: err.to_string(),
                                state: format!("{state:?}"),
                            }
                        })?;
                    }
                }
            }
            Event::End(ref e) => {
                if drop_depth > 0 {
                    drop_depth -= 1;
                    continue;
                }
                match state {
                    WriterState::SeekingDescription { depth } => {
                        let name_bytes = e.name().into_inner();
                        let name_str = String::from_utf8_lossy(name_bytes);
                        if name_str.eq_ignore_ascii_case("rdf:RDF")
                            || name_str.eq_ignore_ascii_case("rdf:rdf")
                        {
                            let mut start_tag = BytesStart::new("rdf:Description");
                            start_tag.push_attribute(("rdf:about", ""));
                            let modified_tag =
                                process_attributes_empty(&start_tag, settings, target_path)?;
                            writer
                                .write_event(Event::Start(modified_tag))
                                .map_err(|err| Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                })?;
                            inject_managed_children(&mut writer, settings, target_path)?;
                            writer
                                .write_event(Event::End(BytesEnd::new("rdf:Description")))
                                .map_err(|err| Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                })?;

                            writer.write_event(Event::End(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                            state = WriterState::InjectionComplete;
                        } else {
                            writer.write_event(Event::End(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                            state = WriterState::SeekingDescription {
                                depth: depth.saturating_sub(1),
                            };
                        }
                    }
                    WriterState::InsideDescription { unmanaged_depth } => {
                        if unmanaged_depth > 0 {
                            writer.write_event(Event::End(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                            state = WriterState::InsideDescription {
                                unmanaged_depth: unmanaged_depth - 1,
                            };
                        } else {
                            // Exiting rdf:Description
                            inject_managed_children(&mut writer, settings, target_path)?;
                            writer.write_event(Event::End(e.clone())).map_err(|err| {
                                Error::XmlParse {
                                    path: target_path.to_path_buf(),
                                    message: err.to_string(),
                                    state: format!("{state:?}"),
                                }
                            })?;
                            state = WriterState::InjectionComplete;
                        }
                    }
                    WriterState::InjectionComplete => {
                        writer.write_event(Event::End(e.clone())).map_err(|err| {
                            Error::XmlParse {
                                path: target_path.to_path_buf(),
                                message: err.to_string(),
                                state: format!("{state:?}"),
                            }
                        })?;
                    }
                }
            }
            Event::Text(_) | Event::CData(_) => {
                if drop_depth == 0 {
                    writer
                        .write_event(event.clone())
                        .map_err(|err| Error::XmlParse {
                            path: target_path.to_path_buf(),
                            message: err.to_string(),
                            state: format!("{state:?}"),
                        })?;
                }
            }
            Event::Decl(_) | Event::PI(_) | Event::DocType(_) | Event::Comment(_) => {
                writer
                    .write_event(event.clone())
                    .map_err(|err| Error::XmlParse {
                        path: target_path.to_path_buf(),
                        message: err.to_string(),
                        state: format!("{state:?}"),
                    })?;
            }
        }
    }

    if let Err(e) = writer.into_inner().flush() {
        return Err(Error::Io {
            path: target_path.to_path_buf(),
            source: e,
        });
    }

    if let Some(dt) = settings.last_processed_at() {
        let ft = filetime::FileTime::from_unix_time(dt.unix_timestamp(), dt.nanosecond());
        if let Err(e) = filetime::set_file_mtime(temp_file.path(), ft) {
            tracing::warn!(path = %temp_file.path().display(), error = %e, "failed to set physical file mtime");
        }
    }

    match std::fs::metadata(target_path) {
        Ok(metadata) => {
            let mut perms = metadata.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
            }
            if let Err(e) = std::fs::set_permissions(temp_file.path(), perms) {
                tracing::warn!(path = %temp_file.path().display(), error = %e, "[PH-PERM-COPY-FAIL] failed to inherit permissions");
            }
            #[cfg(windows)]
            {
                if metadata.permissions().readonly() {
                    let mut target_perms = metadata.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    target_perms.set_readonly(false);
                    let _ = std::fs::set_permissions(target_path, target_perms);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(path = %target_path.display(), error = %e, "failed to read target permissions");
        }
    }

    if let Some(expected) = expected_mtime {
        if let Ok(current_meta) = std::fs::metadata(target_path) {
            if let Ok(current_mtime) = current_meta.modified() {
                if current_mtime != expected {
                    return Err(Error::Io {
                        path: target_path.to_path_buf(),
                        source: std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "Concurrent modification detected",
                        ),
                    });
                }
            }
        }
    }

    let mut original_was_readonly = false;
    if let Ok(metadata) = std::fs::metadata(target_path) {
        original_was_readonly = metadata.permissions().readonly();
    }

    temp_file
        .persist(target_path)
        .map_err(|e| Error::AtomicWrite {
            path: target_path.to_path_buf(),
            source: e.error,
        })?;

    if original_was_readonly {
        if let Ok(metadata) = std::fs::metadata(target_path) {
            let mut perms = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(true);
            let _ = std::fs::set_permissions(target_path, perms);
        }
    }

    #[cfg(unix)]
    {
        if let Some(parent) = target_path.parent() {
            if let Ok(dir) = std::fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
    }

    Ok(())
}

fn process_attributes_empty(
    tag: &BytesStart<'_>,
    settings: &SidecarSettings,
    target_path: &Path,
) -> Result<BytesStart<'static>, Error> {
    let mut new_tag =
        BytesStart::new(String::from_utf8_lossy(tag.name().into_inner()).into_owned());
    let mut managed_keys = std::collections::HashSet::new();

    let mut found_ns_crs = false;
    let mut found_ns_xmp = false;
    let mut found_ns_ph = false;

    // Build list of keys to set
    let dt = settings.last_processed_at().or(settings.metadata_date());
    let mut attributes_to_set = Vec::new();

    if let Some(d) = dt {
        if let Ok(iso) = d.format(&Rfc3339) {
            attributes_to_set.push(("xmp:MetadataDate", iso.clone()));
            managed_keys.insert("xmp:MetadataDate".to_string());
            if settings.last_processed_at().is_some() {
                attributes_to_set.push(("ph:LastProcessedAt", iso));
                managed_keys.insert("ph:LastProcessedAt".to_string());
            }
        }
    }

    if settings.has_crs_fields() {
        attributes_to_set.push(("crs:ProcessVersion", "11.0".to_string()));
        attributes_to_set.push(("crs:HasSettings", "True".to_string()));
        managed_keys.insert("crs:ProcessVersion".to_string());
        managed_keys.insert("crs:HasSettings".to_string());
    }

    if let Some(t) = settings.temperature() {
        attributes_to_set.push(("crs:Temperature", t.to_string()));
        managed_keys.insert("crs:Temperature".to_string());
    }
    if let Some(t) = settings.tint() {
        attributes_to_set.push(("crs:Tint", t.to_string()));
        managed_keys.insert("crs:Tint".to_string());
    }
    if let Some(b) = settings.auto_tone() {
        attributes_to_set.push(("crs:AutoTone", if b { "True" } else { "False" }.to_string()));
        managed_keys.insert("crs:AutoTone".to_string());
    }
    if let Some(e) = settings.exposure() {
        attributes_to_set.push(("crs:Exposure2012", e.to_string()));
        managed_keys.insert("crs:Exposure2012".to_string());
    }
    if let Some(c) = settings.contrast() {
        attributes_to_set.push(("crs:Contrast2012", c.to_string()));
        managed_keys.insert("crs:Contrast2012".to_string());
    }
    if let Some(h) = settings.highlights() {
        attributes_to_set.push(("crs:Highlights2012", h.to_string()));
        managed_keys.insert("crs:Highlights2012".to_string());
    }
    if let Some(s) = settings.shadows() {
        attributes_to_set.push(("crs:Shadows2012", s.to_string()));
        managed_keys.insert("crs:Shadows2012".to_string());
    }

    if let Some(score) = settings.nima_score() {
        attributes_to_set.push(("ph:NimaScore", format!("{score:.4}")));
        managed_keys.insert("ph:NimaScore".to_string());
    }
    if let Some(id) = settings.dedup_cluster_id() {
        attributes_to_set.push(("ph:DedupClusterId", id.to_string()));
        managed_keys.insert("ph:DedupClusterId".to_string());
    }
    if let Some(pid) = settings.photohelper_id() {
        attributes_to_set.push(("ph:PhotohelperId", pid.to_string()));
        managed_keys.insert("ph:PhotohelperId".to_string());
    }

    if let Some(r) = settings.rating() {
        if r == crate::settings::Rating::Unrated {
            managed_keys.insert("xmp:Rating".to_string()); // Clear it
        } else {
            attributes_to_set.push(("xmp:Rating", r.as_i32().to_string()));
        }
        managed_keys.insert("xmp:Rating".to_string());
    }
    if let Some(l) = settings.label() {
        let sanitized: String = l
            .chars()
            .filter(|&c| crate::xml::is_valid_xml_char(c))
            .collect();
        attributes_to_set.push(("xmp:Label", sanitized));
        managed_keys.insert("xmp:Label".to_string());
    }

    // Pass through unmanaged attributes
    for attr_res in tag.attributes() {
        let attr = attr_res.map_err(|e| Error::XmlParse {
            path: target_path.to_path_buf(),
            message: e.to_string(),
            state: "ProcessingAttributes".to_string(),
        })?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();

        if key.eq_ignore_ascii_case("xmlns:crs") {
            found_ns_crs = true;
        }
        if key.eq_ignore_ascii_case("xmlns:xmp") {
            found_ns_xmp = true;
        }
        if key.eq_ignore_ascii_case("xmlns:ph") {
            found_ns_ph = true;
        }

        if !is_managed_tag(attr.key.into_inner()) {
            new_tag.push_attribute(attr);
        }
    }

    // Append namespaces if needed
    if !found_ns_crs {
        new_tag.push_attribute(("xmlns:crs", "http://ns.adobe.com/camera-raw-settings/1.0/"));
    }
    if !found_ns_xmp {
        new_tag.push_attribute(("xmlns:xmp", "http://ns.adobe.com/xap/1.0/"));
    }
    if !found_ns_ph {
        new_tag.push_attribute(("xmlns:ph", "http://ns.photohelper.dev/1.0/"));
    }

    // Append new attributes
    for (k, v) in attributes_to_set {
        new_tag.push_attribute((k, v.as_str()));
    }

    Ok(new_tag)
}

fn is_managed_tag(name: &[u8]) -> bool {
    name.eq_ignore_ascii_case(b"dc:subject")
        || name.eq_ignore_ascii_case(b"lr:hierarchicalSubject")
        || name.eq_ignore_ascii_case(b"crs:Temperature")
        || name.eq_ignore_ascii_case(b"crs:Tint")
        || name.eq_ignore_ascii_case(b"crs:Exposure2012")
        || name.eq_ignore_ascii_case(b"crs:Contrast2012")
        || name.eq_ignore_ascii_case(b"crs:Highlights2012")
        || name.eq_ignore_ascii_case(b"crs:Shadows2012")
        || name.eq_ignore_ascii_case(b"crs:AutoTone")
        || name.eq_ignore_ascii_case(b"ph:NimaScore")
        || name.eq_ignore_ascii_case(b"ph:DedupClusterId")
        || name.eq_ignore_ascii_case(b"ph:PhotohelperId")
        || name.eq_ignore_ascii_case(b"xmp:Rating")
        || name.eq_ignore_ascii_case(b"xmp:Label")
        || name.eq_ignore_ascii_case(b"xmp:MetadataDate")
        || name.eq_ignore_ascii_case(b"ph:LastProcessedAt")
        || name.eq_ignore_ascii_case(b"crs:ProcessVersion")
        || name.eq_ignore_ascii_case(b"crs:HasSettings")
}

fn inject_managed_children<W: std::io::Write>(
    writer: &mut Writer<W>,
    settings: &SidecarSettings,
    target_path: &Path,
) -> Result<(), Error> {
    let mut render_bag = |kws: std::collections::btree_set::Iter<String>,
                          ns_decl: &str,
                          tag: &str,
                          prefix: &str|
     -> Result<(), Error> {
        let mut start_tag = BytesStart::new(format!("{prefix}:{tag}"));
        if ns_decl == "dc" {
            start_tag.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
        } else if ns_decl == "lr" {
            start_tag.push_attribute(("xmlns:lr", "http://ns.adobe.com/lightroom/1.0/"));
        }

        writer
            .write_event(Event::Start(start_tag))
            .map_err(|e| Error::XmlParse {
                path: target_path.to_path_buf(),
                message: e.to_string(),
                state: "Injecting".to_string(),
            })?;
        writer
            .write_event(Event::Start(BytesStart::new("rdf:Bag")))
            .map_err(|e| Error::XmlParse {
                path: target_path.to_path_buf(),
                message: e.to_string(),
                state: "Injecting".to_string(),
            })?;

        for kw in kws {
            let sanitized: String = kw
                .chars()
                .filter(|&c| crate::xml::is_valid_xml_char(c))
                .collect();
            // BytesText::new auto-escapes
            writer
                .write_event(Event::Start(BytesStart::new("rdf:li")))
                .map_err(|e| Error::XmlParse {
                    path: target_path.to_path_buf(),
                    message: e.to_string(),
                    state: "Injecting".to_string(),
                })?;
            writer
                .write_event(Event::Text(quick_xml::events::BytesText::new(&sanitized)))
                .map_err(|e| Error::XmlParse {
                    path: target_path.to_path_buf(),
                    message: e.to_string(),
                    state: "Injecting".to_string(),
                })?;
            writer
                .write_event(Event::End(BytesEnd::new("rdf:li")))
                .map_err(|e| Error::XmlParse {
                    path: target_path.to_path_buf(),
                    message: e.to_string(),
                    state: "Injecting".to_string(),
                })?;
        }

        writer
            .write_event(Event::End(BytesEnd::new("rdf:Bag")))
            .map_err(|e| Error::XmlParse {
                path: target_path.to_path_buf(),
                message: e.to_string(),
                state: "Injecting".to_string(),
            })?;
        writer
            .write_event(Event::End(BytesEnd::new(format!("{prefix}:{tag}"))))
            .map_err(|e| Error::XmlParse {
                path: target_path.to_path_buf(),
                message: e.to_string(),
                state: "Injecting".to_string(),
            })?;
        Ok(())
    };

    if let Some(kws) = settings.keywords().filter(|k| !k.is_empty()) {
        render_bag(kws.iter(), "dc", "subject", "dc")?;
    }

    if let Some(kws) = settings.hierarchical_keywords().filter(|k| !k.is_empty()) {
        render_bag(kws.iter(), "lr", "hierarchicalSubject", "lr")?;
    }

    Ok(())
}

#[allow(dead_code)]
fn strip_managed_attributes(
    tag: &BytesStart<'_>,
    target_path: &Path,
) -> Result<BytesStart<'static>, Error> {
    let mut new_tag =
        BytesStart::new(String::from_utf8_lossy(tag.name().into_inner()).into_owned());
    for attr_res in tag.attributes() {
        let attr = attr_res.map_err(|e| Error::XmlParse {
            path: target_path.to_path_buf(),
            message: e.to_string(),
            state: "ProcessingAttributes".to_string(),
        })?;
        if !is_managed_tag(attr.key.into_inner()) {
            new_tag.push_attribute(attr);
        }
    }
    Ok(new_tag)
}
