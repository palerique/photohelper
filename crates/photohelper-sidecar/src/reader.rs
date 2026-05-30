//! XMP sidecar reader (lenient — malformed field values are not fatal).

use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::Error;
use crate::settings::{ParsedFields, Rating, SidecarSettings};

/// Read an existing XMP sidecar from `path`.
///
/// **Lenient read**: the document is parsed event-by-event. Unknown field names
/// are silently ignored (forward-compatibility). Known fields with malformed
/// values are treated as `None` with a `tracing::warn!` — the read succeeds
/// with partial data. A structurally malformed XML document (unclosed tags,
/// bad encoding) returns [`Error::XmlParse`].
///
/// # Errors
///
/// - [`Error::Io`] if `path` cannot be read.
/// - [`Error::XmlParse`] if the XML structure is malformed (not just a bad value).
pub fn read_xmp(path: &Path) -> Result<SidecarSettings, Error> {
    let content = std::fs::read_to_string(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    parse_xmp_str(&content, path)
}

fn match_prefix(prefix: Option<&str>, expected: &str) -> bool {
    match prefix {
        Some(p) => p.eq_ignore_ascii_case(expected),
        None => true, // allow prefix-less
    }
}

fn split_tag(tag: &str) -> (Option<&str>, &str) {
    if let Some(pos) = tag.find(':') {
        (Some(&tag[..pos]), &tag[pos + 1..])
    } else {
        (None, tag)
    }
}

fn parse_description_attrs<B: std::io::BufRead>(
    fields: &mut ParsedFields,
    metadata_date: &mut Option<OffsetDateTime>,
    e: &quick_xml::events::BytesStart<'_>,
    reader: &Reader<B>,
    path: &Path,
) -> Result<(), Error> {
    for attr_result in e.attributes() {
        let attr = match attr_result {
            Ok(a) => a,
            Err(err) => {
                return Err(Error::XmlParse {
                    path: path.to_path_buf(),
                    message: format!("attribute parse error: {err}"),
                });
            }
        };
        let key = attr.key;
        let key_str = std::str::from_utf8(key.as_ref()).unwrap_or_default();
        let val_result = attr.decode_and_unescape_value(reader.decoder());
        let val = match val_result {
            Ok(v) => v.into_owned(),
            Err(e) => {
                tracing::warn!(key = key_str, error = %e, "failed to decode/unescape attribute value; skipping");
                continue;
            }
        };

        if key_str == "xmlns" || key_str.starts_with("xmlns:") {
            continue;
        }

        // Track whether ANY crs: attribute was seen (Themes A+B fix):
        // this guards the conflict resolver's (None,None) branch against
        // overwriting sidecars with only untracked crs: attrs like
        // crs:WhiteBalance or crs:CameraProfile.
        if key_str.starts_with("crs:") {
            fields.has_any_crs_attr = true;
        }

        // Match on prefix-stripped local attribute names (prefix-independence) with prefix validation
        let (prefix, local_key) = split_tag(key_str);
        match local_key {
            "MetadataDate" if match_prefix(prefix, "xmp") => {
                // Store separately from ph:LastProcessedAt so the conflict
                // resolver can compare "external edit time" vs "our last
                // write time" independently (Theme A fix).
                *metadata_date = parse_datetime(&val, "MetadataDate");
            }
            "Temperature" if match_prefix(prefix, "crs") => {
                fields.temperature = parse_i32(&val, "Temperature");
            }
            "Tint" if match_prefix(prefix, "crs") => {
                fields.tint = parse_i32(&val, "Tint");
            }
            "Exposure2012" if match_prefix(prefix, "crs") => {
                fields.exposure = parse_f32(&val, "Exposure2012");
            }
            "Contrast2012" if match_prefix(prefix, "crs") => {
                fields.contrast = parse_i32(&val, "Contrast2012");
            }
            "Highlights2012" if match_prefix(prefix, "crs") => {
                fields.highlights = parse_i32(&val, "Highlights2012");
            }
            "Shadows2012" if match_prefix(prefix, "crs") => {
                fields.shadows = parse_i32(&val, "Shadows2012");
            }
            "NimaScore" if match_prefix(prefix, "ph") => {
                fields.nima_score = parse_f32(&val, "NimaScore");
            }
            "DedupClusterId" if match_prefix(prefix, "ph") => {
                fields.dedup_cluster_id = parse_i64(&val, "DedupClusterId");
            }
            "PhotohelperId" if match_prefix(prefix, "ph") => {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    fields.photohelper_id = Some(trimmed.to_string());
                }
            }
            "LastProcessedAt" if match_prefix(prefix, "ph") => {
                fields.last_processed_at = parse_datetime(&val, "LastProcessedAt");
            }
            "Rating" if match_prefix(prefix, "xmp") => {
                if let Some(r) = parse_i32(&val, "Rating") {
                    if let Ok(rating) = Rating::try_from(r) {
                        fields.rating = Some(rating);
                    } else {
                        tracing::warn!(value = r, "invalid rating value; ignoring");
                    }
                }
            }
            "Label" if match_prefix(prefix, "xmp") => {
                fields.label = Some(val.to_string());
            }
            _ => {}
        }
    }
    Ok(())
}

