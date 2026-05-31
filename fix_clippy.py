import re

with open("crates/photohelper-sidecar/src/reader.rs", "r") as f:
    reader = f.read()

reader = reader.replace(
    "Ok(event_text_or_cdata @ Event::Text(_))\n            | Ok(event_text_or_cdata @ Event::CData(_)) => {",
    "Ok(event_text_or_cdata @ (Event::Text(_) | Event::CData(_))) => {"
)

reader = reader.replace(
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

reader = reader.replace(
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

reader = reader.replace(
    "fn parse_datetime(val: &str, field: &str, path: &Path) -> Result<Option<OffsetDateTime>, Error> {",
    "fn parse_datetime(val: &str, field: &str, path: &Path) -> Option<OffsetDateTime> {"
)
reader = reader.replace(
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
# Fix the two call sites of parse_datetime
reader = reader.replace("parse_datetime(val, \"crs:MetadataDate\", path)?", "parse_datetime(val, \"crs:MetadataDate\", path)")
reader = reader.replace("parse_datetime(val, \"ph:LastProcessedAt\", path)?", "parse_datetime(val, \"ph:LastProcessedAt\", path)")


with open("crates/photohelper-sidecar/src/reader.rs", "w") as f:
    f.write(reader)

with open("crates/photohelper-sidecar/src/settings.rs", "r") as f:
    settings = f.read()

settings = settings.replace(
"""fn merge_keywords(
    existing: &Option<BTreeSet<String>>,
    incoming: &Option<BTreeSet<String>>,
) -> Option<BTreeSet<String>> {""",
"""fn merge_keywords(
    existing: Option<&BTreeSet<String>>,
    incoming: Option<&BTreeSet<String>>,
) -> Option<BTreeSet<String>> {"""
)
settings = settings.replace("merge_keywords(&self.keywords, &incoming.keywords)", "merge_keywords(self.keywords.as_ref(), incoming.keywords.as_ref())")
settings = settings.replace("merge_keywords(&self.hierarchical_keywords, &incoming.hierarchical_keywords)", "merge_keywords(self.hierarchical_keywords.as_ref(), incoming.hierarchical_keywords.as_ref())")

settings = settings.replace("#[must_use]\n\n    pub fn nima_score", "#[must_use]\n    pub fn nima_score")
settings = settings.replace("pub fn clear_nima_score", "#[must_use]\n    pub fn clear_nima_score")
settings = settings.replace("pub fn clear_dedup_cluster_id", "#[must_use]\n    pub fn clear_dedup_cluster_id")
settings = settings.replace("pub fn clear_photohelper_id", "#[must_use]\n    pub fn clear_photohelper_id")

with open("crates/photohelper-sidecar/src/settings.rs", "w") as f:
    f.write(settings)

with open("crates/photohelper-sidecar/src/writer.rs", "r") as f:
    writer = f.read()
writer = writer.replace("perms.set_readonly(false);", "#[allow(clippy::permissions_set_readonly_false)]\n                perms.set_readonly(false);")
with open("crates/photohelper-sidecar/src/writer.rs", "w") as f:
    f.write(writer)
