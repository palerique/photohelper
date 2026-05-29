//! `PhotoRow` — Rust type mirroring one `photos` table row.
//!
//! Column-name knowledge is confined to this module so column reorders
//! don't silently break positional reads.

use std::path::{Path, PathBuf};

use photohelper_core::Error;
use photohelper_core::catalog_glue;
use photohelper_core::model::PhotoId;

/// One row of the `photos` table.
#[derive(Clone, Debug)]
pub struct PhotoRow {
    /// PhotoId raw bytes (PRIMARY KEY).
    pub id: PhotoId,
    /// Canonical absolute source path.
    pub source_path: String,
    /// File size in bytes.
    pub file_size: i64,
    /// Clamped mtime in Unix seconds.
    pub mtime_unix_seconds: i64,
    /// 1 iff original mtime was outside the allowed range.
    pub mtime_anomalous: i64,
    /// Raw EXIF Make.
    pub make: Option<String>,
    /// Raw EXIF Model.
    pub model: Option<String>,
    /// Known-camera slug (NULL when unknown).
    pub camera_slug: Option<String>,
    /// EXIF DateTimeOriginal as Unix seconds.
    pub capture_time_unix_seconds: Option<i64>,
    /// EXIF PixelXDimension.
    pub width: Option<i64>,
    /// EXIF PixelYDimension.
    pub height: Option<i64>,
    /// EXIF Orientation tag value (1..=8).
    pub exif_orientation: Option<i64>,
    /// When this row was inserted, Unix seconds.
    pub ingested_at_unix_seconds: i64,
    /// Set when a newer row at the same source_path supersedes this one.
    pub superseded_at_unix_seconds: Option<i64>,
}

impl PhotoRow {
    /// SELECT-side mapper. Column order matches `SELECT_ALL_COLUMNS`.
    pub(crate) fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, rusqlite::Error> {
        let id_bytes: Vec<u8> = row.get("id")?;
        let id_arr: [u8; 32] = id_bytes.as_slice().try_into().map_err(|_| {
            rusqlite::Error::InvalidColumnType(0, "id".into(), rusqlite::types::Type::Blob)
        })?;
        Ok(Self {
            id: catalog_glue::photo_id_from_row_bytes(id_arr),
            source_path: row.get("source_path")?,
            file_size: row.get("file_size")?,
            mtime_unix_seconds: row.get("mtime_unix_seconds")?,
            mtime_anomalous: row.get("mtime_anomalous")?,
            make: row.get("make")?,
            model: row.get("model")?,
            camera_slug: row.get("camera_slug")?,
            capture_time_unix_seconds: row.get("capture_time_unix_seconds")?,
            width: row.get("width")?,
            height: row.get("height")?,
            exif_orientation: row.get("exif_orientation")?,
            ingested_at_unix_seconds: row.get("ingested_at_unix_seconds")?,
            superseded_at_unix_seconds: row.get("superseded_at_unix_seconds")?,
        })
    }
}

/// A 2-field projection used by the AI culling pipeline: enough to
/// re-derive the `PhotoId` and locate the file on disk, without pulling
/// all 14 columns of `PhotoRow`.
///
/// Produced by [`super::Catalog::unsuperseded_unscored_rows`].
///
/// `source_path` is the path as canonicalized at ingest time, not re-validated
/// at query time. The culling pipeline is responsible for per-file existence
/// checks; keeping the batch-query path free of filesystem calls ensures that
/// one deleted file cannot abort the entire work list
/// (Theme-A fix: see `docs/code-reviews/session-04-catalog-migration-round1.md § Theme A`).
#[derive(Clone, Debug)]
pub struct CullRow {
    /// `PhotoId` as stored in the catalog (used for content-change detection
    /// and as the FK key in `cull_scores`).
    photo_id: PhotoId,
    /// Source path as canonicalized at ingest time, not re-validated at query time.
    /// The file may have been moved or deleted since ingest; callers must check
    /// existence before opening.
    source_path: PathBuf,
}

impl CullRow {
    /// Construct a `CullRow` from DB-retrieved values. `pub(crate)` keeps
    /// construction inside the catalog layer.
    pub(crate) fn new(photo_id: PhotoId, source_path: PathBuf) -> Self {
        Self {
            photo_id,
            source_path,
        }
    }

    /// `PhotoId` as stored in the catalog.
    pub fn photo_id(&self) -> PhotoId {
        self.photo_id
    }

    /// Source path as a `&Path` reference.
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }
}

/// A 2-field projection used by the dedup embedding pipeline: enough to locate
/// the file on disk and identify the photo in the catalog.
///
/// Intentionally distinct from [`CullRow`]: the types may diverge (e.g.,
/// `EmbeddingRow` could carry `existing_dim` or `model_slug`; `CullRow` could
/// carry `existing_score`). Two identical DTOs is below the three-instance
/// abstraction threshold per project convention.
///
/// Produced by [`super::Catalog::unembedded_rows`].
#[derive(Clone, Debug)]
pub struct EmbeddingRow {
    photo_id: PhotoId,
    source_path: PathBuf,
}

impl EmbeddingRow {
    /// Construct from DB-retrieved values.
    pub(crate) fn new(photo_id: PhotoId, source_path: PathBuf) -> Self {
        Self {
            photo_id,
            source_path,
        }
    }

    /// `PhotoId` as stored in the catalog.
    pub fn photo_id(&self) -> PhotoId {
        self.photo_id
    }

    /// Source path as canonicalized at ingest time.
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }
}

/// Columns selected by `from_row`. Keep in sync with the struct.
pub(crate) const SELECT_ALL_COLUMNS: &str = "id, source_path, file_size, \
     mtime_unix_seconds, mtime_anomalous, make, model, camera_slug, \
     capture_time_unix_seconds, width, height, exif_orientation, \
     ingested_at_unix_seconds, superseded_at_unix_seconds";

/// Convert a rusqlite error into a per-photo catalog-insert error.
pub(crate) fn insert_error(photo_id: PhotoId, source: rusqlite::Error) -> Error {
    Error::CatalogInsert {
        photo_id,
        source: Box::new(source),
    }
}