/// Parse XMP from a string (public for testing without disk I/O).
pub(crate) fn parse_xmp_str(content: &str, path: &Path) -> Result<SidecarSettings, Error> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut fields = ParsedFields::default();
    // xmp:MetadataDate is stored separately for conflict detection; NOT a fallback
    // for ph:LastProcessedAt. See end of function for the storage rationale.
    let mut metadata_date: Option<OffsetDateTime> = None;
    let mut tag_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let qname = e.name();
                let qname_str = std::str::from_utf8(qname.as_ref()).unwrap_or_default();
                if qname_str.starts_with("crs:") {
                    fields.has_any_crs_attr = true;
                }
                if tag_stack.len() >= 64 {
                    return Err(Error::XmlParse {
                        path: path.to_path_buf(),
                        message: "XML nesting depth exceeded safety ceiling limit (64)".to_string(),
                    });
                }
                tag_stack.push(qname_str.to_string());

                let local_bytes = qname.local_name();
                if local_bytes.as_ref() == b"Description" {
                    parse_description_attrs(&mut fields, &mut metadata_date, e, &reader, path)?;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let qname = e.name();
                let qname_str = std::str::from_utf8(qname.as_ref()).unwrap_or_default();
                if qname_str.starts_with("crs:") {
                    fields.has_any_crs_attr = true;
                }
                let local_bytes = qname.local_name();
                if local_bytes.as_ref() == b"Description" {
                    parse_description_attrs(&mut fields, &mut metadata_date, e, &reader, path)?;
                }
            }
            Ok(Event::End(_)) => {
                tag_stack.pop();
            }
            Ok(Event::Text(ref e)) => {
                let len = tag_stack.len();
                if len >= 3 {
                    if let (Some(li_tag), Some(bag_tag), Some(parent_tag)) = (
                        tag_stack.get(len - 1),
                        tag_stack.get(len - 2),
                        tag_stack.get(len - 3),
                    ) {
                        let (_, li_local) = split_tag(li_tag);
                        let (_, bag_local) = split_tag(bag_tag);
                        let (parent_prefix, parent_local) = split_tag(parent_tag);

                        if li_local == "li"
                            && (bag_local == "Bag" || bag_local == "Seq" || bag_local == "Alt")
                        {
                            if parent_local == "subject" && match_prefix(parent_prefix, "dc") {
                                match e.unescape() {
                                    Ok(text) => {
                                        let trimmed = text.trim().to_string();
                                        if !trimmed.is_empty() {
                                            fields.keywords.insert(trimmed);
                                        }
                                    }
                                    Err(err) => {
                                        tracing::warn!(error = %err, "XML unescape error for dc:subject keyword; skipping");
                                    }
                                }
                            } else if parent_local == "hierarchicalSubject"
                                && match_prefix(parent_prefix, "lr")
                            {
                                match e.unescape() {
                                    Ok(text) => {
                                        let trimmed = text.trim().to_string();
                                        if !trimmed.is_empty() {
                                            fields.hierarchical_keywords.insert(trimmed);
                                        }
                                    }
                                    Err(err) => {
                                        tracing::warn!(error = %err, "XML unescape error for lr:hierarchicalSubject keyword; skipping");
                                    }
                                }
                            }
                        }
                    }
                }
                if len > 0 {
                    if let Some(current_tag) = tag_stack.get(len - 1) {
                        let (prefix, local) = split_tag(current_tag);
                        match local {
                            "Rating" if match_prefix(prefix, "xmp") => match e.unescape() {
                                Ok(text) => {
                                    if let Some(r) = parse_i32(&text, "Rating") {
                                        if let Ok(rating) = Rating::try_from(r) {
                                            fields.rating = Some(rating);
                                        } else {
                                            tracing::warn!(
                                                value = r,
                                                "invalid rating value; ignoring"
                                            );
                                        }
                                    }
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Rating; skipping");
                                }
                            },
                            "MetadataDate" if match_prefix(prefix, "xmp") => match e.unescape() {
                                Ok(text) => {
                                    metadata_date = parse_datetime(&text, "MetadataDate");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for MetadataDate; skipping");
                                }
                            },
                            "NimaScore" if match_prefix(prefix, "ph") => match e.unescape() {
                                Ok(text) => {
                                    fields.nima_score = parse_f32(&text, "NimaScore");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for NimaScore; skipping");
                                }
                            },
                            "DedupClusterId" if match_prefix(prefix, "ph") => match e.unescape() {
                                Ok(text) => {
                                    fields.dedup_cluster_id = parse_i64(&text, "DedupClusterId");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for DedupClusterId; skipping");
                                }
                            },
                            "PhotohelperId" if match_prefix(prefix, "ph") => match e.unescape() {
                                Ok(text) => {
                                    let trimmed = text.trim();
                                    if !trimmed.is_empty() {
                                        fields.photohelper_id = Some(trimmed.to_string());
                                    }
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for PhotohelperId; skipping");
                                }
                            },
                            "LastProcessedAt" if match_prefix(prefix, "ph") => match e.unescape() {
                                Ok(text) => {
                                    fields.last_processed_at =
                                        parse_datetime(&text, "LastProcessedAt");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for LastProcessedAt; skipping");
                                }
                            },
                            "Label" if match_prefix(prefix, "xmp") => match e.unescape() {
                                Ok(text) => {
                                    fields.label = Some(text.trim().to_string());
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Label; skipping");
                                }
                            },
                            "Temperature" if match_prefix(prefix, "crs") => match e.unescape() {
                                Ok(text) => {
                                    fields.temperature = parse_i32(&text, "Temperature");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Temperature; skipping");
                                }
                            },
                            "Tint" if match_prefix(prefix, "crs") => match e.unescape() {
                                Ok(text) => {
                                    fields.tint = parse_i32(&text, "Tint");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Tint; skipping");
                                }
                            },
                            "Exposure2012" if match_prefix(prefix, "crs") => match e.unescape() {
                                Ok(text) => {
                                    fields.exposure = parse_f32(&text, "Exposure2012");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Exposure2012; skipping");
                                }
                            },
                            "Contrast2012" if match_prefix(prefix, "crs") => match e.unescape() {
                                Ok(text) => {
                                    fields.contrast = parse_i32(&text, "Contrast2012");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Contrast2012; skipping");
                                }
                            },
                            "Highlights2012" if match_prefix(prefix, "crs") => match e.unescape() {
                                Ok(text) => {
                                    fields.highlights = parse_i32(&text, "Highlights2012");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Highlights2012; skipping");
                                }
                            },
                            "Shadows2012" if match_prefix(prefix, "crs") => match e.unescape() {
                                Ok(text) => {
                                    fields.shadows = parse_i32(&text, "Shadows2012");
                                }
                                Err(err) => {
                                    tracing::warn!(error = %err, "XML unescape error for Shadows2012; skipping");
                                }
                            },
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => {
                return Err(Error::XmlParse {
                    path: path.to_path_buf(),
                    message: format!(
                        "XML parse error at position {}: {err}",
                        reader.error_position()
                    ),
                });
            }
        }
    }

    // Store xmp:MetadataDate separately for conflict resolution (Theme A fix).
    // The conflict resolver compares:
    //   existing.metadata_date()    = xmp:MetadataDate (Lightroom's write time)
    //   existing.last_processed_at() = ph:LastProcessedAt (our write time — NO fallback)
    // This correctly detects "Lightroom edited after our last develop pass."
    fields.metadata_date = metadata_date;
    // NOTE: fields.last_processed_at is populated only from ph:LastProcessedAt;
    // we do NOT fall back to xmp:MetadataDate here. The (Some(_), None) conflict
    // case maps to "existing sidecar has a date but photohelper never wrote to it."

    Ok(SidecarSettings::from_parsed(fields))
}

fn parse_i32(val: &str, field: &str) -> Option<i32> {
    val.trim()
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite() && *f >= f64::from(i32::MIN) && *f <= f64::from(i32::MAX))
        .map(|f| f.round() as i32)
        .or_else(|| {
            tracing::warn!(field, value = val, "malformed XMP field value; ignoring");
            None
        })
}

fn parse_i64(val: &str, field: &str) -> Option<i64> {
    let trimmed = val.trim();
    if let Ok(i) = trimmed.parse::<i64>() {
        return Some(i);
    }
    trimmed
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite() && *f >= i64::MIN as f64 && *f <= i64::MAX as f64)
        .map(|f| f.round() as i64)
        .or_else(|| {
            tracing::warn!(field, value = val, "malformed XMP field value; ignoring");
            None
        })
}

fn parse_f32(val: &str, field: &str) -> Option<f32> {
    val.trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .or_else(|| {
            tracing::warn!(field, value = val, "malformed XMP field value; ignoring");
            None
        })
}

fn parse_datetime(val: &str, field: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(val.trim(), &Rfc3339)
        .map_err(|_| {
            tracing::warn!(field, value = val, "malformed XMP timestamp; ignoring");
        })
        .ok()
}
