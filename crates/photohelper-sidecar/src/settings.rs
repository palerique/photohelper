//! `SidecarSettings` and `SidecarSettingsBuilder`.
//!
//! All fields are private; callers use `SidecarSettings::builder()` to
//! construct validated instances. This mirrors the `ImageEmbedding` and
//! `Photo` pattern in the codebase.

use std::collections::BTreeSet;
use time::OffsetDateTime;

use crate::error::Error;

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

/// Case-insensitive, char-boundary-safe prefix checking helper.
fn strip_prefix_ignore_ascii_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.get(..prefix.len())
        .filter(|h| h.eq_ignore_ascii_case(prefix))
        .and_then(|_| s.get(prefix.len()..))
}

/// Helper function to precisely identify photohelper-managed keywords so they
/// can be stripped during sidecar merging.
fn is_photohelper_keyword(kw: &str) -> bool {
    if kw.eq_ignore_ascii_case("photohelper") {
        return true;
    }

    // Check for "photohelper|" or "photohelper:" prefixes
    if let Some(rest) = strip_prefix_ignore_ascii_case(kw, "photohelper") {
        let mut chars = rest.chars();
        if let Some(sep) = chars.next() {
            if sep == '|' || sep == ':' {
                return true;
            }
        }
    }

    is_ph_suffix(kw)
}

fn is_ph_suffix(suffix: &str) -> bool {
    if let Some(rest) = strip_prefix_ignore_ascii_case(suffix, "cluster:") {
        if let Ok(id) = rest.parse::<i64>() {
            if id >= 0 {
                return true;
            }
        }
    }
    if let Some(rest) = strip_prefix_ignore_ascii_case(suffix, "nima:") {
        if rest.eq_ignore_ascii_case("discard")
            || rest.eq_ignore_ascii_case("poor")
            || rest.eq_ignore_ascii_case("fair")
            || rest.eq_ignore_ascii_case("good")
            || rest.eq_ignore_ascii_case("excellent")
        {
            return true;
        }
    }
    false
}

fn merge_keywords(
    existing: Option<&BTreeSet<String>>,
    incoming: Option<&BTreeSet<String>>,
) -> Option<BTreeSet<String>> {
    match incoming {
        None => existing.cloned(),
        Some(incoming_set) => {
            let mut user_kws: BTreeSet<String> = existing
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|k| !is_photohelper_keyword(k))
                .collect();
            user_kws.extend(incoming_set.iter().cloned());
            Some(user_kws)
        }
    }
}

/// Strongly-typed star rating state inside sidecar metadata.
/// Supported range is `[-1, 5]`, where `-1` represents Rejected,
/// `0` represents Unrated/None, and `[1, 5]` are standard stars.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Rating {
    /// Rejected flag (-1).
    Rejected = -1,
    /// Unrated (0 stars).
    Unrated = 0,
    /// 1 star.
    One = 1,
    /// 2 stars.
    Two = 2,
    /// 3 stars.
    Three = 3,
    /// 4 stars.
    Four = 4,
    /// 5 stars.
    Five = 5,
}

