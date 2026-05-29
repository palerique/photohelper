//! `VerifiedModelBytes` — SHA-256-verified ONNX model bytes.

use std::path::Path;
use std::sync::Arc;

use crate::error::Error;

/// The model slug for the NIMA aesthetic scorer (catalog `model_slug` column).
///
/// Defined here (in `photohelper-ai`, alongside the scorer) rather than in
/// the CLI command layer, so the slug travels with the model definition when
/// a second scorer is added. (R2-M4 remediation)
pub const MODEL_SLUG: &str = "nima-aesthetic-v1";

/// The ONNX filename stem and `manifest.toml` section name for the NIMA model.
///
/// `VerifiedModelBytes::from_manifest(dir, MODEL_MANIFEST_NAME)` loads the model.
/// The manifest section `[nima_mobilenet_aesthetic]` specifies the actual filename.
pub const MODEL_MANIFEST_NAME: &str = "nima_mobilenet_aesthetic";

/// Model slug for the CLIP ViT-B/32 LAION2B image embedder (catalog `model_slug` column).
pub const CLIP_MODEL_SLUG: &str = "clip-vit-b32-laion2b-v1";

/// Manifest.toml section name for the CLIP model (matches filename stem before `_int8.onnx`).
pub const CLIP_MODEL_MANIFEST_NAME: &str = "clip_vit_b32_laion2b";

/// SHA-256-verified ONNX model bytes.
///
/// Constructed by reading the model file from disk and verifying its SHA-256
/// against `manifest.toml`. Workers clone the `Arc<[u8]>` interior cheaply
/// to build per-thread `ort::Session` instances.
#[derive(Clone)]
pub struct VerifiedModelBytes {
    bytes: Arc<[u8]>,
    /// Model name (without `.onnx` extension) — used in error messages.
    // name is read by Nima::new via the name() accessor for error diagnostics.
    #[allow(
        dead_code,
        reason = "used by Nima::new for error-message path construction"
    )]
    name: String,
}

impl VerifiedModelBytes {
    /// Load and SHA-256-verify a model from `model_dir`.
    ///
    /// Reads the file specified by the `filename` field in `{model_dir}/manifest.toml`
    /// under the `[{name}]` section (falling back to `{name}.onnx` if absent), then
    /// verifies its SHA-256 against the `sha256` field in the same section.
    ///
    /// # Errors
    ///
    /// - `ManifestNotFound` if `manifest.toml` is missing
    /// - `ManifestParse` if manifest.toml cannot be parsed or SHA-256 field is absent
    /// - `ModelSha256Mismatch` if SHA-256 of the file does not match the manifest
    /// - `ManifestParse` if the ONNX file cannot be read (wrapped for uniformity)
    pub fn from_manifest(model_dir: &Path, name: &str) -> Result<Self, Error> {
        let manifest_path = model_dir.join("manifest.toml");
        if !manifest_path.exists() {
            return Err(Error::ManifestNotFound {
                path: manifest_path,
            });
        }

        let manifest_text =
            std::fs::read_to_string(&manifest_path).map_err(|e| Error::ManifestParse {
                path: manifest_path.clone(),
                source: Box::new(e),
            })?;

        let expected_sha256 =
            extract_sha256(&manifest_text, name).ok_or_else(|| Error::ManifestParse {
                path: manifest_path.clone(),
                source: format!("missing [{name}].sha256 in manifest.toml").into(),
            })?;

        // Use the `filename` field from the manifest section if present (allows suffixes
        // like `_int8.onnx`); fall back to `{name}.onnx` for backward compatibility.
        let filename =
            extract_filename(&manifest_text, name).unwrap_or_else(|| format!("{name}.onnx"));
        let onnx_path = model_dir.join(&filename);
        let bytes = std::fs::read(&onnx_path).map_err(|e| Error::ManifestParse {
            path: onnx_path.clone(),
            source: Box::new(e),
        })?;

        let actual_sha256 = sha256_hex(&bytes);
        if actual_sha256 != expected_sha256 {
            return Err(Error::ModelSha256Mismatch {
                name: name.to_owned(),
                expected: expected_sha256,
                actual: actual_sha256,
            });
        }

        Ok(Self {
            bytes: Arc::from(bytes.as_slice()),
            name: name.to_owned(),
        })
    }

