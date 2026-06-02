//! `photohelper rename` — copy RAW+XMP into `--output` under catalog-driven prefixed names.
//!
//! Pipeline:
//! 1. Validate args: canonicalize source/output.
//! 2. Open catalog; query `all_photos_with_cull_scores` (ordered by ingested_at, p.id).
//! 3. Filter rows by canonical source-path prefix.
//! 4. Build output filenames via `RenamedFilename` builder + collision resolution.
//! 5. Per-row: existence pre-check → atomic copy (RAW tmp, optional XMP tmp) → commit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use anyhow::Context as _;
use photohelper_ai::{CLIP_MODEL_SLUG, MODEL_SLUG};
use photohelper_catalog::Catalog;
use rayon::prelude::*;

use crate::Cli;
use crate::commands::util::{TempFileGuard, format_nima_score_label, resolve_collisions};
use crate::exit_code;
use crate::heartbeat::{HeartbeatStop, heartbeat_interval, run_heartbeat_loop};

// NAME_MAX on most POSIX filesystems and NTFS.
const NAME_MAX: usize = 255;

/// Clap args for `photohelper rename`.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct RenameArgs {
    /// Source directory to filter rows against (read-only).
    #[arg(long)]
    pub(crate) source: PathBuf,

    /// Output directory for renamed copies.
    #[arg(long)]
    pub(crate) output: PathBuf,

    /// Overwrite existing output files.
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,

    /// Treat any single-row failure as immediately fatal.
    #[arg(long, default_value_t = false)]
    pub(crate) strict: bool,
}

struct RenameStats {
    matched: AtomicU64,
    renamed: AtomicU64,
    sidecar_copied: AtomicU64,
    sidecar_absent: AtomicU64,
    sidecar_copy_failed: AtomicU64,
    file_missing: AtomicU64,
    errored: AtomicU64,
}

impl RenameStats {
    fn new() -> Self {
        Self {
            matched: AtomicU64::new(0),
            renamed: AtomicU64::new(0),
            sidecar_copied: AtomicU64::new(0),
            sidecar_absent: AtomicU64::new(0),
            sidecar_copy_failed: AtomicU64::new(0),
            file_missing: AtomicU64::new(0),
            errored: AtomicU64::new(0),
        }
    }

    fn summary_line(&self) -> String {
        format!(
            "matched: {}, renamed: {}, sidecar-copied: {}, sidecar-absent: {}, \
             sidecar-copy-failed: {}, file-missing: {}, errored: {}",
            self.matched.load(Ordering::Relaxed),
            self.renamed.load(Ordering::Relaxed),
            self.sidecar_copied.load(Ordering::Relaxed),
            self.sidecar_absent.load(Ordering::Relaxed),
            self.sidecar_copy_failed.load(Ordering::Relaxed),
            self.file_missing.load(Ordering::Relaxed),
            self.errored.load(Ordering::Relaxed),
        )
    }

    fn total_failures(&self) -> u64 {
        self.sidecar_copy_failed.load(Ordering::Relaxed)
            + self.file_missing.load(Ordering::Relaxed)
            + self.errored.load(Ordering::Relaxed)
    }
}

/// Validated output filename for a renamed RAW file.
///
/// Format: `Cluster-{X}_Cull-{Y}-{sanitized_stem}.{ext}` where:
/// - `X` = zero-padded cluster id (3+ digits, e.g. `003`)
/// - `Y` = zero-padded NIMA score (`{:05.2}`, e.g. `07.85`)
/// - Stem is sanitized (rejects path separators, NUL, control chars)
/// - Composed name is capped so prefix + `_N` suffix + ext always fit `NAME_MAX`
///
/// `None` cluster or score use named sentinels (`Cluster-NONE`, `Cull-NONE`).
pub struct RenamedFilename {
    /// The composed output filename (stem + ext, no directory component).
    name: String,
}

