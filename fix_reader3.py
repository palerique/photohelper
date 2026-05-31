with open("crates/photohelper-sidecar/src/reader.rs", "r") as f:
    text = f.read()

text = text.replace(
"""fn apply_parsed_field(
    fields: &mut ParsedFields,
    metadata_date: &mut Option<OffsetDateTime>,
    prefix: Option<&str>,
    local_key: &[u8],
    text: &str,
    path: &Path,
) -> Result<(), Error> {""",
"""#[allow(clippy::unnecessary_wraps)]
fn apply_parsed_field(
    fields: &mut ParsedFields,
    metadata_date: &mut Option<OffsetDateTime>,
    prefix: Option<&str>,
    local_key: &[u8],
    text: &str,
    path: &Path,
) -> Result<(), Error> {"""
)

# Also fix the parse_datetime return type issue which we fixed previously
text = text.replace("parse_datetime(text, \"MetadataDate\", path)?", "parse_datetime(text, \"MetadataDate\", path)")
text = text.replace("fields.last_processed_at = parse_datetime(text, \"LastProcessedAt\", path)?", "fields.last_processed_at = parse_datetime(text, \"LastProcessedAt\", path)")

# Fix the nested or-patterns, etc which were wiped when I restored
text = text.replace(
    "Ok(event_text_or_cdata @ Event::Text(_))\n            | Ok(event_text_or_cdata @ Event::CData(_)) => {",
    "Ok(event_text_or_cdata @ (Event::Text(_) | Event::CData(_))) => {"
)
text = text.replace(
"""    match val.parse::<i32>() {
        Ok(v) => Some(v),
        Err(_) => {
            tracing::warn!(path = %path.display(), value = val, "invalid {field} value; ignoring");
            None
        }
    }""",
"""    if let Ok(v) = val.parse::<i32>() {
        Some(v)
    } else {
        tracing::warn!(path = %path.display(), value = val, "invalid {field} value; ignoring");
        None
    }"""
)
text = text.replace(
"""    match val.parse::<i64>() {
        Ok(v) => Some(v),
        Err(_) => {
            tracing::warn!(path = %path.display(), value = val, "invalid {field} value; ignoring");
            None
        }
    }""",
"""    if let Ok(v) = val.parse::<i64>() {
        Some(v)
    } else {
        tracing::warn!(path = %path.display(), value = val, "invalid {field} value; ignoring");
        None
    }"""
)
text = text.replace(
    "fn parse_datetime(val: &str, field: &str, path: &Path) -> Result<Option<OffsetDateTime>, Error> {",
    "fn parse_datetime(val: &str, field: &str, path: &Path) -> Option<OffsetDateTime> {"
)
text = text.replace(
"""    match OffsetDateTime::parse(val, &Rfc3339) {
        Ok(dt) => Ok(Some(dt)),
        Err(e) => {
            tracing::warn!(path = %path.display(), value = val, error = %e, "invalid {field} RFC3339 timestamp; ignoring");
            Ok(None)
        }
    }""",
"""    if let Ok(dt) = OffsetDateTime::parse(val, &Rfc3339) {
        Some(dt)
    } else {
        tracing::warn!(path = %path.display(), value = val, "invalid {field} RFC3339 timestamp; ignoring");
        None
    }"""
)

text = text.replace("parse_datetime(val, \"crs:MetadataDate\", path)?", "parse_datetime(val, \"crs:MetadataDate\", path)")
text = text.replace("parse_datetime(val, \"ph:LastProcessedAt\", path)?", "parse_datetime(val, \"ph:LastProcessedAt\", path)")


with open("crates/photohelper-sidecar/src/reader.rs", "w") as f:
    f.write(text)
