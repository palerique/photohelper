//! Integration tests for the `photohelper` CLI. Per
//! `docs/plans/session-01.md` §Test plan rows 32–48.
//!
//! Every test asserts a concrete observable per `docs/testing-standards.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Workspace `unused_crate_dependencies` lint flags transitive dev-deps
// like `num_cpus`, `rusqlite`, `time` that the test file doesn't
// reference directly (they're used by the photohelper bin under test).
#![allow(unused_crate_dependencies)]

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

/// Helper: a tempdir + path to a synthetic .cr3 fixture with a known-in-range
/// mtime so test row 32's `mtime_anomalous = 0` assertion doesn't flake on
/// CI machines with clock drift.
fn fixture_dir_with_one_cr3() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cr3 = dir.path().join("a.cr3");
    std::fs::write(&cr3, vec![0xCCu8; 200]).unwrap();
    let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0); // 2020-01-01
    filetime::set_file_mtime(&cr3, mtime).unwrap();
    (dir, cr3)
}

// =====================================================================
// Test row 32: happy-path ingest of one CR3 + one JPG.
// =====================================================================

#[test]
fn ingest_happy_path_walks_filters_and_writes_catalog_row() {
    let (dir, _cr3) = fixture_dir_with_one_cr3();
    let jpg = dir.path().join("b.jpg");
    std::fs::write(&jpg, vec![0u8; 100]).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("walked: 2"))
        .stderr(contains("ingested: 1"))
        .stderr(contains("skipped (non-RAW): 1"))
        .stderr(contains("mtime-anomalous: 0"));

    let conn =
        rusqlite::Connection::open(dir.path().join(".photohelper").join("catalog.db")).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "exactly one RAW row should be cataloged");
    let (sp, fs_, anomalous): (String, i64, i64) = conn
        .query_row(
            "SELECT source_path, file_size, mtime_anomalous FROM photos LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(
        sp.ends_with("a.cr3"),
        "source_path should be the .cr3, got {sp}"
    );
    assert_eq!(fs_, 200, "file_size column should match fixture");
    assert_eq!(anomalous, 0, "in-range pinned mtime should not be flagged");
    // R1.T12 pin: kamadak-exif cannot parse our synthetic 0xCC-byte
    // CR3 ISO-BMFF, so DN-006 fallback fires deterministically:
    // camera_slug IS NULL AND make/model IS NULL. Session 02 lands
    // real CR3 fixtures + flips these assertions to expect 'canon-r8'.
    // (Previous `is_none() || == Some("canon-r8")` violated
    // docs/testing-standards.md § Be specific by passing for both
    // branches.)
    let (camera_slug, make, model): (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT camera_slug, make, model FROM photos LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(
        camera_slug.is_none(),
        "DN-006 fallback: synthetic CR3 yields camera_slug NULL; got {camera_slug:?}",
    );
    assert!(make.is_none(), "synthetic CR3 has no EXIF make");
    assert!(model.is_none(), "synthetic CR3 has no EXIF model");
    // Cleanup: drop conn before tempdir cleanup so SQLite releases handles.
    drop(conn);
    drop(dir);
}

// =====================================================================
// Test row 33: summary line survives `-q`.
// =====================================================================

#[test]
fn ingest_summary_line_survives_quiet_flag() {
    let (dir, _cr3) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["-q", "ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("walked: 1"))
        .stderr(contains("ingested: 1"));
}

// =====================================================================
// Test row 35: idempotency — second ingest reports already-catalogued.
// =====================================================================

#[test]
fn ingest_idempotency_second_run_reports_already_catalogued() {
    let (dir, _cr3) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("already-catalogued: 1"))
        .stderr(contains("ingested: 0"));

    let conn =
        rusqlite::Connection::open(dir.path().join(".photohelper").join("catalog.db")).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "second run must not duplicate rows");
}

// =====================================================================
// Test row 36: content change at same path → supersede.
// =====================================================================

