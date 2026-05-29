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

use crate::row::{PhotoRow, SELECT_ALL_COLUMNS, insert_error};
use crate::schema::{INIT_SQL, SCHEMA_VERSION};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// 13-column INSERT used by both the `Inserted` and `SupersededPrevious`
/// arms of `Catalog::upsert`. Extracted in R1.T14 to eliminate duplicate-
/// statement drift risk.
const INSERT_PHOTO_SQL: &str = "INSERT INTO photos (
    id, source_path, file_size, mtime_unix_seconds,
    mtime_anomalous, make, model, camera_slug,
    capture_time_unix_seconds, width, height,
    exif_orientation, ingested_at_unix_seconds,
    superseded_at_unix_seconds
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)";

/// Production lock-retry delay between attempts. Tests override via the
/// public-but-`#[cfg(test)]`-only `with_retry_delay` constructor helper.
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

    /// Test-only constructor allowing a shorter retry delay. Behind
    /// `#[doc(hidden)]` to discourage production callers.
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
                // R2-T8 fix: use IMMEDIATE so init takes the RESERVED lock
                // up-front, matching the prose contract in
                // `docs/decisions/0001-catalog-schema-v1.md` § Init transaction.
                // The file-lock already serialises openers (so DEFERRED would
                // be safe), but IMMEDIATE makes the SQLite-level intent
                // explicit and matches the upsert path at line ~291.
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
            }
            n if n == SCHEMA_VERSION => {}
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
            |_| catalog_path.to_path_buf(),
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
                // ApiMisuse (SQLITE_MISUSE) and "no transaction is active" both
                // indicate no work to undo — ignore. Any other error is unexpected
                // and propagated so the caller can log it.
                match conn.execute("ROLLBACK", []) {
                    Ok(_) => {}
                    // extended_code 1 = SQLITE_ERROR; SQLite returns this with the
                    // message "cannot rollback - no transaction is active" when
                    // the panicked worker held the lock but had no open txn.
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
        // previously this was a 13-column INSERT duplicated twice
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
        // Build a DB with user_version = 2 (newer than SCHEMA_VERSION = 1).
        {
            let conn = Connection::open(&cat).unwrap();
            conn.execute_batch("PRAGMA user_version = 2").unwrap();
        }
        let err = Catalog::open(&cat, 1).unwrap_err();
        assert!(matches!(
            err,
            Error::CatalogSchemaTooNew {
                found: 2,
                expected: 1
            }
        ));
    }

    #[test]
    fn open_init_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let cat = dir.path().join("c.db");
        {
            let _c1 = Catalog::open(&cat, 1).unwrap();
        }
        let c2 = Catalog::open(&cat, 1).unwrap();
        // After second open, user_version should still be 1.
        let v: i64 = c2
            .conn
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
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
        // (ApiMisuse arm), so upsert returns CatalogPoisoned, not CatalogTransaction.
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
}