impl RenamedFilename {
    /// Construct a `RenamedFilename` for the given row fields.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the stem contains path separators, NUL, or control characters.
    pub fn build(
        cluster_id: Option<i64>,
        nima_score: Option<f32>,
        original_path: &Path,
    ) -> Result<Self, RenameError> {
        let raw_stem = original_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("photo");
        let ext = original_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        // Sanitize stem: reject separators, NUL, control characters.
        let sanitized = sanitize_stem(raw_stem)?;

        // Build prefix.
        let cluster_part = match cluster_id {
            Some(id) if id >= 0 => format!("Cluster-{id:03}"),
            // Negative id or None both use the sentinel.
            _ => "Cluster-NONE".to_string(),
        };
        let cull_part = match nima_score {
            Some(s) if s.is_finite() && !s.is_nan() => {
                format!("Cull-{}", format_nima_score_label(s))
            }
            _ => "Cull-NONE".to_string(),
        };
        let prefix = format!("{cluster_part}_{cull_part}-");

        // Reserve space for collision suffix `_N` (up to 4 chars) and extension.
        let ext_with_dot = if ext.is_empty() {
            String::new()
        } else {
            format!(".{ext}")
        };
        // Max stem bytes = NAME_MAX - prefix.len() - ext_with_dot.len() - 4 (for _NNN)
        let max_stem = NAME_MAX
            .saturating_sub(prefix.len())
            .saturating_sub(ext_with_dot.len())
            .saturating_sub(4);

        let stem_capped = cap_utf8(&sanitized, max_stem);
        let name = format!("{prefix}{stem_capped}{ext_with_dot}");

        Ok(Self { name })
    }

    /// The composed filename (no directory component).
    pub fn as_str(&self) -> &str {
        &self.name
    }
}

/// Error from [`RenamedFilename::build`].
#[derive(Debug)]
pub enum RenameError {
    /// Stem contains a path separator, NUL, or control character.
    ForbiddenChar {
        /// The offending stem.
        stem: String,
    },
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForbiddenChar { stem } => {
                write!(f, "filename stem contains forbidden character: {stem:?}")
            }
        }
    }
}

impl std::error::Error for RenameError {}

/// Reject stems containing path separators, NUL, or control characters.
fn sanitize_stem(stem: &str) -> Result<String, RenameError> {
    for ch in stem.chars() {
        if ch == '/' || ch == '\\' || ch == '\0' || ch.is_control() {
            return Err(RenameError::ForbiddenChar {
                stem: stem.to_string(),
            });
        }
    }
    Ok(stem.to_string())
}