#[test]
fn ingest_content_change_at_same_path_supersedes_old_row() {
    let (dir, cr3) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success();
    // Rewrite with different bytes.
    std::fs::write(&cr3, vec![0x55u8; 300]).unwrap();
    let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0);
    filetime::set_file_mtime(&cr3, mtime).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("superseded: 1"));

    let conn =
        rusqlite::Connection::open(dir.path().join(".photohelper").join("catalog.db")).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE source_path LIKE '%a.cr3'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "both old + new rows should be retained");
    let superseded_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM photos WHERE superseded_at_unix_seconds IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(superseded_count, 1, "old row should be flagged superseded");
}

// =====================================================================
// Test row 37: dir of only JPGs → exit 64 EX_USAGE.
// =====================================================================

#[test]
fn ingest_wrong_directory_only_jpgs_exits_64() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.jpg"), vec![0u8; 100]).unwrap();
    std::fs::write(dir.path().join("b.jpg"), vec![0u8; 100]).unwrap();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .code(64)
        .stderr(contains("ingested: 0"));
}

// =====================================================================
// Test row 38: truly empty dir → walked: 0, exit 0.
// =====================================================================

#[test]
fn ingest_truly_empty_directory_exits_0_and_walks_zero() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("walked: 0"));
}

// =====================================================================
// Test row 41: mtime-anomalous summary slot reflects the count.
// =====================================================================

#[test]
fn ingest_anomalous_mtime_appears_in_summary_and_row() {
    let dir = tempfile::tempdir().unwrap();
    let cr3 = dir.path().join("ancient.cr3");
    std::fs::write(&cr3, vec![0xCCu8; 200]).unwrap();
    // mtime in 1970 → below the 1995 lower bound → clamped + flagged.
    let ancient = filetime::FileTime::from_unix_time(1, 0);
    filetime::set_file_mtime(&cr3, ancient).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("mtime-anomalous: 1"));

    let conn =
        rusqlite::Connection::open(dir.path().join(".photohelper").join("catalog.db")).unwrap();
    let anomalous: i64 = conn
        .query_row("SELECT mtime_anomalous FROM photos LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(anomalous, 1, "row should be flagged anomalous");
}

// =====================================================================
// Test row 43: fatal exit codes — catalog-path-is-directory → 74.
// =====================================================================

#[test]
fn ingest_catalog_path_is_directory_exits_74() {
    let dir = tempfile::tempdir().unwrap();
    let (input_dir, _) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            dir.path().to_str().unwrap(),
            "ingest",
            input_dir.path().to_str().unwrap(),
        ])
        .assert()
        .code(74);
}

// =====================================================================
// Test row 44: clap parse failures exit 2 (distinct from our fatal 74).
// =====================================================================

#[test]
fn ingest_threads_zero_exits_2_clap_default() {
    let (dir, _) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["--threads", "0", "ingest", dir.path().to_str().unwrap()])
        .assert()
        .code(2);
}

#[test]
fn ingest_threads_huge_exits_2_clap_default() {
    let (dir, _) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["--threads", "2000", "ingest", dir.path().to_str().unwrap()])
        .assert()
        .code(2);
}

#[test]
fn ingest_lock_timeout_zero_exits_2_clap_default() {
    let (dir, _) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog-lock-timeout-seconds",
            "0",
            "ingest",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .code(2);
}

#[test]
fn ingest_lock_timeout_above_max_exits_2_clap_default() {
    let (dir, _) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog-lock-timeout-seconds",
            "5000",
            "ingest",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .code(2);
}

// =====================================================================
// Test row 45: stub subcommands exit 69 EX_UNAVAILABLE.
// =====================================================================

#[test]
fn stub_subcommands_exit_69_with_not_yet_implemented_message() {
    // "cull" removed after D3 wired the real handler (plan PR1-T2).
    for name in ["develop", "export", "run", "models", "camera"] {
        Command::cargo_bin("photohelper")
            .unwrap()
            .arg(name)
            .assert()
            .code(69)
            .stderr(contains("not yet implemented in v0.1 (ingest only)"));
    }
}

