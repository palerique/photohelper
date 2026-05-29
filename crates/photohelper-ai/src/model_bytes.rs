//! `VerifiedModelBytes` — SHA-256-verified ONNX model bytes.

use std::path::Path;
use std::sync::Arc;

use crate::error::Error;

/// The model slug for the NIMA aesthetic scorer.
///
/// Defined here (in `photohelper-ai`, alongside the scorer) rather than in
/// the CLI command layer, so the slug travels with the model definition when
/// a second scorer is added. (R2-M4 remediation)
pub const MODEL_SLUG: &str = "nima-aesthetic-v1";

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
    /// Reads `{model_dir}/{name}.onnx` and verifies its SHA-256 against the
    /// `sha256` field in `{model_dir}/manifest.toml` under the `[{name}]` section.
    ///
    /// # Errors
    ///
    /// - `ManifestNotFound` if `manifest.toml` is missing
    /// - `ManifestParse` if manifest.toml cannot be parsed
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

        let onnx_path = model_dir.join(format!("{name}.onnx"));
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

/// Extract the sha256 value from a TOML manifest for a named section.
///
/// Minimal TOML parser: just looks for `[name]` sections and `sha256 = "..."` keys.
fn extract_sha256(toml: &str, name: &str) -> Option<String> {
    let section_header = format!("[{name}]");
    let mut in_section = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == section_header;
        } else if in_section {
            if let Some(rest) = trimmed.strip_prefix("sha256") {
                let rest = rest.trim().strip_prefix('=')?;
                let val = rest.trim().trim_matches('"');
                if !val.is_empty() {
                    return Some(val.to_owned());
                }
            }
        }
    }
    None
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