/// Truncate `s` to at most `max_bytes` UTF-8 bytes (clean codepoint boundary).
fn cap_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Driver for `photohelper rename`.
///
/// # Errors
///
/// Returns `Err` for fatal setup failures (catalog open, directory creation).
pub fn run_rename(cli: &Cli, args: &RenameArgs) -> anyhow::Result<u8> {
    // 1. Canonicalize source.
    let source_canonical = dunce::canonicalize(&args.source)
        .with_context(|| format!("source directory not found: {}", args.source.display()))?;

    std::fs::create_dir_all(&args.output).with_context(|| {
        format!(
            "failed to create output directory: {}",
            args.output.display()
        )
    })?;
    let output_canonical = dunce::canonicalize(&args.output)
        .with_context(|| format!("cannot canonicalize output: {}", args.output.display()))?;

    // 2. Open catalog and query (ORDER BY ingested_at, p.id).
    let catalog_path = cli.catalog.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".photohelper")
            .join("catalog.db")
    });

    let all_rows = {
        let catalog = Catalog::open(&catalog_path, cli.catalog_lock_timeout_seconds)
            .with_context(|| format!("opening catalog at {}", catalog_path.display()))?;
        catalog
            .all_photos_with_cull_scores(MODEL_SLUG, CLIP_MODEL_SLUG)
            .with_context(|| "querying catalog for active photos")?
    };

    // 3. Filter by canonical source-path prefix.
    let rows: Vec<_> = all_rows
        .into_iter()
        .filter(|r| r.source_path().starts_with(&source_canonical))
        .collect();

    if rows.is_empty() {
        eprintln!(
            "matched: 0, renamed: 0, sidecar-copied: 0, sidecar-absent: 0, \
             sidecar-copy-failed: 0, file-missing: 0, errored: 0"
        );
        return Ok(0);
    }

    // 4. Build output filenames with collision resolution.
    // Pipeline order: sanitize → compose → cap-stem → resolve_collisions (key on final bytes).
    let source_paths: Vec<PathBuf> = rows.iter().map(|r| r.source_path().to_path_buf()).collect();

    // Build a lookup map from source path to row (for use inside the name_fn closure).
    let row_map: HashMap<PathBuf, &photohelper_catalog::DevelopRow> = rows
        .iter()
        .map(|r| (r.source_path().to_path_buf(), r))
        .collect();

    let collision_map = resolve_collisions(&output_canonical, &source_paths, |src| {
        // row_map was built from source_paths; the Option is structurally guaranteed
        // present because source_paths and row_map share the same key set.
        let Some(row) = row_map.get(src) else {
            return "photo.CR3".to_string();
        };
        match RenamedFilename::build(row.dedup_cluster_id(), row.nima_score(), src) {
            Ok(rf) => rf.as_str().to_string(),
            Err(e) => {
                // Stem had a forbidden character; fall back to the plain filename.
                tracing::warn!(
                    path = %src.display(),
                    error = %e,
                    "stem sanitization failed; falling back to plain filename (Cluster/Cull prefix lost)"
                );
                src.file_name()
                    .map_or_else(|| "photo".to_string(), |n| n.to_string_lossy().into_owned())
            }
        }
    });

    // 5. Parallel per-row copy pipeline.
    let stats = Arc::new(RenameStats::new());
    let cancelled = Arc::new(AtomicBool::new(false));

    let stop = Arc::new(HeartbeatStop::new());
    let heartbeat_handle = {
        let stats = Arc::clone(&stats);
        let stop = Arc::clone(&stop);
        let interval = heartbeat_interval();
        std::thread::Builder::new()
            .name("ph-heartbeat".into())
            .spawn(move || {
                run_heartbeat_loop(&stop, interval, || {
                    eprintln!(
                        "[heartbeat] rename: matched {}, renamed {}, errored {}",
                        stats.matched.load(Ordering::Relaxed),
                        stats.renamed.load(Ordering::Relaxed),
                        stats.errored.load(Ordering::Relaxed),
                    );
                });
            })
            .context("spawning heartbeat thread")?
    };

    rows.par_iter().for_each(|row| {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        stats.matched.fetch_add(1, Ordering::Relaxed);

        let src_path = row.source_path();

        // Existence pre-check.
        if !src_path.exists() {
            tracing::warn!(path = %src_path.display(), "source RAW missing; skipping");
            stats.file_missing.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }

        let Some(final_raw_path) = collision_map.get(src_path) else {
            tracing::error!(path = %src_path.display(), "source path not in collision map");
            stats.errored.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        };

        // Destination containment check (lexical: parent must be output_canonical).
        if final_raw_path.parent() != Some(output_canonical.as_path()) {
            tracing::error!(
                path = %final_raw_path.display(),
                "output path escape detected; skipping"
            );
            stats.errored.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }

        if final_raw_path.exists() && !args.force {
            // Already exists and --force not set → skip (not a failure).
            return;
        }

        // Prepare sidecar paths.
        let src_xmp = src_path.with_extension("xmp");
        let final_xmp_path = final_raw_path.with_extension("xmp");

        // Temp paths: append ".tmp" to the full filename (not replace extension)
        // so raw and xmp temps are always distinct.
        let raw_name = final_raw_path.file_name().map_or_else(
            || "raw.tmp".to_string(),
            |n| format!("{}.tmp", n.to_string_lossy()),
        );
        let xmp_name = final_xmp_path.file_name().map_or_else(
            || "xmp.tmp".to_string(),
            |n| format!("{}.tmp", n.to_string_lossy()),
        );
        let raw_tmp = output_canonical.join(raw_name);
        let xmp_tmp = output_canonical.join(xmp_name);

        // Phase 1: copy RAW → raw.tmp
        let mut raw_guard = TempFileGuard::new(raw_tmp.clone());
        if let Err(e) = std::fs::copy(src_path, &raw_tmp) {
            tracing::warn!(
                src = %src_path.display(),
                dst = %raw_tmp.display(),
                error = %e,
                "failed to copy RAW"
            );
            stats.errored.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }

        // Normalize mode so --force re-runs don't fail on read-only source mode.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&raw_tmp, std::fs::Permissions::from_mode(0o644));
        }

        // Phase 2: copy XMP sidecar → xmp.tmp (if present).
        // xmp_guard is declared here (outside the if-block) so its lifetime
        // extends past Phase 3's rename attempt — ensuring Drop cleans up on
        // rename failure rather than leaking the .tmp file.
        let mut xmp_guard: Option<TempFileGuard> = None;
        let sidecar_result = if src_xmp.exists() {
            let guard = TempFileGuard::new(xmp_tmp.clone());
            match std::fs::copy(&src_xmp, &xmp_tmp) {
                Ok(_) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(
                            &xmp_tmp,
                            std::fs::Permissions::from_mode(0o644),
                        );
                    }
                    // Do NOT commit yet; keep guard armed until after xmp rename.
                    xmp_guard = Some(guard);
                    Some(Ok(()))
                }
                Err(e) => {
                    // guard drops uncommitted here, cleaning up xmp_tmp.
                    tracing::warn!(
                        src = %src_xmp.display(),
                        dst = %xmp_tmp.display(),
                        error = %e,
                        "sidecar copy failed"
                    );
                    Some(Err(()))
                }
            }
        } else {
            None
        };

        // Phase 3: commit only if both temps exist (or no sidecar).
        if matches!(sidecar_result, Some(Err(()))) {
            // Sidecar copy failed → do not commit RAW; drop guards clean up.
            stats.sidecar_copy_failed.fetch_add(1, Ordering::Relaxed);
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }

        if let Err(e) = std::fs::rename(&raw_tmp, final_raw_path) {
            tracing::warn!(
                from = %raw_tmp.display(),
                to = %final_raw_path.display(),
                error = %e,
                "rename RAW tmp → final failed"
            );
            stats.errored.fetch_add(1, Ordering::Relaxed);
            // raw_guard.drop() cleans up raw_tmp automatically.
            // xmp_guard.drop() (if Some) cleans up xmp_tmp automatically.
            if args.strict {
                cancelled.store(true, Ordering::Relaxed);
            }
            return;
        }
        raw_guard.commit(); // Disarm RAW cleanup only after rename succeeds.

        // Commit sidecar if we had one.
        match sidecar_result {
            Some(Ok(())) => {
                if let Err(e) = std::fs::rename(&xmp_tmp, &final_xmp_path) {
                    tracing::warn!(
                        from = %xmp_tmp.display(),
                        to = %final_xmp_path.display(),
                        raw_path = %final_raw_path.display(),
                        error = %e,
                        "XMP sidecar rename failed; RAW is present but sidecar is missing — copy manually"
                    );
                    stats.sidecar_copy_failed.fetch_add(1, Ordering::Relaxed);
                    // RAW was renamed successfully; count it in renamed.
                    stats.renamed.fetch_add(1, Ordering::Relaxed);
                    // xmp_guard.drop() cleans up xmp_tmp automatically.
                    if args.strict {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                } else {
                    if let Some(mut g) = xmp_guard.take() {
                        g.commit(); // Disarm XMP cleanup only after rename succeeds.
                    }
                    stats.sidecar_copied.fetch_add(1, Ordering::Relaxed);
                    stats.renamed.fetch_add(1, Ordering::Relaxed);
                }
            }
            None => {
                stats.sidecar_absent.fetch_add(1, Ordering::Relaxed);
                stats.renamed.fetch_add(1, Ordering::Relaxed);
            }
            Some(Err(())) => {
                // Structurally unreachable: the sidecar_copy_failed early-return
                // above guards this arm. Defensive non-panic fallback.
                tracing::error!("BUG: Some(Err(())) arm reached in rename sidecar match");
                stats.errored.fetch_add(1, Ordering::Relaxed);
                if args.strict {
                    cancelled.store(true, Ordering::Relaxed);
                }
            }
        }
    });

    // Heartbeat shutdown.
    if heartbeat_handle.is_finished() {
        tracing::warn!("heartbeat thread died before end-of-rename");
    }
    stop.signal();
    if let Err(e) = heartbeat_handle.join() {
        tracing::error!("heartbeat thread panicked: {:?}", e);
    }

    eprintln!("{}", stats.summary_line());

    let total = stats.total_failures();
    if total > 0 {
        if args.strict {
            return Ok(exit_code::EX_STRICT_FAIL);
        }
        return Ok(exit_code::EX_PARTIAL_FAIL);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RenamedFilename construction tests ──

    #[test]
    fn renamed_filename_with_cluster_and_score() {
        let p = Path::new("/photos/IMG_1234.CR3");
        let rf = RenamedFilename::build(Some(3), Some(7.85), p).unwrap();
        assert_eq!(rf.as_str(), "Cluster-003_Cull-07.85-IMG_1234.CR3");
    }

    #[test]
    fn renamed_filename_none_cluster_none_score() {
        let p = Path::new("/photos/IMG_0001.cr3");
        let rf = RenamedFilename::build(None, None, p).unwrap();
        assert_eq!(rf.as_str(), "Cluster-NONE_Cull-NONE-IMG_0001.cr3");
    }

    #[test]
    fn renamed_filename_nan_score_becomes_none() {
        let p = Path::new("a.CR3");
        let rf = RenamedFilename::build(Some(1), Some(f32::NAN), p).unwrap();
        assert_eq!(rf.as_str(), "Cluster-001_Cull-NONE-a.CR3");
    }

    #[test]
    fn renamed_filename_large_cluster_id() {
        let p = Path::new("x.CR3");
        let rf = RenamedFilename::build(Some(1234), Some(5.0), p).unwrap();
        // 1234 > 3 digits, so it uses {:03} which pads to at least 3: "1234"
        assert_eq!(rf.as_str(), "Cluster-1234_Cull-05.00-x.CR3");
    }

    #[test]
    fn renamed_filename_negative_cluster_id_becomes_none() {
        let p = Path::new("x.CR3");
        let rf = RenamedFilename::build(Some(-1), Some(4.0), p).unwrap();
        assert_eq!(rf.as_str(), "Cluster-NONE_Cull-04.00-x.CR3");
    }

    #[test]
    fn renamed_filename_stem_with_separator_is_rejected() {
        // The sanitize_stem function rejects '/' in the stem string.
        let result = sanitize_stem("foo/bar");
        assert!(result.is_err(), "stem with '/' must be rejected");
        let result2 = sanitize_stem("foo\0bar");
        assert!(result2.is_err(), "stem with NUL must be rejected");
    }

    #[test]
    fn renamed_filename_sort_order_none_before_some() {
        let p = Path::new("x.CR3");
        let none_rf = RenamedFilename::build(None, None, p).unwrap();
        let some_rf = RenamedFilename::build(Some(1), Some(5.0), p).unwrap();
        // "Cluster-NONE" < "Cluster-001" lexicographically (N < 0 in ASCII)
        // Actually 'N' = 78, '0' = 48 → '0' < 'N', so Cluster-001 < Cluster-NONE
        // This matches spec: named sentinels sort after numeric ids
        assert!(
            none_rf.as_str() > some_rf.as_str(),
            "Cluster-NONE sorts after Cluster-001"
        );
    }

    #[test]
    fn cap_utf8_does_not_split_codepoint() {
        let s = "héllo"; // 'é' is 2 bytes in UTF-8
        let capped = cap_utf8(s, 3);
        // First 3 bytes: 'h' (1) + 'é' (2) = 3 bytes = "hé"
        assert_eq!(capped, "hé");
        let capped4 = cap_utf8(s, 4);
        // 4 bytes: 'h' + 'é' + 'l' = wait, 'h'=1, 'é'=2, 'l'=1 → 4 bytes = "hél"
        assert_eq!(capped4, "hél");
    }

    #[test]
    fn two_distinct_stems_that_truncate_identically_produce_distinct_outputs() {
        // Create two long stems that both cap to the same bytes.
        // Use resolve_collisions to verify they get different collision suffixes.
        let dir = PathBuf::from("/out");
        let stem = "a".repeat(300);
        let s1 = PathBuf::from(format!("/src/{stem}A.CR3"));
        let s2 = PathBuf::from(format!("/src/{stem}B.CR3"));
        let items = vec![s1.clone(), s2.clone()];
        let map = resolve_collisions(&dir, &items, |src| {
            let rf = RenamedFilename::build(Some(1), Some(5.0), src).unwrap();
            rf.as_str().to_string()
        });
        assert_ne!(
            map[&s1], map[&s2],
            "truncated-identical stems must get distinct output paths"
        );
    }
}
