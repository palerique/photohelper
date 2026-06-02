//! `Catalog` — SQLite-backed photo catalog with file-lock + WAL + panic-poison.
//!
//! See `docs/plans/session-01.md` §Deliverables 4.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use photohelper_core::Error;
use photohelper_core::model::{AbsPath, Photo, PhotoId};

use crate::row::{CullRow, DevelopRow, EmbeddingRow, PhotoRow, SELECT_ALL_COLUMNS, insert_error};
use crate::schema::{INIT_SQL, MIGRATE_V1_TO_V2_SQL, MIGRATE_V2_TO_V3_SQL, SCHEMA_VERSION};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// 14-column INSERT (13 bound params + `superseded_at` hardcoded NULL) used by
/// both the `Inserted` and `SupersededPrevious` arms of `Catalog::upsert`.
/// Extracted in R1.T14 to eliminate duplicate-statement drift risk.
const INSERT_PHOTO_SQL: &str = "INSERT INTO photos (
    id, source_path, file_size, mtime_unix_seconds,
    mtime_anomalous, make, model, camera_slug,
    capture_time_unix_seconds, width, height,
    exif_orientation, ingested_at_unix_seconds,
    superseded_at_unix_seconds
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)";

/// Production lock-retry delay between attempts. Tests override via the
/// `#[doc(hidden)]` `open_with_retry_delay` constructor (not `#[cfg(test)]`-gated;
/// `#[doc(hidden)]` discourages but does not prevent production use).
const LOCK_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Outcome of [`Catalog::upsert`] for an `IngestStats` driver.
///
/// NOT `#[non_exhaustive]` for v0.1 — adding the attribute now would
/// force a wildcard arm into the cross-crate match in
/// `photohelper-cli::commands::ingest::ingest_one`, which kills
/// exhaustive-match safety. When `InsertedWithPartialExif` lands
/// (plan §4e enhancement, deferred from D4), the `#[non_exhaustive]`
/// attribute lands in the same commit alongside the wildcard arm.
#[derive(Debug, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// Brand-new row inserted (no prior at this source_path).
    Inserted,
    /// Same source_path had a different PhotoId; old superseded.
    SupersededPrevious {
        /// Previously-superseded row's PhotoId.
        old: PhotoId,
    },
    /// Identical PhotoId already in the catalog (re-ingest or hardlink).
    AlreadyCatalogued,
}

/// Outcome of [`Catalog::insert_embedding`].
///
/// Discriminated via `conn.changes()` after `INSERT OR IGNORE`.
#[derive(Debug, PartialEq, Eq)]
pub enum InsertEmbeddingOutcome {
    /// Row was inserted; this is the first embedding for this photo × model.
    Inserted,
    /// Row already existed; nothing changed.
    AlreadyEmbedded,
}

/// Outcome of [`Catalog::insert_cull_score`].
///
/// Discriminated via `conn.changes()` after `INSERT OR IGNORE` — the
/// standard SQLite idiom that avoids a pre-SELECT round-trip (plan
/// PR1-T13).
#[derive(Debug, PartialEq, Eq)]
pub enum InsertScoreOutcome {
    /// Row was inserted; this is the first score for this photo × model.
    Inserted,
    /// Row already existed (duplicate cull run or race); nothing changed.
    AlreadyScored,
}

/// SQLite-backed catalog. `Send + Sync` for `Arc<Catalog>` sharing across
/// rayon workers. Panic-poisons on worker panic — subsequent ops return
/// [`Error::CatalogPoisoned`].
pub struct Catalog {
    conn: Mutex<Connection>,
    /// Held for the lifetime of the Catalog so the OS keeps the exclusive
    /// flock active. Never read after construction.
    _lock_handle: File,
    canonical_path: PathBuf,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("canonical_path", &self.canonical_path)
            .finish_non_exhaustive()
    }
}

impl Catalog {
    /// Open the catalog at `catalog_path`, acquiring an exclusive file lock
    /// and initializing the schema if needed.
    ///
    /// # Errors
    /// See [`Error`] — every fatal failure mode is a typed variant.
    pub fn open(catalog_path: impl AsRef<Path>, lock_timeout_seconds: u32) -> Result<Self, Error> {
        Self::open_with_retry_delay(catalog_path, lock_timeout_seconds, LOCK_RETRY_DELAY)
    }

    /// Lower-level constructor exposing `retry_delay` for test control.
    /// `#[doc(hidden)]` discourages (but does not prevent) production callers;
    /// this method is NOT `#[cfg(test)]`-gated and compiles in all profiles.
    #[doc(hidden)]
    pub fn open_with_retry_delay(
        catalog_path: impl AsRef<Path>,
        lock_timeout_seconds: u32,
        retry_delay: Duration,
    ) -> Result<Self, Error> {
        let catalog_path = catalog_path.as_ref();

        // Step 1+2: compute lock_path + create parent dir.
        let parent = catalog_path.parent().unwrap_or_else(|| Path::new("."));
        if !parent.exists() {
            tracing::info!("creating catalog parent directory {}", parent.display());
            std::fs::create_dir_all(parent).map_err(|e| Error::Io {
                path: parent.to_path_buf(),
                op: "mkdir-p",
                source: e,
            })?;
        }
        let lock_path = {
            let name = catalog_path.file_name().map_or_else(
                || "catalog.db".to_string(),
                |s| s.to_string_lossy().into_owned(),
            );
            parent.join(format!("{name}.lock"))
        };

        // Step 3+4: open + acquire lock with retry budget.
        // R2-T11 fix: op tag was "mkdir-p" (R1.T10's sibling miss); the actual
        // op is lock-file creation, not directory creation. Operators debugging
        // a permission-denied or read-only-FS lock-file failure should see
        // the accurate tag.
        let lock_file = File::create(&lock_path).map_err(|e| Error::Io {
            path: lock_path.clone(),
            op: "lock-file-create",
            source: e,
        })?;
        let start = Instant::now();
        let max_attempts = u32::try_from(
            ((u64::from(lock_timeout_seconds) * 1000) / (retry_delay.as_millis().max(1) as u64))
                .max(1),
        )
        .unwrap_or(u32::MAX);
        let mut attempts: u32 = 0;
        loop {
            attempts = attempts.saturating_add(1);
            match <File as fs4::FileExt>::try_lock(&lock_file) {
                Ok(()) => break,
                Err(fs4::TryLockError::WouldBlock) => {
                    if attempts >= max_attempts {
                        return Err(Error::CatalogLockHeld {
                            path: lock_path,
                            attempts,
                            total_ms: start.elapsed().as_millis() as u64,
                        });
                    }
                    tracing::warn!(
                        attempt = attempts,
                        max = max_attempts,
                        "catalog lock held; retrying"
                    );
                    thread::sleep(retry_delay);
                }
                Err(fs4::TryLockError::Error(e)) => {
                    // R1.T10 fix: op tag was "stat"; the actual op is
                    // file-lock acquisition. Operators debugging lock
                    // failures should see the accurate tag.
                    return Err(Error::Io {
                        path: lock_path,
                        op: "file-lock",
                        source: e,
                    });
                }
            }
        }

        // Step 5: verify existing catalog file's magic bytes.
        // R2-T1 verified: this check runs AFTER Step 4's `try_lock` loop exit
        // (via `Ok(()) => break`), i.e., while holding the exclusive file lock.
        // SESSION-STATE.md formerly carried a "Magic-byte TOCTOU not yet fixed"
        // item based on a misread of R1.T10 sub-item 3; closed-by-verification.
        if catalog_path.exists() {
            let meta = std::fs::metadata(catalog_path).map_err(|e| Error::Io {
                path: catalog_path.to_path_buf(),
                op: "stat",
                source: e,
            })?;
            if meta.is_dir() {
                return Err(Error::CatalogPathIsDirectory {
                    path: catalog_path.to_path_buf(),
                });
            }
            if meta.len() >= 16 {
                let mut head = [0u8; 16];
                let mut f = File::open(catalog_path).map_err(|e| Error::Io {
                    path: catalog_path.to_path_buf(),
                    op: "read-prefix",
                    source: e,
                })?;
                f.read_exact(&mut head).map_err(|e| Error::Io {
                    path: catalog_path.to_path_buf(),
                    op: "read-prefix",
                    source: e,
                })?;
                if &head != SQLITE_MAGIC {
                    return Err(Error::CatalogPathNotSqlite {
                        path: catalog_path.to_path_buf(),
                    });
                }
            }
        }

        // Step 6: open connection.
        let mut conn = Connection::open(catalog_path).map_err(|e| Error::CatalogOpen {
            path: catalog_path.to_path_buf(),
            source: Box::new(e),
        })?;

        // Step 7: PRAGMAs.
        for pragma in [
            "PRAGMA journal_mode = WAL",
            "PRAGMA synchronous = NORMAL",
            "PRAGMA busy_timeout = 5000",
            // Enable FK enforcement so `cull_scores.photo_id` REFERENCES `photos(id)`,
            // `embeddings.photo_id` REFERENCES `photos(id)`, and
            // `dup_clusters.(photo_id, model_slug)` REFERENCES `embeddings(photo_id, model_slug)`
            // are all enforced at INSERT time.
            "PRAGMA foreign_keys = ON",
        ] {
            conn.execute_batch(pragma).map_err(|e| Error::CatalogOpen {
                path: catalog_path.to_path_buf(),
                source: Box::new(e),
            })?;
        }

        // Step 8: schema-version gate + init if needed.
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| Error::CatalogOpen {
                path: catalog_path.to_path_buf(),
                source: Box::new(e),
            })?;
        match user_version {
            0 => {
                // Fresh DB: run v1 init then chain both migrations so new
                // catalogs start at SCHEMA_VERSION without intermediate states.
                // R2-T8 fix: use IMMEDIATE so init takes the RESERVED lock up-front.
                let tx = conn
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .map_err(|e| Error::CatalogOpen {
                        path: catalog_path.to_path_buf(),
                        source: Box::new(e),
                    })?;
                tx.execute_batch(INIT_SQL).map_err(|e| Error::CatalogOpen {
                    path: catalog_path.to_path_buf(),
                    source: Box::new(e),
                })?;
                tx.commit().map_err(|e| Error::CatalogOpen {
                    path: catalog_path.to_path_buf(),
                    source: Box::new(e),
                })?;
                apply_v1_to_v2(&mut conn, catalog_path)?;
                apply_v2_to_v3(&mut conn, catalog_path)?;
            }
            1 => {
                apply_v1_to_v2(&mut conn, catalog_path)?;
                apply_v2_to_v3(&mut conn, catalog_path)?;
            }
            2 => {
                apply_v2_to_v3(&mut conn, catalog_path)?;
            }
            v if v == SCHEMA_VERSION => {}
            other => {
                return Err(Error::CatalogSchemaTooNew {
                    found: other,
                    expected: SCHEMA_VERSION,
                });
            }
        }

