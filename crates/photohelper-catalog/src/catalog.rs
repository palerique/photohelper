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

use crate::row::{CullRow, PhotoRow, SELECT_ALL_COLUMNS, insert_error};
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
        let max_attempts =
            ((u64::from(lock_timeout_seconds)) / retry_delay.as_secs().max(1)).max(1) as u32;
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
            "INSERT OR IGNORE INTO cull_scores \
             (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) \
             VALUES (?1, ?2, ?3, ?4)",
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

    /// Borrow the canonical catalog path.
    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

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
    fn catalog_fresh_db_initializes_to_v2() {
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
    fn migration_v2_to_v3_is_idempotent() {
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
}
