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
use photohelper_core::model::{AbsPath, ExifMetadata, KnownCamera, Photo, PhotoId};

use crate::row::{PhotoRow, SELECT_ALL_COLUMNS, insert_error};
use crate::schema::{INIT_SQL, SCHEMA_VERSION};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Production lock-retry delay between attempts. Tests override via the
/// public-but-`#[cfg(test)]`-only `with_retry_delay` constructor helper.
const LOCK_RETRY_DELAY: Duration = Duration::from_secs(5);

/// Outcome of [`Catalog::upsert`] for an `IngestStats` driver.
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
        let lock_file = File::create(&lock_path).map_err(|e| Error::Io {
            path: lock_path.clone(),
            op: "mkdir-p",
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
                    return Err(Error::Io {
                        path: lock_path,
                        op: "stat",
                        source: e,
                    });
                }
            }
        }

        // Step 5: verify existing catalog file's magic bytes.
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
                let tx = conn.transaction().map_err(|e| Error::CatalogOpen {
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
        let recovered: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(1))
            .unwrap_or(0);
        if recovered > 0 {
            tracing::warn!(
                frames = recovered,
                "previous shutdown was unclean; recovered N WAL frames"
            );
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
                // ROLLBACK any open transaction left by the panicked worker;
                // ignore errors — there may not be an open txn.
                let _ = conn.execute("ROLLBACK", []);
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
        let existing_by_id: Option<()> = tx
            .query_row(
                "SELECT 1 FROM photos WHERE id = ?1",
                rusqlite::params![&id_bytes],
                |_| Ok(()),
            )
            .ok();
        if existing_by_id.is_some() {
            tx.commit().map_err(|e| insert_error(pid, e))?;
            tracing::info!(
                photo_id = %pid,
                "same content already cataloged; INSERT OR IGNORE"
            );
            return Ok(UpsertOutcome::AlreadyCatalogued);
        }

        let source_path_str = photo.source_path().to_string_lossy().into_owned();
        let existing_at_path: Option<Vec<u8>> = tx
            .query_row(
                "SELECT id FROM photos
                   WHERE source_path = ?1 AND superseded_at_unix_seconds IS NULL",
                rusqlite::params![&source_path_str],
                |row| row.get(0),
            )
            .ok();

        let (camera_slug, _is_known) = match photo.camera_id() {
            Some(photohelper_core::model::CameraId::Known(k)) => (Some(k.slug().to_string()), true),
            _ => (None, false),
        };
        let exif = photo.exif();
        let exif_orientation_i64 = exif.orientation.map(|o| o.to_tag());
        let width_i64 = exif.width.map(i64::from);
        let height_i64 = exif.height.map(i64::from);
        let file_size_i64 = i64::try_from(photo.file_size()).unwrap_or(i64::MAX);

        let outcome = match existing_at_path {
            None => {
                tx.execute(
                    "INSERT INTO photos (
                        id, source_path, file_size, mtime_unix_seconds,
                        mtime_anomalous, make, model, camera_slug,
                        capture_time_unix_seconds, width, height,
                        exif_orientation, ingested_at_unix_seconds,
                        superseded_at_unix_seconds
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)",
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
                    "UPDATE photos SET superseded_at_unix_seconds = ?2
                       WHERE id = ?1",
                    rusqlite::params![&old_bytes, ingested_at_unix_seconds],
                )
                .map_err(|e| insert_error(pid, e))?;
                tx.execute(
                    "INSERT INTO photos (
                        id, source_path, file_size, mtime_unix_seconds,
                        mtime_anomalous, make, model, camera_slug,
                        capture_time_unix_seconds, width, height,
                        exif_orientation, ingested_at_unix_seconds,
                        superseded_at_unix_seconds
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)",
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

// Avoid unused-import warnings for re-exports the linker would otherwise prune.
#[allow(dead_code)]
fn _ensure_exif_metadata_compiles(_x: ExifMetadata, _k: KnownCamera) {}

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_impl_all!(std::sync::Arc<Catalog>: Send, Sync);

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
}