        // Step 9: WAL recovery check.
        // R1.T10 fix: surface PRAGMA failures rather than silently
        // collapsing to "clean shutdown." Any unknown error here (schema
        // mismatch, busy, future SQLite column-count change) leaves us
        // unable to tell the user whether their last run was clean —
        // log explicitly.
        match conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            r.get::<_, i64>(1)
        }) {
            Ok(recovered) if recovered > 0 => {
                tracing::warn!(
                    frames = recovered,
                    "previous shutdown was unclean; recovered {recovered} WAL frames"
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(
                error = %e,
                "could not query WAL checkpoint state; recovery status unknown"
            ),
        }

        let canonical_path = AbsPath::canonicalize(catalog_path).map_or_else(
            |e| {
                tracing::warn!(
                    error = %e,
                    path = %catalog_path.display(),
                    "could not canonicalize catalog path; using raw path in error messages"
                );
                catalog_path.to_path_buf()
            },
            |p| p.as_path().to_path_buf(),
        );

        Ok(Self {
            conn: Mutex::new(conn),
            _lock_handle: lock_file,
            canonical_path,
        })
    }

    /// Insert (or supersede) one photo.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned` if a prior worker panicked mid-write.
    /// - `Error::CatalogInsert` for SQLite failures.
    pub fn upsert(
        &self,
        photo: &Photo,
        ingested_at_unix_seconds: i64,
    ) -> Result<UpsertOutcome, Error> {
        let mut guard = match self.conn.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                let conn = poisoned.into_inner();
                // ROLLBACK any open transaction left by the panicked worker.
                // SQLITE_ERROR (extended_code 1) is returned with "cannot rollback -
                // no transaction is active" when the panicked worker held the lock but
                // had no open transaction — nothing to undo, safe to ignore.
                // Note: plan v4 cited ApiMisuse (SQLITE_MISUSE = 21) here but empirical
                // testing showed SQLite returns SQLITE_ERROR (rc=1) for this case.
                match conn.execute("ROLLBACK", []) {
                    Ok(_) => {}
                    // extended_code 1 = SQLITE_ERROR (not SQLITE_MISUSE/21);
                    // the message is "cannot rollback - no transaction is active".
                    Err(rusqlite::Error::SqliteFailure(e, _)) if e.extended_code == 1 => {}
                    Err(e) => {
                        return Err(Error::CatalogTransaction {
                            op: "rollback-after-worker-panic",
                            source: Box::new(e),
                        });
                    }
                }
                return Err(Error::CatalogPoisoned {
                    path: self.canonical_path.clone(),
                });
            }
        };
        let pid = photo.photo_id();
        let id_bytes = pid.as_bytes().to_vec();

        let tx = guard
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| insert_error(pid, e))?;

        // Look for an existing row by id (same content) OR by source_path.
        // R2-T3 fix: distinguish "no row" (Ok) from real SQLite errors. The
        // former `.ok()` coalesced QueryReturnedNoRows with SqliteFailure /
        // InvalidColumnType / disk-full into None, masking real lookup
        // failures behind a misattributed CatalogInsert downstream.
        let existing_by_id: Option<()> = match tx.query_row(
            "SELECT 1 FROM photos WHERE id = ?1",
            rusqlite::params![&id_bytes],
            |_| Ok(()),
        ) {
            Ok(v) => Some(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(insert_error(pid, e)),
        };
        if existing_by_id.is_some() {
            tx.commit().map_err(|e| insert_error(pid, e))?;
            tracing::info!(
                photo_id = %pid,
                "same content already cataloged; INSERT OR IGNORE"
            );
            return Ok(UpsertOutcome::AlreadyCatalogued);
        }

        let source_path_str = photo.source_path().to_string_lossy().into_owned();
        // R2-T3 fix (second site): same QueryReturnedNoRows-vs-real-error
        // discrimination as the existing_by_id lookup above.
        let existing_at_path: Option<Vec<u8>> = match tx.query_row(
            "SELECT id FROM photos
               WHERE source_path = ?1 AND superseded_at_unix_seconds IS NULL",
            rusqlite::params![&source_path_str],
            |row| row.get(0),
        ) {
            Ok(v) => Some(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(insert_error(pid, e)),
        };

        let camera_slug: Option<String> = match photo.camera_id() {
            Some(photohelper_core::model::CameraId::Known(k)) => Some(k.slug().to_string()),
            _ => None,
        };
        let exif = photo.exif();
        let exif_orientation_i64 = exif.orientation.map(|o| o.to_tag());
        let width_i64 = exif.width.map(i64::from);
        let height_i64 = exif.height.map(i64::from);
        let file_size_i64 = i64::try_from(photo.file_size()).unwrap_or(i64::MAX);

        // R1.T14 fix: single insert call used by both branches —
        // previously this was a 14-column INSERT duplicated twice
        // (drift risk on every schema change). The closure captures
        // the per-call parameter bindings.
        let do_insert = |tx: &rusqlite::Transaction<'_>| -> Result<(), Error> {
            tx.execute(
                INSERT_PHOTO_SQL,
                rusqlite::params![
                    &id_bytes,
                    &source_path_str,
                    file_size_i64,
                    photo.clamped_mtime_unix_seconds(),
                    i64::from(photo.mtime_anomalous()),
                    exif.make.as_deref(),
                    exif.model.as_deref(),
                    camera_slug.as_deref(),
                    exif.capture_time_unix_seconds,
                    width_i64,
                    height_i64,
                    exif_orientation_i64,
                    ingested_at_unix_seconds,
                ],
            )
            .map_err(|e| insert_error(pid, e))?;
            Ok(())
        };

        let outcome = match existing_at_path {
            None => {
                do_insert(&tx)?;
                UpsertOutcome::Inserted
            }
            Some(old_bytes) => {
                let old_arr: [u8; 32] = old_bytes.as_slice().try_into().map_err(|_| {
                    insert_error(
                        pid,
                        rusqlite::Error::InvalidColumnType(
                            0,
                            "id".into(),
                            rusqlite::types::Type::Blob,
                        ),
                    )
                })?;
                let old = photohelper_core::catalog_glue::photo_id_from_row_bytes(old_arr);
                tx.execute(
                    "UPDATE photos SET superseded_at_unix_seconds = ?2 WHERE id = ?1",
                    rusqlite::params![&old_bytes, ingested_at_unix_seconds],
                )
                .map_err(|e| insert_error(pid, e))?;
                do_insert(&tx)?;
                UpsertOutcome::SupersededPrevious { old }
            }
        };
        tx.commit().map_err(|e| insert_error(pid, e))?;
        Ok(outcome)
    }

    /// Fetch all non-superseded photos that have not yet been scored by
    /// `model_slug`, ordered by ingest time. The AI culling pipeline calls
    /// this to get the work list for each cull run.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned` if a prior worker panicked mid-write.
    /// - `Error::CatalogOpen` for query failures.
    pub fn unsuperseded_unscored_rows(&self, model_slug: &str) -> Result<Vec<CullRow>, Error> {
        let guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;
        let sql = "SELECT id, source_path FROM photos \
                   WHERE superseded_at_unix_seconds IS NULL \
                     AND id NOT IN (SELECT photo_id FROM cull_scores WHERE model_slug = ?1) \
                   ORDER BY ingested_at_unix_seconds";
        let mut stmt = guard.prepare(sql).map_err(|e| Error::CatalogOpen {
            path: self.canonical_path.clone(),
            source: Box::new(e),
        })?;
        let rows = stmt
            .query_map(rusqlite::params![model_slug], |row| {
                let id_bytes: Vec<u8> = row.get("id")?;
                let id_arr: [u8; 32] = id_bytes.as_slice().try_into().map_err(|_| {
                    rusqlite::Error::InvalidColumnType(0, "id".into(), rusqlite::types::Type::Blob)
                })?;
                let photo_id = photohelper_core::catalog_glue::photo_id_from_row_bytes(id_arr);
                let path_str: String = row.get("source_path")?;
                Ok((photo_id, path_str))
            })
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
        let mut out = Vec::new();
        for r in rows {
            let (photo_id, path_str) = r.map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
            // Theme-A: store the raw path without calling std::fs::canonicalize.
            // Existence and canonicality checks happen per-file in run_cull so
            // a single missing file does not abort the entire work list.
            out.push(CullRow::new(photo_id, PathBuf::from(path_str)));
        }
        Ok(out)
    }

    /// Persist a cull score for `photo_id` × `model_slug`.
    ///
    /// Uses `INSERT OR IGNORE` so concurrent workers that score the same
    /// photo race safely — the first writer wins, the rest see `AlreadyScored`.
    /// `conn.changes()` after the INSERT discriminates the outcome without a
    /// pre-SELECT round-trip (plan PR1-T13).
    ///
    /// `score` is the raw aesthetic score in `[1.0, 10.0]`; pass
    /// `nima_score.as_f64()` at the call site.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned` if a prior worker panicked.
    /// - `Error::CatalogInsert` for SQLite failures (includes FK violations)
    ///   and out-of-range `score` values.
    // TD-013: per-cull-run audit trail absent; scored_at_unix_seconds records when
    // the score was written but there is no cull_run_id column linking related rows
    // into a single batch. See TECH-DEBT.md § TD-013.
    pub fn insert_cull_score(
        &self,
        photo_id: PhotoId,
        model_slug: &str,
        score: f64,
        scored_at_unix_seconds: i64,
    ) -> Result<InsertScoreOutcome, Error> {
        let mut guard = match self.conn.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                let conn = poisoned.into_inner();
                match conn.execute("ROLLBACK", []) {
                    Ok(_) => {}
                    Err(rusqlite::Error::SqliteFailure(e, _)) if e.extended_code == 1 => {}
                    Err(e) => {
                        return Err(Error::CatalogTransaction {
                            op: "rollback-after-worker-panic",
                            source: Box::new(e),
                        });
                    }
                }
                return Err(Error::CatalogPoisoned {
                    path: self.canonical_path.clone(),
                });
            }
        };
        if !(1.0_f64..=10.0_f64).contains(&score) {
            return Err(Error::CatalogInsert {
                photo_id,
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("aesthetic_score {score} is outside the valid range [1.0, 10.0]"),
                )),
            });
        }
        let id_bytes = photo_id.as_bytes().to_vec();
        let tx = guard
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| insert_error(photo_id, e))?;
        tx.execute(
            "INSERT INTO cull_scores \
             (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) \
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT (photo_id, model_slug) DO NOTHING",
            rusqlite::params![&id_bytes, model_slug, score, scored_at_unix_seconds],
        )
        .map_err(|e| insert_error(photo_id, e))?;
        let outcome = if tx.changes() == 1 {
            InsertScoreOutcome::Inserted
        } else {
            InsertScoreOutcome::AlreadyScored
        };
        tx.commit().map_err(|e| insert_error(photo_id, e))?;
        Ok(outcome)
    }

    /// Fetch all non-superseded photos that do not yet have an embedding from
    /// `model_slug`, ordered by ingest time. The dedup pipeline calls this to
    /// get the work list for the embed phase.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned` if a prior worker panicked.
    /// - `Error::CatalogOpen` for query failures.
    pub fn unembedded_rows(&self, model_slug: &str) -> Result<Vec<EmbeddingRow>, Error> {
        let guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;
        let sql = "SELECT id, source_path FROM photos \
                   WHERE superseded_at_unix_seconds IS NULL \
                     AND id NOT IN (SELECT photo_id FROM embeddings WHERE model_slug = ?1) \
                   ORDER BY ingested_at_unix_seconds";
        let mut stmt = guard.prepare(sql).map_err(|e| Error::CatalogOpen {
            path: self.canonical_path.clone(),
            source: Box::new(e),
        })?;
        let rows = stmt
            .query_map(rusqlite::params![model_slug], |row| {
                let id_bytes: Vec<u8> = row.get("id")?;
                let id_arr: [u8; 32] = id_bytes.as_slice().try_into().map_err(|_| {
                    rusqlite::Error::InvalidColumnType(0, "id".into(), rusqlite::types::Type::Blob)
                })?;
                let photo_id = photohelper_core::catalog_glue::photo_id_from_row_bytes(id_arr);
                let path_str: String = row.get("source_path")?;
                Ok((photo_id, path_str))
            })
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
        let mut out = Vec::new();
        for r in rows {
            let (photo_id, path_str) = r.map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
            out.push(EmbeddingRow::new(photo_id, PathBuf::from(path_str)));
        }
        Ok(out)
    }

    /// Persist a float32 embedding for `photo_id` × `model_slug`.
    ///
    /// `embedding_bytes` must be a little-endian f32 byte slice (`dim × 4` bytes).
    /// Uses `INSERT OR IGNORE` so concurrent workers race safely — the first writer
    /// wins, the rest see `AlreadyEmbedded`.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned` if a prior worker panicked.
    /// - `Error::CatalogInsert` for SQLite failures (includes FK violations and
    ///   constraint violations like `dim` out of range).
    // TD-018: embedding stored as raw f32 LE bytes; quantization='f32' hardcoded.
    // See TECH-DEBT.md § TD-018 for the int8/f16 quantization upgrade plan.
    pub fn insert_embedding(
        &self,
        photo_id: PhotoId,
        model_slug: &str,
        embedding_bytes: &[u8],
        dim: usize,
        embedded_at_unix_seconds: i64,
    ) -> Result<InsertEmbeddingOutcome, Error> {
        // Rust-level guards: INSERT OR IGNORE silently swallows ALL SQLite constraint
        // violations (UNIQUE, CHECK, NOT NULL), so dim-range and byte-length mismatches
        // would silently return AlreadyEmbedded instead of an error.
        if dim == 0 || dim > 65536 {
            return Err(Error::CatalogInsert {
                photo_id,
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("dim {dim} is outside valid range [1, 65536]"),
                )),
            });
        }
        if embedding_bytes.len() != dim * 4 {
            return Err(Error::CatalogInsert {
                photo_id,
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "embedding byte length {} != dim {dim} * 4 ({})",
                        embedding_bytes.len(),
                        dim * 4
                    ),
                )),
            });
        }
        let mut guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;
        let id_bytes = photo_id.as_bytes().to_vec();
        let tx = guard
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| insert_error(photo_id, e))?;
        tx.execute(
            "INSERT INTO embeddings \
             (photo_id, model_slug, dim, quantization, embedding, embedded_at_unix_seconds) \
             VALUES (?1, ?2, ?3, 'f32', ?4, ?5) ON CONFLICT (photo_id, model_slug) DO NOTHING",
            rusqlite::params![
                &id_bytes,
                model_slug,
                dim as i64,
                embedding_bytes,
                embedded_at_unix_seconds
            ],
        )
        .map_err(|e| insert_error(photo_id, e))?;
        let outcome = if tx.changes() == 1 {
            InsertEmbeddingOutcome::Inserted
        } else {
            InsertEmbeddingOutcome::AlreadyEmbedded
        };
        tx.commit().map_err(|e| insert_error(photo_id, e))?;
        Ok(outcome)
    }

    /// Load all non-superseded embeddings for `model_slug` as raw f32 LE byte slices.
    ///
    /// Returns `(PhotoId, embedding_bytes, dim)` triples for the O(n²) clustering pass.
    /// Superseded photos are excluded (consistent with `unembedded_rows` and
    /// `unsuperseded_unscored_rows`). `insert_embedding` enforces `dim*4 == bytes.len()`
    /// at write time, so the returned triples are byte-length consistent.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned`, `Error::CatalogOpen` for query failures.
    pub fn all_embeddings_for_model(
        &self,
        model_slug: &str,
    ) -> Result<Vec<(PhotoId, Vec<u8>, usize)>, Error> {
        let guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;
        let mut stmt = guard
            .prepare(
                "SELECT e.photo_id, e.embedding, e.dim \
                 FROM embeddings e \
                 JOIN photos p ON p.id = e.photo_id \
                 WHERE e.model_slug = ?1 \
                   AND p.superseded_at_unix_seconds IS NULL",
            )
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
        let rows = stmt
            .query_map(rusqlite::params![model_slug], |row| {
                let id_bytes: Vec<u8> = row.get("photo_id")?;
                let embedding: Vec<u8> = row.get("embedding")?;
                let dim: i64 = row.get("dim")?;
                Ok((id_bytes, embedding, dim))
            })
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
        let mut out = Vec::new();
        for r in rows {
            let (id_bytes, embedding, dim_i64) = r.map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
            let id_arr: [u8; 32] =
                id_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|e| Error::CatalogOpen {
                        path: self.canonical_path.clone(),
                        source: Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("photo_id blob wrong length: {e}"),
                        )),
                    })?;
            let photo_id = photohelper_core::catalog_glue::photo_id_from_row_bytes(id_arr);
            let dim = usize::try_from(dim_i64).map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("embeddings.dim value {dim_i64} is out of usize range: {e}"),
                )),
            })?;
            out.push((photo_id, embedding, dim));
        }
        Ok(out)
    }

    /// Persist cluster assignments for a batch of photos.
    ///
    /// Uses `INSERT OR REPLACE` — re-clustering a photo replaces the old
    /// assignment. All inserts are wrapped in a single transaction for
    /// performance.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned` if a prior worker panicked.
    /// - `Error::CatalogInsert` for SQLite failures (includes FK violations).
    pub fn insert_dup_clusters(
        &self,
        assignments: &[(PhotoId, i64)],
        model_slug: &str,
        similarity_threshold: f32,
        clustered_at_unix_seconds: i64,
    ) -> Result<(), Error> {
        let mut guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;

        let tx = guard
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| {
                insert_error(
                    assignments.first().map_or_else(
                        || photohelper_core::catalog_glue::photo_id_from_row_bytes([0; 32]),
                        |(id, _)| *id,
                    ),
                    e,
                )
            })?;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO dup_clusters \
                 (photo_id, model_slug, cluster_id, similarity_threshold, clustered_at_unix_seconds) \
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ).map_err(|e| insert_error(assignments.first().map_or_else(|| photohelper_core::catalog_glue::photo_id_from_row_bytes([0;32]), |(id, _)| *id), e))?;

            for (photo_id, cluster_id) in assignments {
                let id_bytes = photo_id.as_bytes().to_vec();
                stmt.execute(rusqlite::params![
                    &id_bytes,
                    model_slug,
                    cluster_id,
                    f64::from(similarity_threshold),
                    clustered_at_unix_seconds
                ])
                .map_err(|e| insert_error(*photo_id, e))?;
            }
        }

        tx.commit().map_err(|e| {
            insert_error(
                assignments.first().map_or_else(
                    || photohelper_core::catalog_glue::photo_id_from_row_bytes([0; 32]),
                    |(id, _)| *id,
                ),
                e,
            )
        })?;
        Ok(())
    }
    /// Borrow the canonical catalog path.
    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Load all non-superseded photos with their NIMA aesthetic score (if culled)
    /// and duplicate cluster ID (if clustered).
    ///
    /// Used by the `develop` subcommand to build XMP sidecars for every
    /// ingested photo.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned`, `Error::CatalogOpen` for query failures.
    pub fn all_photos_with_cull_scores(
        &self,
        aesthetic_model_slug: &str,
        dedup_model_slug: &str,
    ) -> Result<Vec<DevelopRow>, Error> {
        let guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;
        let mut stmt = guard
            .prepare(
                "SELECT p.id, p.source_path, cs.aesthetic_score, dc.cluster_id \
                 FROM photos p \
                 LEFT JOIN cull_scores cs ON cs.photo_id = p.id AND cs.model_slug = ?1 \
                 LEFT JOIN dup_clusters dc ON dc.photo_id = p.id AND dc.model_slug = ?2 \
                 WHERE p.superseded_at_unix_seconds IS NULL \
                 ORDER BY p.ingested_at_unix_seconds, p.id",
            )
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
        let rows = stmt
            .query_map([aesthetic_model_slug, dedup_model_slug], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let source_path: String = row.get(1)?;
                let nima_score: Option<f64> = row.get(2)?;
                let cluster_id: Option<i64> = row.get(3)?;
                Ok((id_bytes, source_path, nima_score, cluster_id))
            })
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
        // Explicit per-row error propagation (not .flatten() which silently drops
        // rusqlite::Error from individual row reads).
        let mut result = Vec::new();
        for r in rows {
            let (id_bytes, src, score, cluster_id) = r.map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
            if let Ok(arr) = <[u8; 32]>::try_from(id_bytes.as_slice()) {
                let photo_id = photohelper_core::catalog_glue::photo_id_from_row_bytes(arr);
                let source_path = std::path::PathBuf::from(src);
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "NIMA scores in [1.0,10.0]; f64->f32 precision loss negligible"
                )]
                let nima_score = score.map(|v| v as f32);
                result.push(DevelopRow::new(
                    photo_id,
                    source_path,
                    nima_score,
                    cluster_id,
                ));
            } else {
                tracing::warn!(
                    path = %src,
                    len = id_bytes.len(),
                    "all_photos_with_cull_scores: malformed or corrupt photo ID BLOB; skipping row"
                );
            }
        }
        Ok(result)
    }

    ///
    /// Total rows in `photos` (visible to driver for summary tally).
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned`, `Error::CatalogOpen` for query failures.
    pub fn row_count(&self) -> Result<i64, Error> {
        let guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;
        guard
            .query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })
    }

    /// Fetch all rows ordered by `ingested_at_unix_seconds`. For tests +
    /// the `cli camera` / future diagnostic commands.
    ///
    /// # Errors
    /// - `Error::CatalogPoisoned` / `Error::CatalogOpen` for query failures.
    pub fn all_rows(&self) -> Result<Vec<PhotoRow>, Error> {
        let guard = self.conn.lock().map_err(|_| Error::CatalogPoisoned {
            path: self.canonical_path.clone(),
        })?;
        let sql =
            format!("SELECT {SELECT_ALL_COLUMNS} FROM photos ORDER BY ingested_at_unix_seconds");
        let mut stmt = guard.prepare(&sql).map_err(|e| Error::CatalogOpen {
            path: self.canonical_path.clone(),
            source: Box::new(e),
        })?;
        let rows = stmt
            .query_map([], PhotoRow::from_row)
            .map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::CatalogOpen {
                path: self.canonical_path.clone(),
                source: Box::new(e),
            })?);
        }
        Ok(out)
    }
}

/// Apply the v1 → v2 schema migration: add `cull_scores` table + index
/// and set `PRAGMA user_version = 2`. Wrapped in `BEGIN IMMEDIATE` to
/// match the init-path contract; `CREATE TABLE IF NOT EXISTS` makes the
/// DDL idempotent in case a prior run committed the table but crashed
/// before bumping `user_version`.
fn apply_v1_to_v2(conn: &mut Connection, path: &Path) -> Result<(), Error> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| Error::CatalogOpen {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
    tx.execute_batch(MIGRATE_V1_TO_V2_SQL)
        .map_err(|e| Error::CatalogOpen {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
    tx.commit().map_err(|e| Error::CatalogOpen {
        path: path.to_path_buf(),
        source: Box::new(e),
    })
}

/// Apply the v2 → v3 schema migration: add `embeddings` and `dup_clusters` tables,
/// then set `PRAGMA user_version = 3`. Wrapped in `BEGIN IMMEDIATE` matching the
/// v1→v2 convention; `CREATE TABLE IF NOT EXISTS` makes the DDL idempotent.
fn apply_v2_to_v3(conn: &mut Connection, path: &Path) -> Result<(), Error> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| Error::CatalogOpen {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
    tx.execute_batch(MIGRATE_V2_TO_V3_SQL)
        .map_err(|e| Error::CatalogOpen {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
    tx.commit().map_err(|e| Error::CatalogOpen {
        path: path.to_path_buf(),
        source: Box::new(e),
    })
}

/// Test-only extension methods (D5a — `poison_for_testing` knob).
#[cfg(test)]
impl Catalog {
    /// Poison the catalog's internal mutex for testing the poison-recovery path.
    ///
    /// Spawns a thread that acquires the lock and panics, permanently poisoning
    /// the mutex. After this call every `upsert` returns `Error::CatalogPoisoned`.
    ///
    /// Caller must hold an `Arc<Catalog>` so the inner Arc clone is safe.
    pub(crate) fn poison_for_testing(self: &std::sync::Arc<Self>) {
        let c = std::sync::Arc::clone(self);
        let h = std::thread::spawn(move || {
            let _guard = c.conn.lock().expect("mutex must not be pre-poisoned");
            panic!("intentional mutex poison for testing");
        });
        let _ = h.join(); // Err(_) expected — the thread panicked
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use photohelper_core::catalog_glue::photo_id_from_row_bytes;
    use photohelper_core::model::{AbsPath, ExifMetadata, Photo};
    use photohelper_test_helpers::HeartbeatDeathTrigger;

    use super::*;

    static_assertions::assert_impl_all!(Arc<Catalog>: Send, Sync);

    /// Create a minimal test photo at a specific source_path with a given id_seed.
    /// Used to create two Photos with the SAME source_path but DIFFERENT PhotoIds
    /// for supersession testing.
    fn make_test_photo_at_path(
        _dir: &std::path::Path,
        id_seed: u8,
        source_path: &std::path::Path,
    ) -> Photo {
        // Write different bytes to the file so content-change detection fires.
        std::fs::write(source_path, vec![id_seed; 2048]).expect("write test fixture");
        let abs = AbsPath::canonicalize(source_path).expect("canonicalize");
        let pid = photo_id_from_row_bytes([id_seed; 32]);
        Photo::from_filesystem(
            abs,
            2048,
            1_577_836_801,
            false,
            pid,
            None,
            ExifMetadata::default(),
        )
        .expect("valid test photo")
    }

    /// Create a minimal, unique test photo whose source_path is `dir/file.cr3`.
    /// `PhotoId` is synthesised from `id_seed` so callers can distinguish rows.
    fn make_test_photo(dir: &std::path::Path, id_seed: u8) -> Photo {
        let file_path = dir.join(format!("test_{id_seed}.cr3"));
        std::fs::write(&file_path, vec![id_seed; 1024]).expect("write test fixture");
        let abs = AbsPath::canonicalize(&file_path).expect("canonicalize");
        let pid = photo_id_from_row_bytes([id_seed; 32]);
        Photo::from_filesystem(
            abs,
            1024,
            1_577_836_800,
            false,
            pid,
            None,
            ExifMetadata::default(),
        )
        .expect("valid test photo")
    }

    #[test]
    fn open_rejects_path_is_directory() {
        let dir = tempfile::tempdir().unwrap();
        // Catalog path = the directory itself.
        let err = Catalog::open(dir.path(), 1).unwrap_err();
        assert!(matches!(err, Error::CatalogPathIsDirectory { .. }));
    }

    #[test]
    fn open_rejects_path_not_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let cat = dir.path().join("not_sqlite.db");
        std::fs::write(&cat, b"This is just a text file, definitely not SQLite").unwrap();
        let err = Catalog::open(&cat, 1).unwrap_err();
        assert!(matches!(err, Error::CatalogPathNotSqlite { .. }));
    }

    #[test]
    fn open_schema_version_too_new_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let cat = dir.path().join("ahead.db");
        // Build a DB with user_version = SCHEMA_VERSION + 1 (too new).
        // Using SCHEMA_VERSION + 1 so this test stays correct when the
        // constant is bumped in a future session (PR1-T19 fix).
        {
            let conn = Connection::open(&cat).unwrap();
            conn.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1))
                .unwrap();
        }
        let err = Catalog::open(&cat, 1).unwrap_err();
        assert!(
            matches!(
                err,
                Error::CatalogSchemaTooNew { found, expected }
                if found == SCHEMA_VERSION + 1 && expected == SCHEMA_VERSION
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn open_init_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let cat = dir.path().join("c.db");
        {
            let _c1 = Catalog::open(&cat, 1).unwrap();
        }
        let c2 = Catalog::open(&cat, 1).unwrap();
        // After second open, user_version must still be SCHEMA_VERSION (PR1-T25 fix).
        let v: i64 = c2
            .conn
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn catalog_fresh_db_initializes_to_v3() {
        let dir = tempfile::tempdir().unwrap();
        let cat = dir.path().join("fresh.db");
        let c = Catalog::open(&cat, 1).unwrap();
        let conn = c.conn.lock().unwrap();
        // Fresh catalog must be at SCHEMA_VERSION = 3.
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            v, SCHEMA_VERSION,
            "fresh DB must initialize to SCHEMA_VERSION"
        );
        // cull_scores table must exist (created by MIGRATE_V1_TO_V2_SQL).
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='table' AND name='cull_scores'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "cull_scores table must be present in a fresh v3 DB"
        );
        // embeddings table must exist (created by MIGRATE_V2_TO_V3_SQL).
        let count2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='table' AND name='embeddings'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count2, 1,
            "embeddings table must be present in a fresh v3 DB"
        );
    }

    // =========================================================
    // D5a: poison_for_testing + 3 poison-recovery tests
    // =========================================================

    #[test]
    fn poison_propagates_as_catalog_poisoned_error() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Arc::new(Catalog::open(dir.path().join("c.db"), 1).unwrap());
        cat.poison_for_testing();
        let photo = make_test_photo(dir.path(), 1);
        let err = cat.upsert(&photo, 0).unwrap_err();
        assert!(
            matches!(err, Error::CatalogPoisoned { .. }),
            "expected CatalogPoisoned after mutex poison, got {err:?}"
        );
    }

    #[test]
    fn poison_rollback_discards_panicked_workers_partial_insert() {
        // D5b fix: ROLLBACK after poison with no open txn must NOT propagate an error
        // (SQLITE_ERROR / extended_code 1 arm), so upsert returns CatalogPoisoned, not
        // CatalogTransaction. (Plan v4 cited ApiMisuse/SQLITE_MISUSE=21; empirical test
        // showed SQLite returns SQLITE_ERROR=1 for "no transaction is active".)
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("c.db");
        let cat = Arc::new(Catalog::open(&db_path, 1).unwrap());
        // Pre-insert a row so we can assert row count is unchanged after poison.
        cat.upsert(&make_test_photo(dir.path(), 1), 0).unwrap();
        cat.poison_for_testing();
        // This upsert triggers the D5b ROLLBACK + CatalogPoisoned path.
        let err = cat.upsert(&make_test_photo(dir.path(), 2), 0).unwrap_err();
        assert!(
            matches!(err, Error::CatalogPoisoned { .. }),
            "expected CatalogPoisoned (not CatalogTransaction), got {err:?}"
        );
        // The ROLLBACK must not have left the DB in a state that blocks re-open.
        drop(cat);
        let cat2 = Catalog::open(&db_path, 1).unwrap();
        assert_eq!(
            cat2.row_count().unwrap(),
            1,
            "only the pre-poison row must survive"
        );
    }

    #[test]
    fn poison_recovery_admits_subsequent_inserts() {
        // After a poisoned catalog is dropped, a fresh catalog at the same path
        // accepts new inserts — the ROLLBACK left the DB consistent.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("c.db");
        {
            let cat = Arc::new(Catalog::open(&db_path, 1).unwrap());
            cat.poison_for_testing();
            // Trigger the D5b ROLLBACK path.
            let _ = cat.upsert(&make_test_photo(dir.path(), 1), 0);
            // cat dropped here — file lock released.
        }
        let cat2 = Catalog::open(&db_path, 1).unwrap();
        cat2.upsert(&make_test_photo(dir.path(), 2), 0).unwrap();
        assert_eq!(
            cat2.row_count().unwrap(),
            1,
            "fresh catalog must accept inserts after poisoned one is dropped"
        );
    }

    // =========================================================
    // R2-C: insert_cull_score range guard tests
    // =========================================================

    #[test]
    fn insert_cull_score_rejects_out_of_range_values() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("c.db"), 1).unwrap();
        let photo = make_test_photo(dir.path(), 1);
        cat.upsert(&photo, 0).unwrap();
        let pid = photo.photo_id();
        // All of the following must be rejected.
        for bad_score in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            0.999,
            -1.0,
            10.001,
        ] {
            assert!(
                cat.insert_cull_score(pid, "nima-v1", bad_score, 0).is_err(),
                "score {bad_score} must be rejected"
            );
        }
        // Boundary values that MUST be accepted.
        assert_eq!(
            cat.insert_cull_score(pid, "boundary-min", 1.0, 0).unwrap(),
            InsertScoreOutcome::Inserted,
            "score 1.0 must be accepted"
        );
        assert_eq!(
            cat.insert_cull_score(pid, "boundary-max", 10.0, 0).unwrap(),
            InsertScoreOutcome::Inserted,
            "score 10.0 must be accepted"
        );
    }

    // =========================================================
    // R1-H: insert_cull_score poison path test
    // =========================================================

    #[test]
    fn insert_cull_score_poison_returns_catalog_poisoned() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Arc::new(Catalog::open(dir.path().join("c.db"), 1).unwrap());
        let photo = make_test_photo(dir.path(), 1);
        cat.upsert(&photo, 0).unwrap();
        cat.poison_for_testing();
        let err = cat
            .insert_cull_score(photo.photo_id(), "nima-v1", 5.0, 0)
            .unwrap_err();
        assert!(
            matches!(err, Error::CatalogPoisoned { .. }),
            "expected CatalogPoisoned after poison, got {err:?}"
        );
    }

    // =========================================================
    // D2b: cull_scores integration tests
    // =========================================================

    // =========================================================
    // D2a: schema v3 migration tests
    // =========================================================

    #[test]
    fn migration_v2_to_v3_reopen_succeeds() {
        // Build a v2 DB, open twice — second open must not fail or change version.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("v2.db");
        {
            // Create a v2 DB manually (INIT_SQL → user_version=1, then v1→v2).
            let mut conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(crate::schema::INIT_SQL).unwrap();
            apply_v1_to_v2(&mut conn, &db_path).unwrap();
        }
        // First open: applies v2→v3 migration. Drop the catalog (releases lock)
        // before the second open — two concurrent opens would deadlock on the lock.
        drop(Catalog::open(&db_path, 1).unwrap());
        // Second open: v3 already; must succeed with no error.
        let cat2 = Catalog::open(&db_path, 1).unwrap();
        let conn = cat2.conn.lock().unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            v, SCHEMA_VERSION,
            "second open must keep version at SCHEMA_VERSION"
        );
        // Both tables must exist.
        for table in &["embeddings", "dup_clusters"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    rusqlite::params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table} must be present after idempotent v2→v3");
        }
    }

    #[test]
    fn migration_chain_v1_to_v3() {
        // Build a v1 DB and verify opening upgrades all the way to v3.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("v1chain.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(crate::schema::INIT_SQL).unwrap();
        }
        let cat = Catalog::open(&db_path, 1).unwrap();
        let conn = cat.conn.lock().unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            v, SCHEMA_VERSION,
            "v1 DB must chain migrate to SCHEMA_VERSION"
        );
        // All three tables (photos, cull_scores, embeddings) must exist.
        for table in &["photos", "cull_scores", "embeddings", "dup_clusters"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    rusqlite::params![table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                count, 1,
                "{table} must be present after v1→v3 chain migration"
            );
        }
    }

    #[test]
    fn dup_clusters_fk_violation_rejects_nonexistent_embedding() {
        // Verify PRAGMA foreign_keys = ON enforces dup_clusters → embeddings FK.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("fk.db");
        let cat = Catalog::open(&db_path, 1).unwrap();
        let conn = cat.conn.lock().unwrap();
        // Insert a valid photo.
        let fake_photo_id = [0xAAu8; 32];
        conn.execute(
            "INSERT INTO photos (id, source_path, file_size, mtime_unix_seconds, \
             mtime_anomalous, ingested_at_unix_seconds) VALUES (?1, '/tmp/x.cr3', 1000, 1000, 0, 1000)",
            rusqlite::params![fake_photo_id.to_vec()],
        )
        .unwrap();
        // Attempting to insert into dup_clusters without a matching embeddings row
        // must fail with a FK violation (SqliteFailure with ConstraintViolation).
        let res = conn.execute(
            "INSERT INTO dup_clusters (photo_id, model_slug, cluster_id, similarity_threshold, \
             clustered_at_unix_seconds) VALUES (?1, 'clip-v1', 0, 0.95, 1000)",
            rusqlite::params![fake_photo_id.to_vec()],
        );
        assert!(
            res.is_err(),
            "FK violation: dup_clusters insert without matching embeddings must fail"
        );
        let err = res.unwrap_err();
        assert!(
            matches!(
                err,
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                        ..
                    },
                    _
                )
            ),
            "expected ConstraintViolation FK error, got: {err:?}"
        );
    }

    #[test]
    fn migration_v1_to_v2_upgrades_and_enforces_fk() {
        // Build a v1 DB (user_version = 1, only photos table).
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("v1.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            // Write v1 schema manually so we start at version 1.
            conn.execute_batch(crate::schema::INIT_SQL).unwrap();
        }
        // Open with the catalog — must migrate to v2.
        let cat = Catalog::open(&db_path, 1).unwrap();
        {
            let conn = cat.conn.lock().unwrap();
            let v: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, SCHEMA_VERSION, "catalog must upgrade v1 DB to v3");
        }

        // Insert a photos row so we have a valid photo_id for the FK.
        let photo = make_test_photo(dir.path(), 42);
        cat.upsert(&photo, 0).unwrap();
        let pid = photo.photo_id();

        // insert_cull_score with a valid photo_id → Inserted.
        let outcome = cat
            .insert_cull_score(pid, "nima-aesthetic-v1", 5.0, 1_000_000)
            .unwrap();
        assert_eq!(outcome, InsertScoreOutcome::Inserted);

        // Second insert with same photo_id + model_slug → AlreadyScored.
        let outcome2 = cat
            .insert_cull_score(pid, "nima-aesthetic-v1", 6.0, 2_000_000)
            .unwrap();
        assert_eq!(outcome2, InsertScoreOutcome::AlreadyScored);
        // R1-I: INSERT OR IGNORE must preserve first writer's score (5.0, not 6.0).
        {
            let conn = cat.conn.lock().unwrap();
            let stored: f64 = conn
                .query_row(
                    "SELECT aesthetic_score FROM cull_scores \
                     WHERE photo_id = ?1 AND model_slug = ?2",
                    rusqlite::params![&pid.as_bytes().to_vec(), "nima-aesthetic-v1"],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                (stored - 5.0).abs() < f64::EPSILON,
                "INSERT OR IGNORE must preserve first writer's score; got {stored}"
            );
        }

        // FK enforcement: insert with a non-existent photo_id fails.
        let fake_pid = photo_id_from_row_bytes([99u8; 32]);
        let err = cat
            .insert_cull_score(fake_pid, "nima-aesthetic-v1", 7.0, 3_000_000)
            .unwrap_err();
        assert!(
            matches!(err, Error::CatalogInsert { .. }),
            "FK violation must surface as CatalogInsert, got: {err:?}"
        );
    }

    #[test]
    fn unsuperseded_unscored_rows_excludes_scored_and_superseded() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Arc::new(Catalog::open(dir.path().join("c.db"), 1).unwrap());

        let p1 = make_test_photo(dir.path(), 1);
        let p2 = make_test_photo(dir.path(), 2);
        let p3 = make_test_photo(dir.path(), 3);
        cat.upsert(&p1, 1000).unwrap();
        cat.upsert(&p2, 2000).unwrap();
        cat.upsert(&p3, 3000).unwrap();

        let slug = "nima-aesthetic-v1";

        // Before any scores, all 3 rows are in the work list, oldest-first.
        let rows = cat.unsuperseded_unscored_rows(slug).unwrap();
        assert_eq!(rows.len(), 3);
        // R1-C: verify ORDER BY ingested_at_unix_seconds ordering.
        assert_eq!(
            rows[0].photo_id(),
            p1.photo_id(),
            "oldest ingest must be first"
        );
        assert_eq!(rows[1].photo_id(), p2.photo_id());
        assert_eq!(rows[2].photo_id(), p3.photo_id());
        // Score p1 — must disappear from slug's work list.
        cat.insert_cull_score(p1.photo_id(), slug, 5.0, 0).unwrap();
        let rows = cat.unsuperseded_unscored_rows(slug).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.photo_id() != p1.photo_id()));

        // R2-A (R1-D corrected): scoring p1 for model-A must NOT exclude p1 from
        // model-B's work list. This assertion only detects the bug when run AFTER
        // scoring, so the NOT IN subquery's model_slug filter is actually exercised.
        let rows_other = cat.unsuperseded_unscored_rows("other-model-v1").unwrap();
        assert_eq!(
            rows_other.len(),
            3,
            "p1 scored for nima-aesthetic-v1 must still appear in other-model-v1 work list"
        );
        assert!(
            rows_other.iter().any(|r| r.photo_id() == p1.photo_id()),
            "p1 must be present in other-model-v1 results despite being scored for nima-aesthetic-v1"
        );

        // Supersede p2 — mark superseded directly via SQL (no delete path in v0.1).
        {
            let conn = cat.conn.lock().unwrap();
            let id_bytes = p2.photo_id().as_bytes().to_vec();
            conn.execute(
                "UPDATE photos SET superseded_at_unix_seconds = 9999 WHERE id = ?1",
                rusqlite::params![&id_bytes],
            )
            .unwrap();
        }
        let rows = cat.unsuperseded_unscored_rows(slug).unwrap();
        assert_eq!(rows.len(), 1, "only p3 must remain");
        assert_eq!(rows[0].photo_id(), p3.photo_id());
    }

    // =========================================================
    // D5c-ii: HeartbeatDeathTrigger smoke test
    // =========================================================

    // =========================================================
    // D2b: embeddings + dup_clusters catalog API tests
    // =========================================================

    /// Helper: synthetic L2-normalized embedding bytes (512 dims, unit vector).
    fn make_unit_embedding_bytes(dim: usize) -> Vec<u8> {
        let val = 1.0_f32 / (dim as f32).sqrt();
        let mut bytes = Vec::with_capacity(dim * 4);
        for _ in 0..dim {
            bytes.extend_from_slice(&val.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn insert_embedding_happy_path_and_already_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("e.db"), 1).unwrap();
        let photo = make_test_photo(dir.path(), 1);
        cat.upsert(&photo, 0).unwrap();
        let pid = photo.photo_id();

        let emb_bytes = make_unit_embedding_bytes(512);

        // First insert → Inserted.
        let out = cat
            .insert_embedding(pid, "clip-v1", &emb_bytes, 512, 1_000_000)
            .unwrap();
        assert_eq!(out, InsertEmbeddingOutcome::Inserted);

        // Second insert with same (photo_id, model_slug) → AlreadyEmbedded.
        let out2 = cat
            .insert_embedding(pid, "clip-v1", &emb_bytes, 512, 2_000_000)
            .unwrap();
        assert_eq!(out2, InsertEmbeddingOutcome::AlreadyEmbedded);
    }

    #[test]
    fn unembedded_rows_excludes_embedded_and_superseded() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("u.db"), 1).unwrap();
        let p1 = make_test_photo(dir.path(), 1);
        let p2 = make_test_photo(dir.path(), 2);
        let p3 = make_test_photo(dir.path(), 3);
        cat.upsert(&p1, 0).unwrap();
        cat.upsert(&p2, 1).unwrap();
        cat.upsert(&p3, 2).unwrap();

        let emb = make_unit_embedding_bytes(512);

        // Embed p1 under clip-v1.
        cat.insert_embedding(p1.photo_id(), "clip-v1", &emb, 512, 1000)
            .unwrap();

        // Supersede p3 via raw SQL (no delete API in v0.1).
        {
            let conn = cat.conn.lock().unwrap();
            conn.execute(
                "UPDATE photos SET superseded_at_unix_seconds = 9999 WHERE id = ?1",
                rusqlite::params![p3.photo_id().as_bytes().to_vec()],
            )
            .unwrap();
        }

        // unembedded_rows must return only p2 (p1 embedded, p3 superseded).
        let rows = cat.unembedded_rows("clip-v1").unwrap();
        assert_eq!(
            rows.len(),
            1,
            "only p2 should be unembedded and non-superseded"
        );
        assert_eq!(rows[0].photo_id(), p2.photo_id());

        // For a different model slug, all non-superseded photos are returned.
        let rows2 = cat.unembedded_rows("other-model").unwrap();
        assert_eq!(
            rows2.len(),
            2,
            "both p1 and p2 unembedded under 'other-model'"
        );
    }

    #[test]
    fn all_embeddings_for_model_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("r.db"), 1).unwrap();
        let p1 = make_test_photo(dir.path(), 1);
        let p2 = make_test_photo(dir.path(), 2);
        cat.upsert(&p1, 0).unwrap();
        cat.upsert(&p2, 1).unwrap();

        let emb1 = make_unit_embedding_bytes(512);
        let mut emb2 = make_unit_embedding_bytes(512);
        emb2[0] = 0xFF; // make it distinct from emb1

        cat.insert_embedding(p1.photo_id(), "clip-v1", &emb1, 512, 1000)
            .unwrap();
        cat.insert_embedding(p2.photo_id(), "clip-v1", &emb2, 512, 2000)
            .unwrap();

        let results = cat.all_embeddings_for_model("clip-v1").unwrap();
        assert_eq!(results.len(), 2, "must retrieve both embeddings");

        // Verify bytes + dim round-trip correctly.
        let map: std::collections::HashMap<_, _> = results
            .into_iter()
            .map(|(pid, bytes, dim)| (pid, (bytes, dim)))
            .collect();
        let (bytes1, dim1) = &map[&p1.photo_id()];
        assert_eq!(bytes1, &emb1, "p1 embedding bytes must round-trip");
        assert_eq!(*dim1, 512_usize, "p1 dim must round-trip");
        let (bytes2, dim2) = &map[&p2.photo_id()];
        assert_eq!(bytes2, &emb2, "p2 embedding bytes must round-trip");
        assert_eq!(*dim2, 512_usize, "p2 dim must round-trip");

        // Different model → empty result.
        let empty = cat.all_embeddings_for_model("other-model").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn insert_dup_cluster_happy_path_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("d.db"), 1).unwrap();
        let p1 = make_test_photo(dir.path(), 1);
        cat.upsert(&p1, 0).unwrap();

        let emb = make_unit_embedding_bytes(512);
        cat.insert_embedding(p1.photo_id(), "clip-v1", &emb, 512, 1000)
            .unwrap();

        // First cluster assignment.
        cat.insert_dup_clusters(&[(p1.photo_id(), 0)], "clip-v1", 0.95, 2000)
            .unwrap();

        // Re-cluster with different cluster_id (INSERT OR REPLACE).
        cat.insert_dup_clusters(&[(p1.photo_id(), 7)], "clip-v1", 0.90, 3000)
            .unwrap();

        // Verify the second assignment replaced the first.
        let conn = cat.conn.lock().unwrap();
        let cluster_id: i64 = conn
            .query_row(
                "SELECT cluster_id FROM dup_clusters WHERE photo_id = ?1 AND model_slug = ?2",
                rusqlite::params![p1.photo_id().as_bytes().to_vec(), "clip-v1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            cluster_id, 7,
            "INSERT OR REPLACE must overwrite previous assignment"
        );
        // Verify other columns were also replaced (not just cluster_id).
        let (threshold, clustered_at): (f64, i64) = conn
            .query_row(
                "SELECT similarity_threshold, clustered_at_unix_seconds \
                 FROM dup_clusters WHERE photo_id = ?1 AND model_slug = ?2",
                rusqlite::params![p1.photo_id().as_bytes().to_vec(), "clip-v1"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        // 0.90_f32 cast to f64 is 0.8999999761581421; use f32 epsilon tolerance.
        assert!(
            (threshold - f64::from(0.90_f32)).abs() < f64::EPSILON,
            "similarity_threshold must be replaced to 0.90 (f32), got {threshold}"
        );
        assert_eq!(
            clustered_at, 3000,
            "clustered_at_unix_seconds must be replaced to 3000"
        );
    }

    #[test]
    fn insert_embedding_dim_zero_rejects_with_error() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("dim.db"), 1).unwrap();
        let photo = make_test_photo(dir.path(), 1);
        cat.upsert(&photo, 0).unwrap();

        // dim=0 must be caught by the Rust-level guard before INSERT OR IGNORE
        // can swallow the CHECK constraint violation.
        let err = cat
            .insert_embedding(photo.photo_id(), "clip-v1", &[0u8; 0], 0, 1000)
            .unwrap_err();
        assert!(
            matches!(err, Error::CatalogInsert { .. }),
            "dim=0 must return CatalogInsert, not AlreadyEmbedded"
        );
    }

    #[test]
    fn insert_embedding_dim_bounds_guard() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("bounds.db"), 1).unwrap();
        let photo = make_test_photo(dir.path(), 1);
        cat.upsert(&photo, 0).unwrap();
        let pid = photo.photo_id();

        // dim=65537 (above upper bound) must also be caught.
        let err = cat
            .insert_embedding(pid, "clip-v1", &make_unit_embedding_bytes(512), 65537, 1000)
            .unwrap_err();
        assert!(
            matches!(err, Error::CatalogInsert { .. }),
            "dim=65537 must return CatalogInsert, got {err:?}"
        );

        // dim=65536 (at upper bound) must succeed.
        let large_emb = make_unit_embedding_bytes(65536);
        let out = cat
            .insert_embedding(pid, "clip-v1", &large_emb, 65536, 1000)
            .unwrap();
        assert_eq!(out, InsertEmbeddingOutcome::Inserted);
    }

    #[test]
    fn insert_embedding_fk_violation_with_nonexistent_photo() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("fk2.db"), 1).unwrap();
        let fake_pid = photo_id_from_row_bytes([0xFFu8; 32]);
        let emb = make_unit_embedding_bytes(512);

        // SQLite docs: ON CONFLICT clause (OR IGNORE) does NOT apply to FOREIGN KEY
        // constraints — only to UNIQUE, NOT NULL, CHECK, and ROWID. So a FK violation
        // with INSERT OR IGNORE still fires as a ConstraintViolation error.
        let err = cat
            .insert_embedding(fake_pid, "clip-v1", &emb, 512, 1000)
            .unwrap_err();
        assert!(
            matches!(err, Error::CatalogInsert { .. }),
            "FK violation must surface as CatalogInsert, got: {err:?}"
        );
    }

    #[test]
    fn insert_dup_cluster_with_missing_embedding_fails() {
        // API-level test: insert_dup_cluster without matching embeddings row.
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("dcfk.db"), 1).unwrap();
        let photo = make_test_photo(dir.path(), 1);
        cat.upsert(&photo, 0).unwrap();
        // No embedding inserted → FK violation in dup_clusters.
        let err = cat
            .insert_dup_clusters(&[(photo.photo_id(), 0)], "clip-v1", 0.95, 1000)
            .unwrap_err();
        assert!(
            matches!(err, Error::CatalogInsert { .. }),
            "FK violation must surface as CatalogInsert, got: {err:?}"
        );
    }

    #[test]
    fn heartbeat_death_trigger_panics_and_join_returns_err() {
        // D5c-ii: verify the HeartbeatDeathTrigger helper itself works correctly.
        // A signalled trigger thread panics; join() returns Err; is_finished()
        // becomes true after signalling. This is the foundation for the in-process
        // heartbeat-death-WARN regression test (see D5e in session 03 plan §D5c-ii).
        let trigger = HeartbeatDeathTrigger::spawn();
        assert!(
            !trigger.is_finished(),
            "thread should be running before signal"
        );
        trigger.signal();
        // Spin until finished (tiny delay while the thread wakes and panics).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !trigger.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "trigger thread did not finish within 5s"
            );
            std::thread::yield_now();
        }
        let result = trigger.join();
        assert!(result.is_err(), "join must return Err after a panic");
    }

    // --- all_photos_with_cull_scores ---

    fn upsert_photo(cat: &Catalog, dir: &std::path::Path, seed: u8) -> Photo {
        let p = make_test_photo(dir, seed);
        cat.upsert(&p, 0).unwrap();
        p
    }

    #[test]
    fn all_photos_with_cull_scores_returns_all_non_superseded() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("d.db"), 1).unwrap();
        let p1 = upsert_photo(&cat, dir.path(), 1);
        let p2 = upsert_photo(&cat, dir.path(), 2);
        let rows = cat
            .all_photos_with_cull_scores("nima-v1", "clip-vit-b32")
            .unwrap();
        assert_eq!(rows.len(), 2, "both photos must appear");
        let paths: Vec<_> = rows.iter().map(|r| r.source_path().to_path_buf()).collect();
        assert!(paths.contains(&p1.source_path().to_path_buf()));
        assert!(paths.contains(&p2.source_path().to_path_buf()));
    }

    #[test]
    fn all_photos_with_cull_scores_nima_score_attached() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("d.db"), 1).unwrap();
        let p = upsert_photo(&cat, dir.path(), 1);
        // Insert a cull score.
        cat.insert_cull_score(p.photo_id(), "nima-v1", 7.5, 1000)
            .unwrap();
        let rows = cat
            .all_photos_with_cull_scores("nima-v1", "clip-vit-b32")
            .unwrap();
        assert_eq!(rows.len(), 1);
        let score = rows[0].nima_score();
        assert!(score.is_some(), "nima_score must be present");
        assert!((score.unwrap() - 7.5).abs() < 0.001);
    }

    #[test]
    fn all_photos_with_cull_scores_superseded_excluded() {
        // To test supersession, we need a second Photo with a DIFFERENT PhotoId
        // but the SAME source_path as p1. upsert() takes the existing_at_path branch
        // (line ~450) when source_path matches but PhotoId differs, marking the
        // first row superseded and inserting the second as the active row.
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("d.db"), 1).unwrap();
        let p1 = upsert_photo(&cat, dir.path(), 1);
        // p2 uses the same source_path as p1 but different content (seed=2 → different PhotoId).
        // This triggers supersession of p1.
        let p2 = make_test_photo_at_path(dir.path(), 2, p1.source_path());
        cat.upsert(&p2, 0).unwrap();
        let rows = cat
            .all_photos_with_cull_scores("nima-v1", "clip-vit-b32")
            .unwrap();
        // Only the non-superseded row (p2) should be returned.
        assert_eq!(rows.len(), 1, "superseded photo must be excluded");
        assert_eq!(
            rows[0].photo_id(),
            p2.photo_id(),
            "only the active row must be returned"
        );
    }
    #[test]
    fn all_photos_wrong_model_slug_returns_none_score() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("d.db"), 1).unwrap();
        let p = upsert_photo(&cat, dir.path(), 1);
        cat.insert_cull_score(p.photo_id(), "nima-v1", 7.5, 1000)
            .unwrap();
        // Query with a different model slug — should return None for nima_score.
        let rows = cat
            .all_photos_with_cull_scores("other-model", "clip-vit-b32")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].nima_score().is_none(),
            "wrong model slug must return None score"
        );
    }

    #[test]
    fn test_develop_row_retrieves_cluster_id() {
        let dir = tempfile::tempdir().unwrap();
        let cat = Catalog::open(dir.path().join("d.db"), 1).unwrap();
        let p = upsert_photo(&cat, dir.path(), 1);

        let dummy_embedding = vec![0.0f32; 512];
        let bytes: Vec<u8> = dummy_embedding
            .iter()
            .flat_map(|&f| f.to_ne_bytes())
            .collect();
        cat.insert_embedding(p.photo_id(), "clip-vit-b32", &bytes, 512, 1000)
            .unwrap();
        cat.insert_dup_clusters(&[(p.photo_id(), 42)], "clip-vit-b32", 0.95, 1000)
            .unwrap();

        let rows = cat
            .all_photos_with_cull_scores("nima-v1", "clip-vit-b32")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].dedup_cluster_id(), Some(42));
    }
}