impl Rating {
    /// Convert Rating to raw i32.
    #[must_use]
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

impl TryFrom<i32> for Rating {
    type Error = String;

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            -1 => Ok(Rating::Rejected),
            0 => Ok(Rating::Unrated),
            1 => Ok(Rating::One),
            2 => Ok(Rating::Two),
            3 => Ok(Rating::Three),
            4 => Ok(Rating::Four),
            5 => Ok(Rating::Five),
            _ => Err(format!("invalid rating value: {v}")),
        }
    }
}

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
    pub auto_tone: Option<bool>,
    pub nima_score: Option<f32>,
    pub dedup_cluster_id: Option<i64>,
    pub photohelper_id: Option<String>,
    pub last_processed_at: Option<OffsetDateTime>,
    /// The raw `xmp:MetadataDate` value from the sidecar (NOT collapsed with
    /// `ph:LastProcessedAt`). Used by the conflict resolver to detect external
    /// edits: if `metadata_date > last_processed_at`, a third-party tool (e.g.
    /// Lightroom) wrote to the sidecar after our last develop pass.
    pub metadata_date: Option<OffsetDateTime>,
    /// True if ANY `crs:` attribute was encountered in the parsed XML,
    /// regardless of whether its value was successfully parsed. Prevents
    /// the conflict resolver from overwriting sidecars that contain only
    /// untracked `crs:` attributes (e.g. `crs:WhiteBalance`, `crs:CameraProfile`).
    pub has_any_crs_attr: bool,
    /// Standard metadata fields.
    pub rating: Option<Rating>,
    pub label: Option<String>,
    pub keywords: BTreeSet<String>,
    pub hierarchical_keywords: BTreeSet<String>,
}

