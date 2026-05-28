//! `PhotoRow` — Rust type mirroring one `photos` table row.
//!
//! Column-name knowledge is confined to this module so column reorders
//! don't silently break positional reads.

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