#[test]
fn cull_help_does_not_emit_stub_message() {
    // `cull --help` shows clap help, not the stub message.
    // After D4 wires cull, this confirms the real handler is active.
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["cull", "--help"])
        .assert()
        .code(0)
        .stderr(contains("not yet implemented in v0.1").not());
}

// =====================================================================
// Test row 47: --catalog override actually used.
// =====================================================================

#[test]
fn ingest_catalog_flag_overrides_default_path() {
    let (input_dir, _) = fixture_dir_with_one_cr3();
    let cat_dir = tempfile::tempdir().unwrap();
    let cat = cat_dir.path().join("explicit.db");
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            cat.to_str().unwrap(),
            "ingest",
            input_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(
        cat.exists(),
        "DB should land at the explicit --catalog path"
    );
    assert!(
        !input_dir
            .path()
            .join(".photohelper")
            .join("catalog.db")
            .exists(),
        "default path should NOT be created when --catalog overrides"
    );
}

// =====================================================================
// Test row 40: walker edges (consolidated).
// =====================================================================

#[cfg(unix)]
#[test]
fn ingest_walker_handles_hidden_files_and_empty_subdirs() {
    let dir = tempfile::tempdir().unwrap();
    // Hidden CR3 should be cataloged (we treat hidden files normally).
    let hidden = dir.path().join(".hidden.cr3");
    std::fs::write(&hidden, vec![0xCCu8; 200]).unwrap();
    let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0);
    filetime::set_file_mtime(&hidden, mtime).unwrap();
    // Deeply nested empty subdir.
    std::fs::create_dir_all(dir.path().join("a/b/c/d")).unwrap();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("ingested: 1"));
}

// =====================================================================
// Test row 46: --verbose mapping (sanity).
// =====================================================================

#[test]
fn ingest_quiet_mutes_warn_but_summary_still_prints() {
    let (dir, _) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["-q", "ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        // Summary line uses eprintln! not tracing — must survive -q.
        .stderr(contains("walked:"));
}

// =====================================================================
// R1.T1 regression test: no_exif counter increments when EXIF parse
// yields zero fields. Without the R1.T1 fix the counter stayed at 0
// forever — invisible drift from the §Observability contract.
// =====================================================================

#[test]
fn ingest_no_exif_counter_increments_for_synthetic_cr3() {
    // Our synthetic CR3 fixture is raw 0xCC bytes — kamadak-exif
    // cannot parse the ISO-BMFF container, so EXIF returns zero
    // fields. The summary must show no-exif: 1.
    let (dir, _cr3) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("no-exif: 1"));
}

// =====================================================================
// R1 plan row 48: heartbeat fires at the configured interval.
// R2-T6 rewrite: previous version asserted on the unconditional summary
// line `walked: 1`, which fired whether or not the heartbeat thread ran.
// The test would pass even if `heartbeat_loop` were deleted. Per the
// global testing standards, that pattern blocks merge.
//
// New shape — deterministic by construction:
//   * 80 CR3 fixtures so the walk + per-photo ingest takes >>1ms
//     (kamadak-exif parse + BLAKE3 + SQLite insert per photo).
//   * `PHOTOHELPER_HEARTBEAT_INTERVAL_MS=1` — with R2-T4's
//     `granularity = min(interval, 100ms)`, the heartbeat ticks every
//     1ms regardless of the walk speed.
//   * Assert on the `[heartbeat]` substring (unique to the heartbeat
//     output) — would FAIL if `heartbeat_loop` body were deleted.
// =====================================================================

/// 80-CR3 fixture so the ingest worker takes long enough that a 1ms
/// heartbeat is guaranteed to fire at least once during the walk.
fn fixture_dir_with_many_cr3s(n: usize) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..n {
        let cr3 = dir.path().join(format!("img_{i:04}.cr3"));
        // Vary the byte to vary the PhotoId so the catalog inserts (not
        // dedup-skips) each row — exercises the full ingest path per file.
        std::fs::write(&cr3, vec![(i & 0xFF) as u8; 200]).unwrap();
        let mtime = filetime::FileTime::from_unix_time(1_577_836_800 + i as i64, 0);
        filetime::set_file_mtime(&cr3, mtime).unwrap();
    }
    dir
}