/// Develop settings mapping to `crs:` (Camera Raw), `ph:` (photohelper),
/// and standard XMP namespaces.
///
/// Private fields; use [`SidecarSettings::builder()`] to construct.
/// Validation runs at construction time — callers cannot construct invalid
/// settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SidecarSettings {
    // crs: namespace
    temperature: Option<i32>,
    tint: Option<i32>,
    exposure: Option<f32>,
    contrast: Option<i32>,
    highlights: Option<i32>,
    shadows: Option<i32>,
    auto_tone: Option<bool>,
    // ph: namespace
    nima_score: Update<f32>,
    dedup_cluster_id: Update<i64>,
    photohelper_id: Update<String>,
    last_processed_at: Option<OffsetDateTime>,
    // Conflict-resolution metadata (reader-only, not set by builder).
    /// The raw `xmp:MetadataDate` from the parsed sidecar; kept separate from
    /// `ph:LastProcessedAt` so the conflict resolver can compare "external edit
    /// time" vs "our last write time" independently.
    metadata_date: Option<OffsetDateTime>,
    /// True if any `crs:` attribute or element was present in the parsed XML, even if not
    /// numerically parsed. Guards the (None,None) conflict path against overwriting
    /// sidecars with untracked `crs:` settings (e.g. `crs:WhiteBalance`).
    has_any_crs_attr: bool,
    // Standard namespaces (dc:, lr:, xmp:)
    rating: Option<Rating>,
    label: Option<String>,
    keywords: Option<BTreeSet<String>>,
    hierarchical_keywords: Option<BTreeSet<String>>,
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

    /// Auto Tone enablement, if set.
    #[must_use]
    pub fn auto_tone(&self) -> Option<bool> {
        self.auto_tone
    }

    /// NIMA aesthetic score, if set.
    #[must_use]
    pub fn nima_score(&self) -> Option<f32> {
        self.nima_score.as_option().copied()
    }

    /// Duplicate cluster ID from the catalog, if set.
    #[must_use]
    pub fn dedup_cluster_id(&self) -> Option<i64> {
        self.dedup_cluster_id.as_option().copied()
    }

    /// photohelper photo ID (43-char base64url), if set.
    #[must_use]
    pub fn photohelper_id(&self) -> Option<&str> {
        self.photohelper_id.as_option().map(|s| s.as_str())
    }

    /// Timestamp of the last photohelper develop pass, if set.
    #[must_use]
    pub fn last_processed_at(&self) -> Option<OffsetDateTime> {
        self.last_processed_at
    }

    /// Raw `xmp:MetadataDate` from the parsed sidecar.
    #[must_use]
    pub fn metadata_date(&self) -> Option<OffsetDateTime> {
        self.metadata_date
    }

    /// True if ANY `crs:` attribute was present in the parsed XMP.
    #[must_use]
    pub fn has_any_crs_attribute(&self) -> bool {
        self.has_any_crs_attr
    }

    /// Star rating, if set.
    #[must_use]
    pub fn rating(&self) -> Option<Rating> {
        self.rating
    }

    /// Lightroom color label, if set.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Flat keywords.
    #[must_use]
    pub fn keywords(&self) -> Option<&BTreeSet<String>> {
        self.keywords.as_ref()
    }

    /// Hierarchical keywords.
    #[must_use]
    pub fn hierarchical_keywords(&self) -> Option<&BTreeSet<String>> {
        self.hierarchical_keywords.as_ref()
    }

    /// Returns `true` if any of the 6 numeric `crs:` develop fields is set.
    #[must_use]
    pub fn has_crs_fields(&self) -> bool {
        self.temperature.is_some()
            || self.tint.is_some()
            || self.exposure.is_some()
            || self.contrast.is_some()
            || self.highlights.is_some()
            || self.shadows.is_some()
            || self.auto_tone.is_some()
    }

    /// Returns `true` if no fields (crs:, ph:, or standard) are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.has_crs_fields()
            && matches!(self.nima_score, Update::Keep | Update::Clear)
            && matches!(self.dedup_cluster_id, Update::Keep | Update::Clear)
            && matches!(self.photohelper_id, Update::Keep | Update::Clear)
            && self.last_processed_at.is_none()
            && self.rating.is_none_or(|r| r == Rating::Unrated)
            && self.label.as_ref().is_none_or(|l| l.is_empty())
            && self.keywords.as_ref().is_none_or(|k| k.is_empty())
            && self
                .hierarchical_keywords
                .as_ref()
                .is_none_or(|k| k.is_empty())
    }

    /// Merges this existing settings with incoming updates, preserving standard
    /// tags and user-defined keywords when individual update flags are absent.
    #[must_use]
    pub fn merge(&self, incoming: &SidecarSettings) -> Self {
        let temperature = incoming.temperature.or(self.temperature);
        let tint = incoming.tint.or(self.tint);
        let exposure = incoming.exposure.or(self.exposure);
        let contrast = incoming.contrast.or(self.contrast);
        let highlights = incoming.highlights.or(self.highlights);
        let shadows = incoming.shadows.or(self.shadows);
        let auto_tone = incoming.auto_tone.or(self.auto_tone);

        let nima_score = incoming
            .nima_score
            .clone()
            .resolve(self.nima_score.clone().as_option().copied())
            .into();
        let dedup_cluster_id = incoming
            .dedup_cluster_id
            .clone()
            .resolve(self.dedup_cluster_id.clone().as_option().copied())
            .into();
        let photohelper_id = incoming
            .photohelper_id
            .clone()
            .resolve(self.photohelper_id.clone().as_option().cloned())
            .into();
        let last_processed_at = incoming.last_processed_at.or(self.last_processed_at);

        let rating = incoming.rating.or(self.rating);

        let label = incoming.label.clone().or_else(|| self.label.clone());

        let keywords = merge_keywords(self.keywords.as_ref(), incoming.keywords.as_ref());
        let hierarchical_keywords = merge_keywords(
            self.hierarchical_keywords.as_ref(),
            incoming.hierarchical_keywords.as_ref(),
        );

        Self {
            temperature,
            tint,
            exposure,
            contrast,
            highlights,
            shadows,
            auto_tone,
            nima_score,
            dedup_cluster_id,
            photohelper_id,
            last_processed_at,
            metadata_date: self.metadata_date,
            has_any_crs_attr: self.has_any_crs_attr || incoming.has_any_crs_attr,
            rating,
            label,
            keywords,
            hierarchical_keywords,
        }
    }

    /// Lenient constructor used by the XMP reader. Out-of-range temperature and tint
    /// values are clamped to their boundary limits, while other out-of-range numeric
    /// values are ignored (mapped to `None`) with a `tracing::warn!`.
    pub(crate) fn from_parsed(fields: ParsedFields) -> Self {
        let temperature = fields.temperature.map(|v| {
            let clamped = v.clamp(2000, 50_000);
            if v != clamped {
                tracing::warn!(value = v, "crs:Temperature out of bounds; clamped");
            }
            clamped
        });
        let tint = fields.tint.map(|v| {
            let clamped = v.clamp(-150, 150);
            if v != clamped {
                tracing::warn!(value = v, "crs:Tint out of bounds; clamped");
            }
            clamped
        });
        let exposure = fields.exposure.and_then(|v| {
            if v.is_finite() && (-5.0..=5.0).contains(&v) {
                Some(v)
            } else {
                tracing::warn!(
                    value = v,
                    "crs:Exposure2012 out of [-5.0, 5.0] or non-finite; ignoring"
                );
                None
            }
        });
        let validate_100 = |name: &str, v: i32| -> Option<i32> {
            if (-100..=100).contains(&v) {
                Some(v)
            } else {
                tracing::warn!(
                    field = name,
                    value = v,
                    "crs field out of [-100, 100]; ignoring"
                );
                None
            }
        };

        let label = fields.label.map(|v| v.trim().to_string());

        Self {
            temperature,
            tint,
            exposure,
            contrast: fields
                .contrast
                .and_then(|v| validate_100("crs:Contrast2012", v)),
            highlights: fields
                .highlights
                .and_then(|v| validate_100("crs:Highlights2012", v)),
            shadows: fields
                .shadows
                .and_then(|v| validate_100("crs:Shadows2012", v)),
            auto_tone: fields.auto_tone,
            nima_score: fields.nima_score.into(),
            dedup_cluster_id: fields.dedup_cluster_id.into(),
            photohelper_id: fields.photohelper_id.into(),
            last_processed_at: fields.last_processed_at,
            metadata_date: fields.metadata_date,
            has_any_crs_attr: fields.has_any_crs_attr,
            rating: fields.rating,
            label,
            keywords: Some(fields.keywords),
            hierarchical_keywords: Some(fields.hierarchical_keywords),
        }
    }
}