    /// Returns the raw model bytes (the inner `Arc<[u8]>` is cloned cheaply).
    pub(crate) fn bytes(&self) -> Arc<[u8]> {
        Arc::clone(&self.bytes)
    }

    /// Returns the model name (without `.onnx` extension).
    #[allow(
        dead_code,
        reason = "used by Nima::new for error-message path construction"
    )]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// Extract a string field from a named TOML manifest section.
///
/// Minimal TOML parser: looks for `[name]` sections then `key = "..."` within them.
fn extract_field(toml: &str, section: &str, key: &str) -> Option<String> {
    let section_header = format!("[{section}]");
    let mut in_section = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section_header;
        } else if in_section {
            if let Some(rest) = trimmed.strip_prefix(key) {
                // strip_prefix('=') may be None for prefix-matching lines (e.g., "sha256_extra").
                // Use if-let instead of ? so we continue to the next line rather than
                // returning None from the function.
                if let Some(rest) = rest.trim().strip_prefix('=') {
                    let val = rest.trim().trim_matches('"');
                    if !val.is_empty() {
                        return Some(val.to_owned());
                    }
                }
            }
        }
    }
    None
}

/// Extract the `sha256` field from a named manifest section.
fn extract_sha256(toml: &str, name: &str) -> Option<String> {
    extract_field(toml, name, "sha256")
}

/// Extract the optional `filename` field from a named manifest section.
/// Returns `None` if the field is absent (caller uses `{name}.onnx` as fallback).
fn extract_filename(toml: &str, name: &str) -> Option<String> {
    extract_field(toml, name, "filename")
}

/// Compute the lowercase hex SHA-256 of `data`.
fn sha256_hex(data: &[u8]) -> String {
    use std::fmt::Write as _;
    let hash = sha2_digest(data);
    let mut out = String::with_capacity(64);
    for byte in hash {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Minimal SHA-256 implementation using the `sha2` crate available in the
/// build-script dep (workspace-level dep). We use it directly here rather
/// than pulling the `sha2` crate into the main dep list.
fn sha2_digest(data: &[u8]) -> [u8; 32] {
    // sha2 is a workspace dep (used by photohelper-raw's build.rs); we pull
    // it into photohelper-ai here for the SHA-256 check.
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

static_assertions::assert_impl_all!(VerifiedModelBytes: Send, Sync);

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    const SAMPLE_TOML: &str = r#"
[nima_model]
filename = "nima_model.onnx"
sha256   = "abc123"
source_license = "Apache-2.0"

[clip_model]
sha256 = "def456"
"#;

    #[test]
    fn extract_field_happy_path() {
        let sha = extract_field(SAMPLE_TOML, "nima_model", "sha256").unwrap();
        assert_eq!(sha, "abc123");
        let filename = extract_field(SAMPLE_TOML, "nima_model", "filename").unwrap();
        assert_eq!(filename, "nima_model.onnx");
    }

    #[test]
    fn extract_field_missing_section_returns_none() {
        assert!(extract_field(SAMPLE_TOML, "no_such_section", "sha256").is_none());
    }

    #[test]
    fn extract_field_missing_key_returns_none() {
        // "clip_model" section exists but has no "filename" field.
        assert!(extract_field(SAMPLE_TOML, "clip_model", "filename").is_none());
    }

    #[test]
    fn extract_field_no_filename_fallback_produces_name_dot_onnx() {
        // extract_filename returns None for clip_model → caller uses {name}.onnx.
        let filename = extract_filename(SAMPLE_TOML, "clip_model");
        assert!(filename.is_none(), "clip_model has no filename field");
        let fallback = filename.unwrap_or_else(|| "clip_model.onnx".to_owned());
        assert_eq!(fallback, "clip_model.onnx");
    }

    #[test]
    fn extract_sha256_wrapper() {
        assert_eq!(extract_sha256(SAMPLE_TOML, "clip_model").unwrap(), "def456");
    }

    #[test]
    fn extract_field_key_prefix_disambiguation() {
        // "sha256" should not false-match "sha256_extra" (guarded by strip_prefix('=')).
        let toml = "[s]\nsha256_extra = \"bad\"\nsha256 = \"good\"\n";
        assert_eq!(extract_field(toml, "s", "sha256").unwrap(), "good");
    }
}