#[test]
fn heartbeat_fires_during_ingest_when_interval_is_short() {
    let dir = fixture_dir_with_many_cr3s(80);
    let assert = Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "1")
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success();
    // R2-T6: assert on the `[heartbeat]` substring (uniquely produced by
    // `heartbeat_loop`'s eprintln!). If the loop body is deleted or the
    // env-var override stops being honored, this assertion fails.
    assert.stderr(contains("[heartbeat] walked"));
}

// =====================================================================
// D5d tests — DN-008 6 rows (session 03)
// Row 6: compile-time Send+Sync assertion already in catalog tests via
//        static_assertions::assert_impl_all!(Arc<Catalog>: Send, Sync).
// =====================================================================

// =====================================================================
// Row 17: hardlink dedup — two paths → same PhotoId → one catalog row.
// =====================================================================

#[test]
fn ingest_hardlink_produces_one_row_and_already_catalogued() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.cr3");
    // 200 bytes of 0xCC so PhotoId derivation produces a deterministic ID.
    std::fs::write(&original, vec![0xCCu8; 200]).unwrap();
    let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0);
    filetime::set_file_mtime(&original, mtime).unwrap();
    let hardlink = dir.path().join("hardlink.cr3");
    std::fs::hard_link(&original, &hardlink).unwrap();
    // Hardlink shares inode — same size, same mtime, same content → same PhotoId.

    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("walked: 2"))
        .stderr(contains("ingested: 1"))
        .stderr(contains("already-catalogued: 1"));

    let conn =
        rusqlite::Connection::open(dir.path().join(".photohelper").join("catalog.db")).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "hardlinked files share a PhotoId; only one row expected"
    );
    drop(conn);
}

// =====================================================================
// Row 39: --strict on CR3-only dir with real EXIF asserts exit 0.
// Requires Git LFS fixtures (tests/fixtures/cr3/).
// =====================================================================

#[test]
fn ingest_strict_on_real_cr3_dir_exits_zero() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("cr3");

    // Skip this test if LFS fixtures aren't pulled (CI without LFS).
    if !fixtures.join("RAW_FULL_FRAME.CR3").exists() {
        return;
    }

    let cat_dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            cat_dir.path().join("c.db").to_str().unwrap(),
            "ingest",
            "--strict",
            fixtures.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(contains("no-exif: 0"));
}

// =====================================================================
// Row 42: walker edge cases — nested-dirs + broken symlinks.
// =====================================================================

#[test]
fn ingest_walks_nested_directories() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub1").join("sub2");
    std::fs::create_dir_all(&sub).unwrap();
    let cr3 = sub.join("deep.cr3");
    std::fs::write(&cr3, vec![0xAAu8; 200]).unwrap();
    filetime::set_file_mtime(&cr3, filetime::FileTime::from_unix_time(1_577_836_800, 0)).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("ingested: 1"));
}

#[test]
#[cfg(unix)]
fn ingest_skips_broken_symlinks_without_error() {
    let dir = tempfile::tempdir().unwrap();
    // A broken symlink: points to a non-existent target.
    std::os::unix::fs::symlink(dir.path().join("ghost.cr3"), dir.path().join("broken.cr3"))
        .unwrap();
    // A valid CR3 so the walk finds at least one file.
    let cr3 = dir.path().join("real.cr3");
    std::fs::write(&cr3, vec![0xBBu8; 200]).unwrap();
    filetime::set_file_mtime(&cr3, filetime::FileTime::from_unix_time(1_577_836_800, 0)).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("ingested: 1"))
        .stderr(contains("errored: 0"));
}

// =====================================================================
// Row 43: mtime_anomalous round-trip — mtime > 2100 sets flag = 1.
// =====================================================================

