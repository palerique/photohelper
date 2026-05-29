//! Catalog SQL schema.
//!
//! Authoritative references:
//! - v1: `docs/decisions/0001-catalog-schema-v1.md`
//! - v2: `docs/decisions/0002-catalog-schema-v2.md`
//! - v3: `docs/decisions/0003-catalog-schema-v3.md`

/// Current schema version this binary supports.
pub const SCHEMA_VERSION: i64 = 3;

/// One-shot v1 init SQL. Sets `user_version = 1` so `Catalog::open`
/// can then apply `MIGRATE_V1_TO_V2_SQL` in a second transaction.
/// Wrapped in `BEGIN IMMEDIATE; ... COMMIT;` by the caller.
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

/// v1 → v2 migration SQL. Adds the `cull_scores` table and its index,
/// then bumps `user_version` to 2. Wrapped in `BEGIN IMMEDIATE; ...
/// COMMIT;` by `apply_v1_to_v2` in `catalog.rs`.
///
/// `CREATE TABLE IF NOT EXISTS` is idempotent — safe to replay if a
/// previous migration run committed the DDL but crashed before bumping
/// `user_version` (per plan PR1-T27).
pub const MIGRATE_V1_TO_V2_SQL: &str = r"
    CREATE TABLE IF NOT EXISTS cull_scores (
        photo_id                BLOB    NOT NULL REFERENCES photos(id),
        model_slug              TEXT    NOT NULL,
        aesthetic_score         REAL    NOT NULL,
        scored_at_unix_seconds  INTEGER NOT NULL,
        PRIMARY KEY (photo_id, model_slug)
    );
    CREATE INDEX IF NOT EXISTS idx_cull_scores_photo ON cull_scores(photo_id);
    PRAGMA user_version = 2;
";

/// v2 → v3 migration SQL. Adds the `embeddings` and `dup_clusters` tables,
/// then bumps `user_version` to 3. Wrapped in `BEGIN IMMEDIATE; ... COMMIT;`
/// by `apply_v2_to_v3` in `catalog.rs`.
///
/// `CREATE TABLE IF NOT EXISTS` is idempotent — safe to replay if a previous
/// run committed the DDL but crashed before bumping `user_version`.
///
/// Design decisions:
/// - `embeddings.PRIMARY KEY (photo_id, model_slug)` — one row per photo per model.
/// - `dup_clusters.FOREIGN KEY (photo_id, model_slug) REFERENCES embeddings(photo_id, model_slug)` —
///   a cluster assignment can only reference a photo that has been embedded.
/// - `similarity_threshold REAL` stored per-row as a v0.1 stop-gap (TD-019: no
///   `dedup_runs` table yet; the threshold is re-readable from CLI args).
/// - ON DELETE CASCADE absent: no `photos` delete path in v0.1; see DN-023.
pub const MIGRATE_V2_TO_V3_SQL: &str = r"
    CREATE TABLE IF NOT EXISTS embeddings (
        photo_id                    BLOB    NOT NULL REFERENCES photos(id),
        model_slug                  TEXT    NOT NULL,
        dim                         INTEGER NOT NULL CHECK(dim > 0 AND dim <= 65536),
        quantization                TEXT    NOT NULL DEFAULT 'f32',
        embedding                   BLOB    NOT NULL,
        embedded_at_unix_seconds    INTEGER NOT NULL,
        PRIMARY KEY (photo_id, model_slug)
    );

    CREATE TABLE IF NOT EXISTS dup_clusters (
        photo_id                    BLOB    NOT NULL,
        model_slug                  TEXT    NOT NULL,
        cluster_id                  INTEGER NOT NULL CHECK(cluster_id >= 0),
        similarity_threshold        REAL    NOT NULL,
        clustered_at_unix_seconds   INTEGER NOT NULL,
        PRIMARY KEY (photo_id, model_slug),
        FOREIGN KEY (photo_id, model_slug) REFERENCES embeddings(photo_id, model_slug)
    );

    PRAGMA user_version = 3;
";
