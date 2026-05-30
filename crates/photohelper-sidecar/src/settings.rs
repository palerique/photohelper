//! `SidecarSettings` and `SidecarSettingsBuilder`.
//!
//! All fields are private; callers use `SidecarSettings::builder()` to
//! construct validated instances. This mirrors the `ImageEmbedding` and
//! `Photo` pattern in the codebase.

use time::OffsetDateTime;

use crate::error::Error;

/// Intermediate struct used by the XMP reader to pass parsed fields to
/// [`SidecarSettings::from_parsed`] without exceeding the 7-argument limit.
#[derive(Default)]
pub(crate) struct ParsedFields {
    pub temperature: Option<i32>,
    pub tint: Option<i32>,
    pub exposure: Option<f32>,
    pub contrast: Option<i32>,
    pub highlights: Option<i32>,
    pub shadows: Option<i32>,
    pub nima_score: Option<f32>,
    pub dedup_cluster_id: Option<i64>,
    pub photohelper_id: Option<String>,
    pub last_processed_at: Option<OffsetDateTime>,
}

/// Develop settings mapping to `crs:` (Camera Raw) and `ph:` (photohelper)
/// XMP namespaces.
///
/// Private fields; use [`SidecarSettings::builder()`] to construct.
/// Validation runs at construction time — callers cannot construct invalid
/// settings.
///
/// **Fields removed from v0.1** (no CLI exposure; reserved for future sessions):
/// `clarity`, `vibrance`, `saturation`, `white_balance`. The XMP reader
/// silently ignores these fields in existing sidecars (forward-compat).
/// `process_version` is hardcoded as `"11.0"` in the writer.
#[derive(Debug, Clone, PartialEq)]
pub struct SidecarSettings {
    // crs: namespace
    temperature: Option<i32>,
    tint: Option<i32>,
    exposure: Option<f32>,
    contrast: Option<i32>,
    highlights: Option<i32>,
    shadows: Option<i32>,
    // ph: namespace
    nima_score: Option<f32>,
    dedup_cluster_id: Option<i64>,
    photohelper_id: Option<String>,
    last_processed_at: Option<OffsetDateTime>,
}

impl SidecarSettings {
    /// Returns a builder for constructing `SidecarSettings`.
    #[must_use]
    pub fn builder() -> SidecarSettingsBuilder {
        SidecarSettingsBuilder::default()
    }

    /// White balance temperature in Kelvin, if set.
    #[must_use]
    pub fn temperature(&self) -> Option<i32> {
        self.temperature
    }

    /// White balance tint (green/magenta), if set.
    #[must_use]
    pub fn tint(&self) -> Option<i32> {
        self.tint
    }

    /// Exposure compensation in stops, if set.
    #[must_use]
    pub fn exposure(&self) -> Option<f32> {
        self.exposure
    }

    /// Contrast adjustment, if set.
    #[must_use]
    pub fn contrast(&self) -> Option<i32> {
        self.contrast
    }

    /// Highlights adjustment, if set.
    #[must_use]
    pub fn highlights(&self) -> Option<i32> {
        self.highlights
    }

    /// Shadows adjustment, if set.
    #[must_use]
    pub fn shadows(&self) -> Option<i32> {
        self.shadows
    }

    /// NIMA aesthetic score, if set.
    #[must_use]
    pub fn nima_score(&self) -> Option<f32> {
        self.nima_score
    }

    /// Duplicate cluster ID from the catalog, if set.
    #[must_use]
    pub fn dedup_cluster_id(&self) -> Option<i64> {
        self.dedup_cluster_id
    }

    /// photohelper photo ID (43-char base64url), if set.
    #[must_use]
    pub fn photohelper_id(&self) -> Option<&str> {
        self.photohelper_id.as_deref()
    }

    /// Timestamp of the last photohelper develop pass, if set.
    #[must_use]
    pub fn last_processed_at(&self) -> Option<OffsetDateTime> {
        self.last_processed_at
    }

    /// Returns `true` if any `crs:` develop field is set.
    #[must_use]
    pub fn has_crs_fields(&self) -> bool {
        self.temperature.is_some()
            || self.tint.is_some()
            || self.exposure.is_some()
            || self.contrast.is_some()
            || self.highlights.is_some()
            || self.shadows.is_some()
    }