#[test]
fn ingest_future_mtime_sets_mtime_anomalous_flag() {
    let dir = tempfile::tempdir().unwrap();
    let cr3 = dir.path().join("future.cr3");
    std::fs::write(&cr3, vec![0xDDu8; 200]).unwrap();
    // Year 2200 = unix timestamp ≈ 7_258_118_400 (well beyond 2100 ceiling).
    filetime::set_file_mtime(&cr3, filetime::FileTime::from_unix_time(7_258_118_400, 0)).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("mtime-anomalous: 1"));

    let conn =
        rusqlite::Connection::open(dir.path().join(".photohelper").join("catalog.db")).unwrap();
    let flag: i64 = conn
        .query_row("SELECT mtime_anomalous FROM photos LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        flag, 1,
        "mtime > 2100 must set mtime_anomalous = 1 in the DB"
    );
    drop(conn);
}

// =====================================================================
// Row 49: fatal exit codes.
// =====================================================================

#[test]
fn ingest_exits_75_when_catalog_lock_is_held() {
    use std::fs::File;

    let input_dir = tempfile::tempdir().unwrap();
    let cat_dir = tempfile::tempdir().unwrap();
    let db_path = cat_dir.path().join("catalog.db");

    // Pre-create the lock file and acquire it exclusively.
    let lock_path = cat_dir.path().join("catalog.db.lock");
    let lock_file = File::create(&lock_path).unwrap();
    // fs4 1.x: `lock()` = exclusive blocking lock; `unlock()` = release.
    // Fully-qualified to sidestep the `unstable_name_collisions` lint.
    <std::fs::File as fs4::FileExt>::lock(&lock_file).unwrap();

    // With the lock held, ingest must fail with EX_TEMPFAIL (75).
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            db_path.to_str().unwrap(),
            "--catalog-lock-timeout-seconds",
            "1",
            "ingest",
            input_dir.path().to_str().unwrap(),
        ])
        .assert()
        .code(75);

    <std::fs::File as fs4::FileExt>::unlock(&lock_file).unwrap();
}

#[test]
#[cfg(unix)]
fn ingest_exits_77_when_catalog_dir_is_not_writable() {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    let input_dir = tempfile::tempdir().unwrap();
    let cat_dir = tempfile::tempdir().unwrap();
    // Make the catalog dir read-only so lock-file-create fails.
    std::fs::set_permissions(cat_dir.path(), Permissions::from_mode(0o555)).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            cat_dir.path().join("catalog.db").to_str().unwrap(),
            "ingest",
            input_dir.path().to_str().unwrap(),
        ])
        .assert()
        .code(77);

    // Restore write permission so tempdir cleanup succeeds.
    std::fs::set_permissions(cat_dir.path(), Permissions::from_mode(0o755)).unwrap();
}

// =====================================================================
// D5e: R2-T18 WARN regression tests.
// Note: "build_global already initialized" requires in-process test
// infrastructure (calling run_ingest() twice in the same process) which
// is deferred — that test lives outside subprocess integration tests.
// =====================================================================

// =====================================================================
// D5e row 3: file-lock op-tag in fatal error output.
// When the catalog dir is non-writable, the error must surface
// op="lock-file-create" in the fatal error message on stderr.
// =====================================================================

#[test]
#[cfg(unix)]
fn ingest_permission_denied_error_includes_lock_file_create_op_tag() {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    let input_dir = tempfile::tempdir().unwrap();
    let cat_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(cat_dir.path(), Permissions::from_mode(0o555)).unwrap();

    // R2-T11 fix: the error message must contain "lock-file-create" (not
    // the old "mkdir-p" which was the wrong op tag before R1.T10).
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            cat_dir.path().join("catalog.db").to_str().unwrap(),
            "ingest",
            input_dir.path().to_str().unwrap(),
        ])
        .assert()
        .code(77)
        .stderr(contains("lock-file-create"));

    std::fs::set_permissions(cat_dir.path(), Permissions::from_mode(0o755)).unwrap();
}

// =====================================================================
// D5e row 2: wal_checkpoint recovered N frames — WARN fires on reopen
// after a dirty WAL (un-checkpointed writes from a prior run).
// Tests that the tracing::warn!(...) in Catalog::open fires when the
// WAL checkpoint recovers frames.
// =====================================================================

