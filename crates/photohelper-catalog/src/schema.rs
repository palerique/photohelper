//! Catalog SQL schema (v1).
//!
//! Authoritative reference: `docs/decisions/0001-catalog-schema-v1.md`.

/// Current schema version this binary supports.
pub const SCHEMA_VERSION: i64 = 1;

/// One-shot init SQL. Wrapped in `BEGIN IMMEDIATE; ... COMMIT;` by the
/// caller so partial failures don't leave the schema half-initialized.
pub const INIT_SQL: &str = r"
    CREATE TABLE IF NOT EXISTS photos (
        id BLOB PRIMARY KEY,
        source_path TEXT NOT NULL,
        file_size INTEGER NOT NULL,
        mtime_unix_seconds INTEGER NOT NULL,
        mtime_anomalous INTEGER NOT NULL DEFAULT 0,
        make TEXT,
        model TEXT,
        camera_slug TEXT,
        capture_time_unix_seconds INTEGER,
        width INTEGER,
        height INTEGER,
        exif_orientation INTEGER,
        ingested_at_unix_seconds INTEGER NOT NULL,
        superseded_at_unix_seconds INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_photos_source_path ON photos(source_path);
    CREATE INDEX IF NOT EXISTS idx_photos_camera_slug ON photos(camera_slug);
    PRAGMA user_version = 1;
";