    /// Returns `true` if no fields (crs: or ph:) are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.has_crs_fields()
            && self.nima_score.is_none()
            && self.dedup_cluster_id.is_none()
            && self.photohelper_id.is_none()
            && self.last_processed_at.is_none()
    }

    /// Unchecked constructor used by the XMP reader (values already come
    /// from a persisted file; we trust them rather than re-validating).
    pub(crate) fn from_parsed(fields: ParsedFields) -> Self {
        Self {
            temperature: fields.temperature,
            tint: fields.tint,
            exposure: fields.exposure,
            contrast: fields.contrast,
            highlights: fields.highlights,
            shadows: fields.shadows,
            nima_score: fields.nima_score,
            dedup_cluster_id: fields.dedup_cluster_id,
            photohelper_id: fields.photohelper_id,
            last_processed_at: fields.last_processed_at,
        }
    }
}

/// Builder for [`SidecarSettings`].
///
/// Obtain via [`SidecarSettings::builder()`]. Call [`build()`][SidecarSettingsBuilder::build]
/// to validate and produce a `SidecarSettings`.
#[derive(Debug, Default)]
pub struct SidecarSettingsBuilder {
    temperature: Option<i32>,
    tint: Option<i32>,
    exposure: Option<f32>,
    contrast: Option<i32>,
    highlights: Option<i32>,
    shadows: Option<i32>,
    nima_score: Option<f32>,
    dedup_cluster_id: Option<i64>,
    photohelper_id: Option<String>,
    last_processed_at: Option<OffsetDateTime>,
}

impl SidecarSettingsBuilder {
    /// White balance temperature in Kelvin (2000–50000).
    #[must_use]
    pub fn temperature(mut self, v: i32) -> Self {
        self.temperature = Some(v);
        self
    }

    /// White balance tint (–150 to 150).
    #[must_use]
    pub fn tint(mut self, v: i32) -> Self {
        self.tint = Some(v);
        self
    }

    /// Exposure compensation in stops (–5.0 to 5.0).
    #[must_use]
    pub fn exposure(mut self, v: f32) -> Self {
        self.exposure = Some(v);
        self
    }

    /// Contrast (–100 to 100).
    #[must_use]
    pub fn contrast(mut self, v: i32) -> Self {
        self.contrast = Some(v);
        self
    }

    /// Highlights (–100 to 100).
    #[must_use]
    pub fn highlights(mut self, v: i32) -> Self {
        self.highlights = Some(v);
        self
    }

    /// Shadows (–100 to 100).
    #[must_use]
    pub fn shadows(mut self, v: i32) -> Self {
        self.shadows = Some(v);
        self
    }

    /// NIMA aesthetic score.
    #[must_use]
    pub fn nima_score(mut self, v: f32) -> Self {
        self.nima_score = Some(v);
        self
    }

    /// Duplicate cluster ID.
    #[must_use]
    pub fn dedup_cluster_id(mut self, v: i64) -> Self {
        self.dedup_cluster_id = Some(v);
        self
    }

    /// photohelper photo ID (43-char base64url string from `PhotoId`).
    #[must_use]
    pub fn photohelper_id(mut self, v: impl Into<String>) -> Self {
        self.photohelper_id = Some(v.into());
        self
    }

    /// Timestamp of the last photohelper develop pass.
    #[must_use]
    pub fn last_processed_at(mut self, v: OffsetDateTime) -> Self {
        self.last_processed_at = Some(v);
        self
    }

    /// Build and validate.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Validation`] if any field is out of its valid range.
    pub fn build(self) -> Result<SidecarSettings, Error> {
        if let Some(t) = self.temperature {
            if !(2000..=50_000).contains(&t) {
                return Err(Error::Validation {
                    message: format!("temperature {t} is outside [2000, 50000]"),
                });
            }
        }
        if let Some(t) = self.tint {
            if !(-150..=150).contains(&t) {
                return Err(Error::Validation {
                    message: format!("tint {t} is outside [-150, 150]"),
                });
            }
        }
        if let Some(e) = self.exposure {
            if !e.is_finite() || !(-5.0..=5.0).contains(&e) {
                return Err(Error::Validation {
                    message: format!("exposure {e} is outside [-5.0, 5.0]"),
                });
            }
        }
        for (name, val) in [
            ("contrast", self.contrast),
            ("highlights", self.highlights),
            ("shadows", self.shadows),
        ] {
            if let Some(v) = val {
                if !(-100..=100).contains(&v) {
                    return Err(Error::Validation {
                        message: format!("{name} {v} is outside [-100, 100]"),
                    });
                }
            }
        }
        Ok(SidecarSettings {
            temperature: self.temperature,
            tint: self.tint,
            exposure: self.exposure,
            contrast: self.contrast,
            highlights: self.highlights,
            shadows: self.shadows,
            nima_score: self.nima_score,
            dedup_cluster_id: self.dedup_cluster_id,
            photohelper_id: self.photohelper_id,
            last_processed_at: self.last_processed_at,
        })
    }
}
