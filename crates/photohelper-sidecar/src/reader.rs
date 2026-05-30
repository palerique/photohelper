//! XMP sidecar reader (lenient — malformed field values are not fatal).

use std::path::Path;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::Error;
use crate::settings::{ParsedFields, SidecarSettings};

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

/// Parse XMP from a string (public for testing without disk I/O).
pub(crate) fn parse_xmp_str(content: &str, path: &Path) -> Result<SidecarSettings, Error> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut fields = ParsedFields::default();
    // xmp:MetadataDate is used as fallback if ph:LastProcessedAt is absent.
    let mut metadata_date: Option<OffsetDateTime> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                // We only care about rdf:Description — extract its attributes.
                let name = e.name();
                let local = name.local_name();
                if local.as_ref() != b"Description" {
                    continue;
                }

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
                    let key_str = std::str::from_utf8(key.as_ref()).unwrap_or("");
                    let val = attr
                        .decode_and_unescape_value(reader.decoder())
                        .map(|v| v.into_owned())
                        .unwrap_or_default();

                    // Strip namespace prefix for matching.
                    let local_key = key_str.rfind(':').map_or(key_str, |i| &key_str[i + 1..]);

                    match local_key {
                        "MetadataDate" => {
                            metadata_date = parse_datetime(&val, "xmp:MetadataDate");
                        }
                        "Temperature" => {
                            fields.temperature = parse_i32(&val, "crs:Temperature");
                        }
                        "Tint" => {
                            fields.tint = parse_i32(&val, "crs:Tint");
                        }
                        "Exposure2012" => {
                            fields.exposure = parse_f32(&val, "crs:Exposure2012");
                        }
                        "Contrast2012" => {
                            fields.contrast = parse_i32(&val, "crs:Contrast2012");
                        }
                        "Highlights2012" => {
                            fields.highlights = parse_i32(&val, "crs:Highlights2012");
                        }
                        "Shadows2012" => {
                            fields.shadows = parse_i32(&val, "crs:Shadows2012");
                        }
                        "NimaScore" => {
                            fields.nima_score = parse_f32(&val, "ph:NimaScore");
                        }
                        "DedupClusterId" => {
                            fields.dedup_cluster_id = parse_i64(&val, "ph:DedupClusterId");
                        }
                        "PhotohelperId" => {
                            fields.photohelper_id = Some(val.to_string());
                        }
                        "LastProcessedAt" => {
                            fields.last_processed_at = parse_datetime(&val, "ph:LastProcessedAt");
                        }
                        // All other attributes (rdf:about, xmlns:*, ProcessVersion,
                        // WhiteBalance, etc.) are silently ignored.
                        _ => {}
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

    // Prefer ph:LastProcessedAt; fall back to xmp:MetadataDate.
    if fields.last_processed_at.is_none() {
        fields.last_processed_at = metadata_date;
    }

    Ok(SidecarSettings::from_parsed(fields))
}

fn parse_i32(val: &str, field: &str) -> Option<i32> {
    val.trim()
        .parse::<i32>()
        .map_err(|_| {
            tracing::warn!(field, value = val, "malformed XMP field value; ignoring");
        })
        .ok()
}

fn parse_i64(val: &str, field: &str) -> Option<i64> {
    val.trim()
        .parse::<i64>()
        .map_err(|_| {
            tracing::warn!(field, value = val, "malformed XMP field value; ignoring");
        })
        .ok()
}

fn parse_f32(val: &str, field: &str) -> Option<f32> {
    val.trim()
        .parse::<f32>()
        .map_err(|_| {
            tracing::warn!(field, value = val, "malformed XMP field value; ignoring");
        })
        .ok()
        .filter(|v| v.is_finite())
}

fn parse_datetime(val: &str, field: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(val.trim(), &Rfc3339)
        .map_err(|_| {
            tracing::warn!(field, value = val, "malformed XMP timestamp; ignoring");
        })
        .ok()
}