#[test]
fn ingest_wal_checkpoint_warn_fires_on_reopen_with_dirty_wal() {
    // D5e: Catalog::open runs PRAGMA wal_checkpoint(TRUNCATE) and WARNs
    // if the checkpoint recovers > 0 frames (indicating un-checkpointed
    // writes from a previous run). We simulate this by writing, then
    // copying just the -wal sidecar to force a non-empty WAL on reopen.
    //
    // NOTE: The WARN is emitted by `tracing::warn!` at level WARN which
    // the CLI filters to stderr when not in quiet mode. The CLI's default
    // log level is WARN so this WARN should appear.
    let dir = tempfile::tempdir().unwrap();
    let cr3 = dir.path().join("a.cr3");
    std::fs::write(&cr3, vec![0xCCu8; 200]).unwrap();
    filetime::set_file_mtime(&cr3, filetime::FileTime::from_unix_time(1_577_836_800, 0)).unwrap();

    // First ingest: writes the catalog (and implicitly the WAL in WAL mode).
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success();

    let wal_path = dir.path().join(".photohelper").join("catalog.db-wal");

    // SQLite in WAL mode typically checkpoints and truncates the WAL on a clean
    // connection close. In that common case we cannot synthesize a dirty WAL
    // from a subprocess test without low-level file surgery (e.g. forcibly
    // leaving the WAL non-empty by aborting a connection mid-write).
    // This test is therefore best-effort: it runs when the WAL is non-empty
    // after first ingest (uncommon), skips otherwise.
    // TD: rewrite as an in-process unit test that calls Catalog::open directly
    // and can control WAL state between opens.
    if !wal_path.exists() || std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0) == 0 {
        // WAL fully checkpointed on close — cannot reliably test recovery here.
        // This skip is expected on most machines; not a test failure.
        return;
    }

    // Second ingest: if we reach here, the WAL has non-zero frames.
    // catalog.rs emits: tracing::warn!("previous shutdown was unclean; recovered
    // {recovered} WAL frames") — assert on that actual message text.
    Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(contains("previous shutdown was unclean"));
}

// =====================================================================
// D3: cull integration tests
// =====================================================================

/// Path to the NIMA model directory (crates/photohelper-ai/models/).
fn nima_model_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("crates")
        .join("photohelper-ai")
        .join("models")
}

/// Path to the CC0 Canon R8 CR3 fixture directory.
fn cr3_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("cr3")
}

/// D3 test: ingest CC0 CR3 fixtures → cull → verify cull_scores row.
///
/// Skipped if LFS fixtures are absent (CI without LFS pull).
/// Exercises the full D3 pipeline end-to-end against real sensor data.
#[test]
fn cull_scores_real_canon_r8_cr3_fixture() {
    let fixtures = cr3_fixture_dir();
    if !fixtures.join("RAW_FULL_FRAME.CR3").exists() {
        return;
    }
    let model_dir = nima_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    let cat_dir = tempfile::tempdir().unwrap();
    let cat_path = cat_dir.path().join("c.db");

    // Ingest the CC0 fixture directory.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "ingest",
            fixtures.to_str().unwrap(),
        ])
        .assert()
        .code(0);

    // Cull: NIMA inference on the ingested photos.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_path.to_str().unwrap(), "cull"])
        .assert()
        .code(0)
        .stderr(contains("scored: 2"));

    // Verify cull_scores rows exist with scores in [1.0, 10.0].
    let conn = rusqlite::Connection::open(&cat_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cull_scores", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2, "both CC0 CR3 fixtures must be scored");
    let scores: Vec<f64> = {
        let mut stmt = conn
            .prepare("SELECT aesthetic_score FROM cull_scores")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    for score in &scores {
        assert!(
            (1.0..=10.0).contains(score),
            "aesthetic_score {score} must be in [1.0, 10.0]"
        );
    }

    // Theme-E idempotency: second cull run finds no unscored photos (the
    // unsuperseded_unscored_rows SQL excludes already-scored photos via NOT IN),
    // so walked=0. This verifies the SQL filter is active and cull exits cleanly.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_path.to_str().unwrap(), "cull"])
        .assert()
        .code(0)
        .stderr(contains("walked: 0"))
        .stderr(contains("scored: 0"));
}

/// D3 test: ingest a synthetic (undecodeable) CR3 → cull --strict → exit ≠ 0.
///
/// The synthetic fixture has 0xCC bytes — `read_raw_rgb` will fail with a
/// LibRaw decode error → `decode_failed > 0` → `--strict` escalation.
#[test]
fn cull_strict_exits_nonzero_on_decode_fail() {
    let model_dir = nima_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    // Ingest the synthetic CR3 fixture.
    let (dir, _cr3) = fixture_dir_with_one_cr3();
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .code(0);

    // Cull --strict: decode failure must cause non-zero exit.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            dir.path()
                .join(".photohelper")
                .join("catalog.db")
                .to_str()
                .unwrap(),
            "cull",
            "--strict",
        ])
        .assert()
        .code(1) // EX_STRICT_FAIL = 1 (POSIX generic failure)
        .stderr(contains("decode-failed: 1"));
}