/// Builder for [`SidecarSettings`].
#[derive(Default, Clone)]
pub struct SidecarSettingsBuilder {
    temperature: Option<i32>,
    tint: Option<i32>,
    exposure: Option<f32>,
    contrast: Option<i32>,
    highlights: Option<i32>,
    shadows: Option<i32>,
    auto_tone: Option<bool>,
    nima_score: Update<f32>,
    dedup_cluster_id: Update<i64>,
    photohelper_id: Update<String>,
    last_processed_at: Option<OffsetDateTime>,
    rating: Option<Rating>,
    label: Option<String>,
    keywords: Option<BTreeSet<String>>,
    hierarchical_keywords: Option<BTreeSet<String>>,
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

    /// Auto Tone flag.
    /// Defers to Lightroom's internal `AutoTone` engine and does not apply numerical adjustments.
    #[must_use]
    pub fn auto_tone(mut self, v: bool) -> Self {
        self.auto_tone = Some(v);
        self
    }

    /// NIMA aesthetic score.
    #[must_use]
    pub fn nima_score(mut self, v: f32) -> Self {
        self.nima_score = Update::Set(v);
        self
    }
    /// Clear (delete) the NIMA aesthetic score.
    #[must_use]
    pub fn clear_nima_score(mut self) -> Self {
        self.nima_score = Update::Clear;
        self
    }

    /// Duplicate cluster ID (must be non-negative).
    #[must_use]
    pub fn dedup_cluster_id(mut self, v: i64) -> Self {
        self.dedup_cluster_id = Update::Set(v);
        self
    }
    /// Clear (delete) the duplicate cluster ID.
    #[must_use]
    pub fn clear_dedup_cluster_id(mut self) -> Self {
        self.dedup_cluster_id = Update::Clear;
        self
    }

