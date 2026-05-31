import re

with open("crates/photohelper-sidecar/src/settings.rs", "r") as f:
    content = f.read()

# Add Update enum
update_enum = """
/// Represents an explicit update instruction for a sidecar field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update<T> {
    /// Keep the existing value (no-op during merge).
    Keep,
    /// Clear (delete) the existing value.
    Clear,
    /// Set a new value.
    Set(T),
}

impl<T> Default for Update<T> {
    fn default() -> Self {
        Self::Keep
    }
}

impl<T> Update<T> {
    /// Returns `Some(&v)` if `Set`, otherwise `None`.
    pub fn as_option(&self) -> Option<&T> {
        match self {
            Self::Set(v) => Some(v),
            _ => None,
        }
    }

    /// Resolves this update against an existing absolute value.
    pub fn resolve(self, existing: Option<T>) -> Option<T> {
        match self {
            Update::Keep => existing,
            Update::Clear => None,
            Update::Set(v) => Some(v),
        }
    }
}

impl<T> From<Option<T>> for Update<T> {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => Self::Set(v),
            None => Self::Clear,
        }
    }
}
"""

content = content.replace("use crate::error::Error;", "use crate::error::Error;\n" + update_enum)

# Replace Option<T> with Update<T> for specific fields in SidecarSettings
content = re.sub(r"pub struct SidecarSettings \{(.*?)\}", lambda m: "pub struct SidecarSettings {" + m.group(1).replace("nima_score: Option<f32>,", "nima_score: Update<f32>,").replace("dedup_cluster_id: Option<i64>,", "dedup_cluster_id: Update<i64>,").replace("photohelper_id: Option<String>,", "photohelper_id: Update<String>,") + "}", content, flags=re.DOTALL)

# In merge
merge_repl = """        let nima_score = incoming.nima_score.clone().resolve(self.nima_score.clone().as_option().copied()).into();
        let dedup_cluster_id = incoming.dedup_cluster_id.clone().resolve(self.dedup_cluster_id.clone().as_option().copied()).into();
        let photohelper_id = incoming.photohelper_id.clone().resolve(self.photohelper_id.clone().as_option().cloned()).into();"""
content = re.sub(r"        let nima_score = incoming\.nima_score\.or\(self\.nima_score\);\n        let dedup_cluster_id = incoming\.dedup_cluster_id\.or\(self\.dedup_cluster_id\);\n        let photohelper_id = incoming\n            \.photohelper_id\n            \.clone\(\)\n            \.or_else\(\|\| self\.photohelper_id\.clone\(\)\);", merge_repl, content)

# In from_parsed
from_parsed_repl = """            auto_tone: fields.auto_tone,
            nima_score: fields.nima_score.into(),
            dedup_cluster_id: fields.dedup_cluster_id.into(),
            photohelper_id: fields.photohelper_id.into(),
            last_processed_at: fields.last_processed_at,"""
content = re.sub(r"            auto_tone: fields\.auto_tone,\n            nima_score,\n            dedup_cluster_id,\n            photohelper_id: fields\.photohelper_id,\n            last_processed_at: fields\.last_processed_at,", from_parsed_repl, content)

content = content.replace("let nima_score = fields.nima_score;", "")
content = content.replace("let dedup_cluster_id = fields.dedup_cluster_id;", "")

# In SidecarSettingsBuilder
content = re.sub(r"pub struct SidecarSettingsBuilder \{(.*?)\}", lambda m: "pub struct SidecarSettingsBuilder {" + m.group(1).replace("nima_score: Option<f32>,", "nima_score: Update<f32>,").replace("dedup_cluster_id: Option<i64>,", "dedup_cluster_id: Update<i64>,").replace("photohelper_id: Option<String>,", "photohelper_id: Update<String>,") + "}", content, flags=re.DOTALL)

builder_impl = """
    pub fn nima_score(mut self, v: f32) -> Self {
        self.nima_score = Update::Set(v);
        self
    }
    pub fn clear_nima_score(mut self) -> Self {
        self.nima_score = Update::Clear;
        self
    }

    pub fn dedup_cluster_id(mut self, v: i64) -> Self {
        self.dedup_cluster_id = Update::Set(v);
        self
    }
    pub fn clear_dedup_cluster_id(mut self) -> Self {
        self.dedup_cluster_id = Update::Clear;
        self
    }

    pub fn photohelper_id(mut self, v: impl Into<String>) -> Self {
        self.photohelper_id = Update::Set(v.into());
        self
    }
    pub fn clear_photohelper_id(mut self) -> Self {
        self.photohelper_id = Update::Clear;
        self
    }
"""

content = re.sub(r"    pub fn nima_score\(mut self, v: f32\) -> Self \{.*?(?=    /// Duplicate cluster ID)", builder_impl.split("    pub fn dedup")[0], content, flags=re.DOTALL)
content = re.sub(r"    pub fn dedup_cluster_id\(mut self, v: i64\) -> Self \{.*?(?=    /// photohelper photo ID)", "    pub fn dedup" + builder_impl.split("    pub fn dedup")[1].split("    pub fn photohelper_id")[0], content, flags=re.DOTALL)
content = re.sub(r"    pub fn photohelper_id\(mut self, v: impl Into<String>\) -> Self \{.*?(?=    /// Timestamp of the last)", "    pub fn photohelper_id" + builder_impl.split("    pub fn photohelper_id")[1], content, flags=re.DOTALL)

# Build validation
content = content.replace("if let Some(s) = self.nima_score {", "if let Some(&s) = self.nima_score.as_option() {")
content = content.replace("if let Some(c) = self.dedup_cluster_id {", "if let Some(&c) = self.dedup_cluster_id.as_option() {")
content = content.replace("if let Some(pid) = &self.photohelper_id {", "if let Some(pid) = self.photohelper_id.as_option() {")

# Getters
content = content.replace("pub fn nima_score(&self) -> Option<f32> {\n        self.nima_score\n    }", "pub fn nima_score(&self) -> Option<f32> {\n        self.nima_score.as_option().copied()\n    }")
content = content.replace("pub fn dedup_cluster_id(&self) -> Option<i64> {\n        self.dedup_cluster_id\n    }", "pub fn dedup_cluster_id(&self) -> Option<i64> {\n        self.dedup_cluster_id.as_option().copied()\n    }")
content = content.replace("pub fn photohelper_id(&self) -> Option<&str> {\n        self.photohelper_id.as_deref()\n    }", "pub fn photohelper_id(&self) -> Option<&str> {\n        self.photohelper_id.as_option().map(|s| s.as_str())\n    }")

# empty check
content = content.replace("&& self.nima_score.is_none()", "&& matches!(self.nima_score, Update::Keep | Update::Clear)")
content = content.replace("&& self.dedup_cluster_id.is_none()", "&& matches!(self.dedup_cluster_id, Update::Keep | Update::Clear)")
content = content.replace("&& self.photohelper_id.is_none()", "&& matches!(self.photohelper_id, Update::Keep | Update::Clear)")


with open("crates/photohelper-sidecar/src/settings.rs", "w") as f:
    f.write(content)

print("done")