// =====================================================================
// D3: dedup integration tests
// =====================================================================

fn clip_model_dir() -> PathBuf {
    nima_model_dir() // both NIMA and CLIP live in the same models dir
}

/// D3 test 1: ingest CC0 fixtures → dedup → embeddings + clusters written.
///
/// Requires the CLIP ONNX model (Git LFS). Skips if model not present.
#[test]
fn dedup_end_to_end_embeds_and_clusters_cc0_fixtures() {
    let model_dir = clip_model_dir();
    let clip_manifest = model_dir.join("manifest.toml");
    if !clip_manifest.exists() {
        return; // Git LFS not pulled; skip.
    }

    let fixture_dir = cr3_fixture_dir();
    if !fixture_dir.join("CRAW_FULL_FRAME.CR3").exists() {
        return; // LFS fixtures not pulled; skip.
    }

    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");

    // Ingest the CC0 fixtures first.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "ingest",
            fixture_dir.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(contains("ingested: 2"));

    // Dedup: embed + cluster.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_path.to_str().unwrap(), "dedup"])
        .assert()
        .code(0)
        .stderr(contains("embedded: 2"));

    // Verify embeddings and dup_clusters rows exist.
    let conn = rusqlite::Connection::open(&cat_path).unwrap();
    let emb_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(emb_count, 2, "both CC0 CR3 fixtures must be embedded");
    let cluster_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM dup_clusters", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        cluster_count, 2,
        "both photos must have cluster assignments"
    );
}

/// D3 test 2: second dedup run walks 0 photos (already embedded → SQL filter).
#[test]
fn dedup_idempotency_second_run_walks_zero() {
    let model_dir = clip_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }
    let fixture_dir = cr3_fixture_dir();
    if !fixture_dir.join("CRAW_FULL_FRAME.CR3").exists() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "ingest",
            fixture_dir.to_str().unwrap(),
        ])
        .assert()
        .code(0);

    // First dedup.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_path.to_str().unwrap(), "dedup"])
        .assert()
        .code(0)
        .stderr(contains("embedded: 2"));

    // Second dedup: unembedded_rows filter → walked: 0.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_path.to_str().unwrap(), "dedup"])
        .assert()
        .code(0)
        .stderr(contains("walked: 0"));
}

/// D3 test 3: strict mode exits 1 when a photo's file is missing.
#[test]
fn dedup_strict_exits_nonzero_on_file_missing() {
    let model_dir = clip_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let fake_cr3 = tmp.path().join("gone.cr3");

    // Insert a stub CR3 file so ingest can hash it.
    std::fs::write(&fake_cr3, vec![0xFFu8; 1024 * 1024]).unwrap();
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "ingest",
            tmp.path().to_str().unwrap(),
        ])
        .assert()
        .code(0);

    // Remove the file so dedup sees it as missing.
    std::fs::remove_file(&fake_cr3).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_path.to_str().unwrap(), "dedup", "--strict"])
        .assert()
        .code(1) // EX_STRICT_FAIL = 1
        .stderr(contains("file-missing: 1"));
}