    /// photohelper photo ID (43-char base64url string from `PhotoId`).
    #[must_use]
    pub fn photohelper_id(mut self, v: impl Into<String>) -> Self {
        self.photohelper_id = Update::Set(v.into());
        self
    }
    /// Clear (delete) the photohelper photo ID.
    #[must_use]
    pub fn clear_photohelper_id(mut self) -> Self {
        self.photohelper_id = Update::Clear;
        self
    }
    /// Timestamp of the last photohelper develop pass.
    #[must_use]
    pub fn last_processed_at(mut self, v: OffsetDateTime) -> Self {
        self.last_processed_at = Some(v);
        self
    }

    /// Star rating.
    #[must_use]
    pub fn rating(mut self, v: Rating) -> Self {
        self.rating = Some(v);
        self
    }

    /// Color label.
    #[must_use]
    pub fn label(mut self, v: impl Into<String>) -> Self {
        self.label = Some(v.into());
        self
    }

    /// Flat keywords.
    #[must_use]
    pub fn keywords(mut self, v: BTreeSet<String>) -> Self {
        self.keywords = Some(v);
        self
    }

    /// Hierarchical keywords.
    #[must_use]
    pub fn hierarchical_keywords(mut self, v: BTreeSet<String>) -> Self {
        self.hierarchical_keywords = Some(v);
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
        if let Some(&s) = self.nima_score.as_option() {
            if !s.is_finite() || !(1.0..=10.0).contains(&s) {
                return Err(Error::Validation {
                    message: format!("nima_score {s} is not finite or outside [1.0, 10.0]"),
                });
            }
        }
        if let Some(&c) = self.dedup_cluster_id.as_option() {
            if c < 0 {
                return Err(Error::Validation {
                    message: format!("dedup_cluster_id {c} is negative (must be >= 0)"),
                });
            }
        }

        let label = self.label.map(|v| {
            let trimmed = v.trim().to_string();
            if trimmed.is_empty() {
                String::new()
            } else {
                trimmed
            }
        });

        // Trim/clean hierarchical keywords
        let hierarchical_keywords = self.hierarchical_keywords.map(|kws| {
            let mut set = BTreeSet::new();
            for kw in kws {
                let trimmed = kw.trim().to_string();
                if !trimmed.is_empty() {
                    set.insert(trimmed);
                }
            }
            set
        });

        let keywords = self.keywords.map(|kws| {
            let mut set = BTreeSet::new();
            for kw in kws {
                let trimmed = kw.trim().to_string();
                if !trimmed.is_empty() {
                    set.insert(trimmed);
                }
            }
            set
        });

        if let Some(pid) = self.photohelper_id.as_option() {
            if !crate::xml::is_valid_xml_string(pid) {
                return Err(Error::Validation {
                    message: "photohelper_id contains invalid XML characters".to_string(),
                });
            }
        }

        if let Some(l) = &label {
            if !crate::xml::is_valid_xml_string(l) {
                return Err(Error::Validation {
                    message: "label contains invalid XML characters".to_string(),
                });
            }
        }

        if let Some(kws) = &keywords {
            for kw in kws {
                if !crate::xml::is_valid_xml_string(kw) {
                    return Err(Error::Validation {
                        message: "keyword contains invalid XML characters".to_string(),
                    });
                }
            }
        }

        if let Some(kws) = &hierarchical_keywords {
            for kw in kws {
                if !crate::xml::is_valid_xml_string(kw) {
                    return Err(Error::Validation {
                        message: "hierarchical_keyword contains invalid XML characters".to_string(),
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
            auto_tone: self.auto_tone,
            nima_score: self.nima_score,
            dedup_cluster_id: self.dedup_cluster_id,
            photohelper_id: self.photohelper_id,
            last_processed_at: self.last_processed_at,
            metadata_date: None,
            has_any_crs_attr: false,
            rating: self.rating,
            label,
            keywords,
            hierarchical_keywords,
        })
    }
}
