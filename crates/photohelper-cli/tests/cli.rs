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
    // "cull" removed after session 04 wired the real handler.
    // "run" removed after session 10 wired the real handler.
    for name in ["models", "camera"] {
        Command::cargo_bin("photohelper")
            .unwrap()
            .arg(name)
            .assert()
            .code(69)
            .stderr(contains("not yet implemented in v0.1"));
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
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "dedup",
            "--similarity-threshold",
            "0.95",
        ])
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
        cluster_count, 0,
        "singletons must be filtered out, leaving 0 cluster assignments"
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

// =====================================================================
// Develop subcommand integration tests (D4b, session 06).
// =====================================================================

/// Helper: ingest a synthetic (fake) CR3 so develop can find it in the catalog.
/// The fake file is 1MB of 0xFF bytes — enough to get a PhotoId hash without
/// needing LibRaw (develop does not decode RAW pixels).
fn ingest_fake_cr3(catalog_path: &str, dir: &std::path::Path) -> std::path::PathBuf {
    let fake_cr3 = dir.join("photo.CR3");
    std::fs::write(&fake_cr3, vec![0xFFu8; 1024 * 1024]).unwrap();
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", catalog_path, "ingest", dir.to_str().unwrap()])
        .assert()
        .code(0);
    fake_cr3
}

#[test]
fn develop_creates_xmp_sidecar_for_ingested_photo() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();

    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let expected_xmp = cr3.with_extension("xmp"); // photo.xmp NOT photo.CR3.xmp

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0)
        .stderr(contains("written: 1"));

    assert!(
        expected_xmp.exists(),
        "photo.xmp must exist (not photo.CR3.xmp)"
    );
    assert!(
        !tmp.path().join("photo.CR3.xmp").exists(),
        "photo.CR3.xmp must NOT exist"
    );
}

#[test]
fn develop_empty_catalog_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    // Create an empty subdirectory for ingest (no RAW files → catalog is initialized but empty).
    let empty_dir = tmp.path().join("empty");
    std::fs::create_dir(&empty_dir).unwrap();
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "ingest",
            empty_dir.to_str().unwrap(),
        ])
        .assert()
        .code(0); // walked 0 RAW files → exit 0 (empty dir is not an EX_USAGE error)

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_path.to_str().unwrap(), "develop"])
        .assert()
        .code(0)
        .stderr(contains("walked: 0"));
}

#[test]
fn develop_idempotency_second_run_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    ingest_fake_cr3(cat_str, tmp.path());

    // First run: written=1
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0)
        .stderr(contains("written: 1"));

    // Second run without --force: the existing sidecar was written by us (no MetadataDate
    // from a third party), so merge_and_write will Overwrite or ConflictPreserve depending
    // on timestamps. Either way, written should be 0 on the second run.
    let out = Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&out);
    assert!(
        !stderr.contains("written: 1"),
        "second run must not report written: 1; got: {stderr}"
    );
}

#[test]
fn develop_strict_exits_nonzero_on_file_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());

    // Delete the source file after ingest.
    std::fs::remove_file(&cr3).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--strict"])
        .assert()
        .code(1) // EX_STRICT_FAIL = 1
        .stderr(contains("file-missing: 1"));
}

#[test]
fn develop_cli_flags_written_to_sidecar() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_str,
            "develop",
            "--temp",
            "5500",
            "--exposure",
            "1.50",
            "--auto-tone",
        ])
        .assert()
        .code(0);

    let xml = std::fs::read_to_string(&xmp).expect("sidecar must exist");
    assert!(
        xml.contains("crs:Temperature=\"5500\""),
        "Temperature must be in sidecar"
    );
    assert!(
        xml.contains("crs:Exposure2012=\"1.5\""),
        "Exposure must be in sidecar"
    );
    assert!(
        xml.contains("crs:AutoTone=\"True\""),
        "AutoTone must be in sidecar"
    );
}

#[test]
fn develop_strict_exits_nonzero_on_xml_parse_error() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    // Write malformed XML to the sidecar.
    std::fs::write(&xmp, "<x:xmpmeta><unclosed tag></x:xmpmeta>").unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--strict"])
        .assert()
        .code(1) // EX_STRICT_FAIL = 1
        .stderr(contains("errored: 1"));
}

#[test]
fn develop_summary_line_contains_expected_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    ingest_fake_cr3(cat_str, tmp.path());

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0)
        .stderr(contains("walked:"))
        .stderr(contains("written:"))
        .stderr(contains("updated:"))
        .stderr(contains("conflict-preserved:"))
        .stderr(contains("force-overwritten:"))
        .stderr(contains("file-missing:"))
        .stderr(contains("errored:"));
}

#[test]
fn develop_force_overwrites_conflict() {
    // Test that --force CLI flag bypasses conflict resolution and overwrites.
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    // First develop run: creates sidecar.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--temp", "5500"])
        .assert()
        .code(0)
        .stderr(contains("written: 1"));

    // Replace the existing xmp:MetadataDate with a future value to simulate Lightroom
    // editing after our last develop pass (avoids duplicate-attribute parse errors).
    let raw = std::fs::read_to_string(&xmp).unwrap();
    let future_xml = if let Some(start) = raw.find("xmp:MetadataDate=\"") {
        let after_open = start + "xmp:MetadataDate=\"".len();
        let close = raw[after_open..].find('"').expect("closing quote");
        format!(
            "{}xmp:MetadataDate=\"2099-01-01T00:00:00Z\"{}",
            &raw[..start],
            &raw[after_open + close + 1..]
        )
    } else {
        raw.replace(
            "ph:LastProcessedAt=\"",
            "xmp:MetadataDate=\"2099-01-01T00:00:00Z\"\n      ph:LastProcessedAt=\"",
        )
    };
    std::fs::write(&xmp, future_xml).unwrap();

    // Without --force, should conflict-preserve.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0);

    // With --force, must overwrite unconditionally.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--force"])
        .assert()
        .code(0)
        .stderr(contains("force-overwritten: 1"));
}

#[test]
fn develop_conflict_preserved_appears_in_summary() {
    // Test that conflict-preserved counter appears in summary when Lightroom
    // has edited a sidecar after our last write.
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    // First run: create sidecar.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0)
        .stderr(contains("written: 1"));

    // Simulate Lightroom writing a future xmp:MetadataDate (external edit after us).
    // REPLACE (not prepend) the existing xmp:MetadataDate to avoid duplicate-attribute
    // XML parse errors. A future xmp:MetadataDate > ph:LastProcessedAt causes ConflictPreserved.
    let raw = std::fs::read_to_string(&xmp).unwrap();
    let with_future_date = {
        // Replace the "xmp:MetadataDate="..." value with a far-future date.
        if let Some(start) = raw.find("xmp:MetadataDate=\"") {
            let after_open = start + "xmp:MetadataDate=\"".len();
            let close = raw[after_open..].find('"').expect("closing quote");
            format!(
                "{}xmp:MetadataDate=\"2099-01-01T00:00:00Z\"{}",
                &raw[..start],
                &raw[after_open + close + 1..]
            )
        } else {
            // No existing xmp:MetadataDate — inject before ph:LastProcessedAt.
            raw.replace(
                "ph:LastProcessedAt=\"",
                "xmp:MetadataDate=\"2099-01-01T00:00:00Z\"\n      ph:LastProcessedAt=\"",
            )
        }
    };
    std::fs::write(&xmp, with_future_date).unwrap();

    // Second run: should detect Lightroom's future date and preserve.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0)
        .stderr(contains("conflict-preserved: 1"));
}

#[test]
fn develop_lightroom_compatibility_flags_written_to_sidecar() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    // Manually insert mock cull score, embedding and cluster assignment
    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let photo_id: Vec<u8> = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
            .unwrap();

        // 1. Insert cull score (e.g. 7.8 -> Rating::Four, Label::"Green", flat nima:good, hierarchical nima:good)
        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![photo_id, "nima-aesthetic-v1", 7.8, 1_700_000_000],
        )
        .unwrap();

        // 2. Insert embedding (required for dup_clusters foreign key)
        let dummy_embedding = vec![0.0f32; 512];
        let embedding_bytes: Vec<u8> = dummy_embedding
            .iter()
            .flat_map(|&f| f.to_ne_bytes())
            .collect();
        conn.execute(
            "INSERT INTO embeddings (photo_id, model_slug, dim, quantization, embedding, embedded_at_unix_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![photo_id, "clip-vit-b32-laion2b-v1", 512, "f32", embedding_bytes, 1_700_000_000],
        )
        .unwrap();

        // 3. Insert cluster assignment
        conn.execute(
            "INSERT INTO dup_clusters (photo_id, model_slug, cluster_id, similarity_threshold, clustered_at_unix_seconds) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![photo_id, "clip-vit-b32-laion2b-v1", 15, 0.85, 1_700_000_000],
        )
        .unwrap();
    }

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_str,
            "develop",
            "--lr-rating",
            "--lr-label",
            "--lr-keywords",
        ])
        .assert()
        .code(0);

    let xml = std::fs::read_to_string(&xmp).expect("sidecar must exist");

    // Star rating
    // A score of 7.8 is < 8.5 but >= 7.0, which maps to Rating::Four (4 stars)
    assert!(
        xml.contains("xmp:Rating=\"4\""),
        "Rating 4 must be written when score is 7.8"
    );

    // Label
    // A score of 7.8 is >= 7.0, which maps to "Green"
    assert!(
        xml.contains("xmp:Label=\"Green\""),
        "Label Green must be written when score is 7.8"
    );

    // Flat keywords (dc:subject Bag structure)
    assert!(
        xml.contains("<rdf:li>photohelper</rdf:li>"),
        "photohelper keyword must be present"
    );
    assert!(
        xml.contains("<rdf:li>nima:good</rdf:li>"),
        "nima:good keyword must be present"
    );
    assert!(
        xml.contains("<rdf:li>cluster:15</rdf:li>"),
        "cluster:15 keyword must be present"
    );

    // Hierarchical keywords (lr:hierarchicalSubject Bag structure)
    assert!(
        xml.contains("<rdf:li>photohelper|nima:good</rdf:li>"),
        "hierarchical nima:good must be present"
    );
    assert!(
        xml.contains("<rdf:li>photohelper|cluster:15</rdf:li>"),
        "hierarchical cluster:15 must be present"
    );
}

#[test]
fn develop_clean_isolation_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop"])
        .assert()
        .code(0)
        .stderr(contains(
            "WARNING: photohelper develop is running without any Lightroom NIMA mapping flags activated.",
        ));

    let xml = std::fs::read_to_string(&xmp).expect("sidecar must exist");
    assert!(
        !xml.contains("xmp:Rating="),
        "Rating should be absent by default"
    );
    assert!(
        !xml.contains("xmp:Label="),
        "Label should be absent by default"
    );
    assert!(
        !xml.contains("dc:subject"),
        "dc:subject keywords should be absent by default"
    );
}

#[test]
fn develop_individual_lr_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let photo_id: Vec<u8> = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![photo_id, "nima-aesthetic-v1", 9.2, 1_700_000_000],
        )
        .unwrap();
    }

    // Develop with only --lr-rating flag
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--lr-rating"])
        .assert()
        .code(0);

    let xml = std::fs::read_to_string(&xmp).expect("sidecar must exist");
    assert!(xml.contains("xmp:Rating=\"5\""), "Rating should be 5");
    assert!(!xml.contains("xmp:Label="), "Label should not be present");
    assert!(
        !xml.contains("dc:subject"),
        "dc:subject keywords should not be present"
    );
}

#[test]
fn develop_handles_nan_and_infinite_scores() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    // Test non-finite (Infinity) score via SQL literal 9e999
    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let photo_id: Vec<u8> = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
            .unwrap();

        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, ?2, 9e999, ?3)",
            rusqlite::params![photo_id, "nima-aesthetic-v1", 1_700_000_000],
        )
        .unwrap();
    }

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--lr-rating", "--lr-label"])
        .assert()
        .code(0);

    // Since infinity is non-finite, we now treat it as an error and skip writing the sidecar.
    assert!(
        !xmp.exists(),
        "Sidecar should not be written for non-finite score"
    );
}

#[test]
fn develop_handles_out_of_bounds_scores() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let photo_id: Vec<u8> = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
            .unwrap();

        // 99.9 is out of bounds, so the builder now correctly rejects it.
        // It does not silently clamp.
        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![photo_id, "nima-aesthetic-v1", 99.9, 1_700_000_000],
        )
        .unwrap();
    }

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--lr-rating", "--lr-label"])
        .assert()
        .code(0)
        .stderr(predicates::str::contains("invalid settings; skipping"))
        .stderr(predicates::str::contains("errored: 1"));

    assert!(
        !xmp.exists(),
        "Sidecar should not be written for out of bounds score"
    );
}

#[test]
fn develop_missing_scores_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let _cr3 = ingest_fake_cr3(cat_str, tmp.path());

    // Develop a photo that has no cull scores or clusters inserted
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--lr-rating", "--lr-label", "--lr-keywords"])
        .assert()
        .code(0)
        .stderr(predicates::str::contains("WARNING: Lightroom rating/label flags were requested, but no culled scores exist in the catalog."))
        .stderr(predicates::str::contains("WARNING: Lightroom keywords flag was requested, but neither culled scores nor duplicate clusters exist in the catalog."));
}

#[test]
fn test_nima_score_mapping_boundaries() {
    let scores = [3.0, 5.0, 6.5, 8.0, 9.0];
    let expected_ratings = ["1", "2", "3", "4", "5"];
    let expected_labels = ["Red", "", "", "Green", "Green"];
    let expected_tiers = ["discard", "poor", "fair", "good", "excellent"];

    for (i, &score) in scores.iter().enumerate() {
        let run_tmp = tempfile::tempdir().unwrap();
        let run_cat_path = run_tmp.path().join("catalog.db");
        let run_cat_str = run_cat_path.to_str().unwrap();
        let cr3 = ingest_fake_cr3(run_cat_str, run_tmp.path());
        let xmp = cr3.with_extension("xmp");

        {
            let conn = rusqlite::Connection::open(&run_cat_path).unwrap();
            let photo_id: Vec<u8> = conn
                .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
                .unwrap();

            conn.execute(
                "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![photo_id, "nima-aesthetic-v1", score, 1_700_000_000],
            )
            .unwrap();
        }

        Command::cargo_bin("photohelper")
            .unwrap()
            .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
            .args([
                "--catalog",
                run_cat_str,
                "develop",
                "--lr-rating",
                "--lr-label",
                "--lr-keywords",
            ])
            .assert()
            .code(0);

        let xml = std::fs::read_to_string(&xmp).expect("sidecar must exist");

        let expected_rating = expected_ratings.get(i).copied().unwrap();
        let expected_label = expected_labels.get(i).copied().unwrap();
        let expected_tier = expected_tiers.get(i).copied().unwrap();

        let expected_rating_attr = format!("xmp:Rating=\"{expected_rating}\"");
        assert!(
            xml.contains(&expected_rating_attr),
            "Rating incorrect for score {score}"
        );

        if expected_label.is_empty() {
            assert!(!xml.contains("xmp:Label=\"Red\"") && !xml.contains("xmp:Label=\"Green\""));
        } else {
            let expected_label_attr = format!("xmp:Label=\"{expected_label}\"");
            assert!(
                xml.contains(&expected_label_attr),
                "Label incorrect for score {score}"
            );
        }

        let expected_keyword = format!("nima:{expected_tier}");
        let expected_hierarchical = format!("photohelper|nima:{expected_tier}");
        assert!(
            xml.contains(&expected_keyword),
            "Flat keyword incorrect for score {score}"
        );
        assert!(
            xml.contains(&expected_hierarchical),
            "Hierarchical keyword incorrect for score {score}"
        );
    }
}

// =====================================================================
// Export subcommand integration tests (Session 08).
// =====================================================================

#[test]
fn export_empty_catalog_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let out_dir = tmp.path().join("export_out");

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "export",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(predicates::str::contains(
            "walked: 0, written: 0, skipped-existing: 0, skipped-rating: 0, file-missing: 0, errored: 0"
        ));
}

#[test]
fn export_runs_successfully_for_ingested_photos() {
    let fixture_dir = cr3_fixture_dir();
    if !fixture_dir.join("CRAW_FULL_FRAME.CR3").exists() {
        return; // LFS fixtures not pulled; skip.
    }

    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let out_dir = tmp.path().join("export_out");

    // Copy a real CR3 to a path inside tmp so we can ingest and export it
    let test_cr3_src = fixture_dir.join("CRAW_FULL_FRAME.CR3");
    let test_cr3_dest = tmp.path().join("CRAW_FULL_FRAME.CR3");
    std::fs::copy(&test_cr3_src, &test_cr3_dest).unwrap();

    // Ingest the photo
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
        .code(0)
        .stderr(contains("ingested: 1"));

    // Run export with --min-rating 0 (which includes unrated photos), no resize, no watermark
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "export",
            "--output",
            out_dir.to_str().unwrap(),
            "--min-rating",
            "0",
        ])
        .assert()
        .code(0)
        .stderr(predicates::str::contains("written: 1"));

    // Verify output file exists
    let expected_jpg = out_dir.join("CRAW_FULL_FRAME.jpg");
    assert!(expected_jpg.exists(), "Output JPEG must exist");
    assert!(
        std::fs::metadata(&expected_jpg).unwrap().len() > 0,
        "Output JPEG must not be empty"
    );
}

#[test]
fn export_applies_resize_and_watermark() {
    let fixture_dir = cr3_fixture_dir();
    if !fixture_dir.join("CRAW_FULL_FRAME.CR3").exists() {
        return; // LFS fixtures not pulled; skip.
    }

    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let out_dir = tmp.path().join("export_out");

    let test_cr3_src = fixture_dir.join("CRAW_FULL_FRAME.CR3");
    let test_cr3_dest = tmp.path().join("CRAW_FULL_FRAME.CR3");
    std::fs::copy(&test_cr3_src, &test_cr3_dest).unwrap();

    // Ingest
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

    // Export with --min-rating 0, long-edge resize, watermark
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "export",
            "--output",
            out_dir.to_str().unwrap(),
            "--min-rating",
            "0",
            "--long-edge",
            "800",
            "--watermark",
            "Antigravity",
            "--watermark-position",
            "bottom-left",
        ])
        .assert()
        .code(0)
        .stderr(predicates::str::contains("written: 1"));

    let expected_jpg = out_dir.join("CRAW_FULL_FRAME.jpg");
    assert!(expected_jpg.exists(), "Output JPEG must exist");
    assert!(
        std::fs::metadata(&expected_jpg).unwrap().len() > 0,
        "Output JPEG must not be empty"
    );
}

#[test]
fn export_strict_cancellation_on_missing_file() {
    let fixture_dir = cr3_fixture_dir();
    if !fixture_dir.join("CRAW_FULL_FRAME.CR3").exists() {
        return; // LFS fixtures not pulled; skip.
    }

    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let out_dir = tmp.path().join("export_out");

    let test_cr3_src = fixture_dir.join("CRAW_FULL_FRAME.CR3");
    let test_cr3_dest = tmp.path().join("CRAW_FULL_FRAME.CR3");
    std::fs::copy(&test_cr3_src, &test_cr3_dest).unwrap();

    // Ingest
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

    // Delete the file on disk to simulate missing file
    std::fs::remove_file(&test_cr3_dest).unwrap();

    // Export with strict, should fail
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "export",
            "--output",
            out_dir.to_str().unwrap(),
            "--min-rating",
            "0",
            "--strict",
        ])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("file-missing: 1"));
}

// =====================================================================
// Lightroom Sync Improvement tests (Session 09).
// =====================================================================

#[test]
fn develop_all_lr_and_lr_label_score() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let _cr3 = ingest_fake_cr3(cat_str, tmp.path());

    // Both flags can coexist without a Clap conflict.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_str,
            "develop",
            "--all-lr",
            "--lr-label-score",
        ])
        .assert()
        .success();
}

#[test]
fn develop_shorthand_all_lr() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let photo_id: Vec<u8> = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
            .unwrap();

        // 8.0 is < 8.5, so it maps to Rating::Four (4) and "good" keyword
        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![photo_id, "nima-aesthetic-v1", 8.0, 1_700_000_000],
        )
        .unwrap();
    }

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--all-lr"])
        .assert()
        .code(0);

    let xml = std::fs::read_to_string(&xmp).expect("sidecar must exist");
    assert!(
        xml.contains("xmp:Rating=\"4\""),
        "Rating should be written via --all-lr"
    );
    assert!(
        xml.contains("xmp:Label=\"Green\""),
        "Label should be written via --all-lr"
    );
    assert!(
        xml.contains("<rdf:li>nima:good</rdf:li>"),
        "Keywords should be written via --all-lr"
    );
}

#[test]
fn develop_custom_labels() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let photo_id: Vec<u8> = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
            .unwrap();

        // Low rating to ensure red label
        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![photo_id, "nima-aesthetic-v1", 3.0, 1_700_000_000],
        )
        .unwrap();
    }

    // Set custom red label via env var, and green label via CLI argument
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .env("PHOTOHELPER_LR_LABEL_RED", "Vermelho")
        .args([
            "--catalog",
            cat_str,
            "develop",
            "--lr-label",
            "--lr-label-green",
            "Verde",
        ])
        .assert()
        .code(0);

    let xml = std::fs::read_to_string(&xmp).expect("sidecar must exist");
    assert!(
        xml.contains("xmp:Label=\"Vermelho\""),
        "Custom red label should be written"
    );
}

#[test]
fn develop_custom_labels_upfront_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();

    // 1. Colliding labels (exits 74 EX_IOERR for fatal configuration bails)
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            cat_str,
            "develop",
            "--lr-label",
            "--lr-label-red",
            "Same",
            "--lr-label-green",
            "Same",
        ])
        .assert()
        .code(74)
        .stderr(predicates::str::contains(
            "invalid custom color label: 'Red' and 'Green' labels must be distinct",
        ));

    // 2. Empty/whitespace red label
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            cat_str,
            "develop",
            "--lr-label",
            "--lr-label-red",
            "   ",
        ])
        .assert()
        .code(74)
        .stderr(predicates::str::contains(
            "invalid custom color label: 'Red' label cannot be empty",
        ));

    // 3. Illegal XML character validation
    Command::cargo_bin("photohelper")
        .unwrap()
        .args([
            "--catalog",
            cat_str,
            "develop",
            "--lr-label",
            "--lr-label-red",
            "Red\x01Label",
        ])
        .assert()
        .code(74)
        .stderr(predicates::str::contains(
            "invalid custom color label: 'Red' label contains illegal XML characters",
        ));
}

#[test]
fn develop_mtime_conflict_shield() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());
    let xmp = cr3.with_extension("xmp");

    // Write initial sidecar to set up ph:LastProcessedAt
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "develop", "--all-lr"])
        .assert()
        .code(0);

    // Verify sidecar exists
    assert!(xmp.exists());

    // Wait a brief moment, then simulate an external tool edit by touching the modification time of the sidecar file forward.
    let mtime = std::fs::metadata(&xmp).unwrap().modified().unwrap();
    let forward_mtime = mtime + std::time::Duration::from_secs(5);
    filetime::set_file_mtime(&xmp, filetime::FileTime::from_system_time(forward_mtime)).unwrap();

    // Now run develop again with verbose flag -v. It should preserve the sidecar as conflict_preserved because file's mtime is newer.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["-v", "--catalog", cat_str, "develop", "--all-lr"])
        .assert()
        .code(0)
        .stderr(predicates::str::contains(
            "Preserved newer Lightroom Classic edits; skipped",
        ))
        .stderr(predicates::str::contains(
            "1 files were skipped to protect Lightroom Classic manual edits",
        ))
        .stderr(predicates::str::contains(
            "If you want to unconditionally force overwrite, re-run with --force.",
        ));
}

#[test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn develop_case_insensitive_path_deduplication() {
    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();

    // Ingest first photo row (e.g. photo.cr3)
    let cr3_lower = tmp.path().join("photo.cr3");
    std::fs::write(&cr3_lower, vec![0xCCu8; 200]).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["--catalog", cat_str, "ingest", tmp.path().to_str().unwrap()])
        .assert()
        .code(0);

    // Let's manually inject a duplicate photo row into the db with different casing targeting the same sidecar path.
    // e.g. "PHOTO.cr3".
    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let (mut id, file_size, mtime, anomalous, ingested_at): (Vec<u8>, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT id, file_size, mtime_unix_seconds, mtime_anomalous, ingested_at_unix_seconds FROM photos LIMIT 1",
                [],
                |r| Ok((r.get(0).unwrap(), r.get(1).unwrap(), r.get(2).unwrap(), r.get(3).unwrap(), r.get(4).unwrap())),
            )
            .unwrap();

        // Ensure we generate a unique PRIMARY KEY by flipping the first byte.
        if let Some(first) = id.get_mut(0) {
            *first ^= 1;
        }

        // Inject path with duplicate/upper casing pointing to the same file location on case-insensitive filesystems
        let cr3_upper = std::fs::canonicalize(tmp.path()).unwrap().join("PHOTO.cr3");
        let cr3_upper_str = cr3_upper.to_string_lossy().to_string();

        conn.execute(
            "INSERT INTO photos (
                id, source_path, file_size, mtime_unix_seconds,
                mtime_anomalous, make, model, camera_slug,
                capture_time_unix_seconds, width, height,
                exif_orientation, ingested_at_unix_seconds,
                superseded_at_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, NULL, NULL, NULL, NULL, ?6, NULL)",
            rusqlite::params![id, cr3_upper_str, file_size, mtime, anomalous, ingested_at],
        )
        .unwrap();
    }

    // Now run develop subcommand with verbose logging. It should trigger the warning about skipping duplicate photo row to prevent race conditions.
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(["-v", "--catalog", cat_str, "develop", "--all-lr"])
        .assert()
        .code(0)
        .stderr(predicates::str::contains(
            "skipping duplicate photo row targeting the same sidecar path to prevent concurrent write race hazards",
        ));
}

// =====================================================================
// Session 10: `photohelper run` integration tests
// =====================================================================

#[test]
fn run_happy_path_pipeline_and_option_propagation() {
    let fixtures = cr3_fixture_dir();
    if !fixtures.join("RAW_FULL_FRAME.CR3").exists() {
        return;
    }
    let model_dir = nima_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    let input_dir = workspace.path().join("input");
    let output_dir = workspace.path().join("output");
    std::fs::create_dir(&input_dir).unwrap();

    // Copy one fixture
    std::fs::copy(
        fixtures.join("RAW_FULL_FRAME.CR3"),
        input_dir.join("RAW_FULL_FRAME.CR3"),
    )
    .unwrap();

    let cat_path = workspace.path().join(".photohelper").join("catalog.db");

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "run",
            input_dir.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--all-lr",
            "--watermark",
            "IntegrationTest",
            "--quality",
            "90",
            "--long-edge",
            "800",
            "--lr-label-red",
            "Vermelho",
            "--min-rating",
            "0",
        ])
        .assert()
        .success()
        .stderr(contains("walked: 1")) // ingest
        .stderr(contains("scored: 1")) // cull
        .stderr(contains("written: 1")) // develop
        .stderr(contains("written: 1")); // export

    // Verify sidecar exists
    let sidecar_path = input_dir.join("RAW_FULL_FRAME.xmp");
    assert!(sidecar_path.exists(), "Sidecar must be generated");
    let sidecar_content = std::fs::read_to_string(&sidecar_path).unwrap();
    assert!(
        sidecar_content.contains("xmp:Rating="),
        "Sidecar must have rating"
    );
    assert!(
        sidecar_content.contains("lr:hierarchicalSubject>"),
        "Sidecar must have keywords"
    );

    // Verify JPEG exists. Because it has a NIMA score, the filename will be like `RAW_FULL_FRAME_cull5.30.jpg`.
    // Since the exact score varies slightly by architecture, we just look for any exported JPEG.
    let exported_jpegs: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    assert!(
        !exported_jpegs.is_empty(),
        "JPEG must be exported, but found: {:?}",
        std::fs::read_dir(&output_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>()
    );
}

#[test]
fn run_strict_mode_aborts_mid_pipeline() {
    let fixtures = cr3_fixture_dir();
    if !fixtures.join("RAW_FULL_FRAME.CR3").exists() {
        return;
    }
    let model_dir = nima_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    let input_dir = workspace.path().join("input");
    let output_dir = workspace.path().join("output");
    std::fs::create_dir(&input_dir).unwrap();

    // 1. Valid fixture that ingests successfully
    std::fs::copy(
        fixtures.join("RAW_FULL_FRAME.CR3"),
        input_dir.join("RAW_FULL_FRAME.CR3"),
    )
    .unwrap();

    // 2. Corrupt fixture that fails ingest, triggering strict abort at end of Stage 1.
    std::fs::write(input_dir.join("corrupt.cr3"), vec![0xCC; 100]).unwrap();

    let cat_path = workspace.path().join(".photohelper").join("catalog.db");

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "run",
            input_dir.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--strict",
        ])
        .assert()
        .failure()
        .code(1);

    // Verify torn state prevention: the valid CR3 was ingested but NOT developed/exported.
    assert!(cat_path.exists(), "Catalog should exist from stage 1");
    let jpeg_path = output_dir.join("RAW_FULL_FRAME.jpg");
    assert!(
        !jpeg_path.exists(),
        "JPEG must not be exported due to mid-pipeline abort"
    );
}

#[test]
fn run_pipeline_without_explicit_catalog() {
    let fixtures = cr3_fixture_dir();
    if !fixtures.join("RAW_FULL_FRAME.CR3").exists() {
        return;
    }
    let model_dir = nima_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    let input_dir = workspace.path().join("input");
    let output_dir = workspace.path().join("output");
    std::fs::create_dir(&input_dir).unwrap();

    std::fs::copy(
        fixtures.join("RAW_FULL_FRAME.CR3"),
        input_dir.join("RAW_FULL_FRAME.CR3"),
    )
    .unwrap();

    // No --catalog flag passed!
    Command::cargo_bin("photohelper")
        .unwrap()
        .current_dir(workspace.path()) // Run from workspace root, not input dir
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "run",
            input_dir.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--min-rating",
            "0",
        ])
        .assert()
        .success()
        .stderr(contains("written: 1")); // ensure develop/export actually ran!

    // Verify default catalog was created in the input dir
    let default_cat_path = input_dir.join(".photohelper").join("catalog.db");
    assert!(
        default_cat_path.exists(),
        "Catalog should be created in input dir by default"
    );
    let exported_jpegs: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    assert!(
        !exported_jpegs.is_empty(),
        "JPEG must be exported, but found: {:?}",
        std::fs::read_dir(&output_dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>()
    );
}

#[test]
fn run_input_output_collision_boundary() {
    let workspace = tempfile::tempdir().unwrap();
    let input_dir = workspace.path().join("input");
    std::fs::create_dir(&input_dir).unwrap();

    // Output is inside input
    let output_dir = input_dir.join("output");

    let model_dir = nima_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .args([
            "run",
            input_dir.to_str().unwrap(),
            "--output", output_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains("Output path cannot be a subdirectory of the input path to prevent recursive ingest loops"));

    // Output is exact same as input
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .args([
            "run",
            input_dir.to_str().unwrap(),
            "--output",
            input_dir.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(contains(
            "Output path cannot be a subdirectory of the input path",
        ));
}

#[test]
fn run_negative_behavioral_min_rating_skip() {
    let fixtures = cr3_fixture_dir();
    if !fixtures.join("RAW_FULL_FRAME.CR3").exists() {
        return;
    }
    let model_dir = nima_model_dir();
    if !model_dir.join("manifest.toml").exists() {
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    let input_dir = workspace.path().join("input");
    let output_dir = workspace.path().join("output");
    std::fs::create_dir(&input_dir).unwrap();

    std::fs::copy(
        fixtures.join("RAW_FULL_FRAME.CR3"),
        input_dir.join("RAW_FULL_FRAME.CR3"),
    )
    .unwrap();

    let cat_path = workspace.path().join(".photohelper").join("catalog.db");

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "run",
            input_dir.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
            "--min-rating",
            "5", // Force skip during export, as the fixture scores ~3.9
        ])
        .assert()
        .success()
        // verify export skipped it
        .stderr(contains("skipped-rating: 1"));

    // Verify JPEG does NOT exist
    let exported_jpegs: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jpg"))
        .collect();
    assert!(
        exported_jpegs.is_empty(),
        "JPEG must not be exported due to rating threshold"
    );
}

#[test]
fn develop_lr_label_score_conflicts_with_lr_label() {
    let mut cmd = Command::cargo_bin("photohelper").unwrap();
    cmd.args(["develop", "--lr-label", "--lr-label-score"]);

    cmd.assert()
        .failure()
        .stderr(contains("cannot be used with"));
}

#[test]
fn develop_auto_tone_and_lr_label_score() {
    let input_dir = tempfile::TempDir::new().unwrap();
    let cat_dir = tempfile::TempDir::new().unwrap();
    let cat_path = cat_dir.path().join("catalog.db");

    // Create a RAW file
    let raw_path = input_dir.path().join("photo.cr3");
    std::fs::write(&raw_path, b"dummy raw").unwrap();

    {
        let _db = photohelper_catalog::Catalog::open(&cat_path, 5).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        conn.execute(
            "INSERT INTO photos (
                id, source_path, file_size, mtime_unix_seconds, mtime_anomalous, ingested_at_unix_seconds
            ) VALUES (?1, ?2, 100, 0, 0, 0)",
            rusqlite::params![&b"0123456789abcdef0123456789abcdef"[..], raw_path.to_str().unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) VALUES (?1, 'nima-aesthetic-v1', 8.25, 0)",
            rusqlite::params![&b"0123456789abcdef0123456789abcdef"[..]],
        )
        .unwrap();
    }

    let output = Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "develop",
            "--auto-tone",
            "--lr-label-score",
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Command failed!\nstderr:\n{stderr}"
    );
    println!("Command stderr: {stderr}");

    let xmp_path = raw_path.with_extension("xmp");
    assert!(xmp_path.exists());
    let xmp_content = std::fs::read_to_string(xmp_path).unwrap();
    println!("XMP CONTENT:\n{xmp_content}");

    assert!(xmp_content.contains("crs:AutoTone=\"True\""));
    // Score should be 8.25 (rounded) and zero-padded to 08.25
    assert!(xmp_content.contains("xmp:Label=\"08.25\""));
}

// =====================================================================
// D2 — watermark subcommand integration tests
// =====================================================================

/// Create a minimal valid JPEG at `path` with dimensions `w`×`h`.
fn write_synthetic_jpeg(path: &std::path::Path, w: u32, h: u32) {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([180u8, 100u8, 60u8]));
    let dyn_img = image::DynamicImage::ImageRgb8(img);
    let mut buf = std::io::Cursor::new(Vec::new());
    dyn_img
        .write_to(&mut buf, image::ImageFormat::Jpeg)
        .unwrap();
    std::fs::write(path, buf.into_inner()).unwrap();
}

/// Create a minimal valid PNG at `path` with dimensions `w`×`h`.
fn write_synthetic_png(path: &std::path::Path, w: u32, h: u32) {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([200u8, 200u8, 200u8]));
    let dyn_img = image::DynamicImage::ImageRgb8(img);
    let mut buf = std::io::Cursor::new(Vec::new());
    dyn_img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    std::fs::write(path, buf.into_inner()).unwrap();
}

/// D2 — empty source → summary line with walked: 0.
#[test]
fn watermark_empty_source_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    let mark = tmp.path().join("mark.png");
    std::fs::create_dir_all(&src).unwrap();
    write_synthetic_png(&mark, 50, 50);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            mark.to_str().unwrap(),
            "--mark2",
            mark.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(contains("walked: 0"));
}

/// D2 — happy path: JPEG + PNG source → two output JPEGs, exit 0.
#[test]
fn watermark_happy_path_writes_jpeg_and_png() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    let mark = tmp.path().join("mark.png");
    std::fs::create_dir_all(&src).unwrap();

    // Source files: one JPEG, one PNG (both large enough for marks to fit).
    write_synthetic_jpeg(&src.join("photo.jpg"), 800, 600);
    write_synthetic_png(&src.join("image.png"), 800, 600);
    // Mark: square 60×60
    write_synthetic_png(&mark, 60, 60);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            mark.to_str().unwrap(),
            "--mark2",
            mark.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(contains("written: 2"));

    // Both output JPEGs must exist and be non-empty.
    let j1 = out.join("photo.jpg");
    let j2 = out.join("image.jpg");
    assert!(j1.exists(), "output JPEG for source JPEG must exist");
    assert!(j2.exists(), "output JPEG for source PNG must exist");
    assert!(std::fs::metadata(&j1).unwrap().len() > 0);
    assert!(std::fs::metadata(&j2).unwrap().len() > 0);
}

/// D2 — mark-fit contract: oversized mark + tiny image → exit 2, mark-doesnt-fit: 1, no output.
#[test]
fn watermark_mark_doesnt_fit_exits_2_no_output_written() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    // Very wide mark (500×20) that won't fit at height-14% of a 100×80 image.
    // mark_h = round(80 * 0.14) = 11; scale = 11/20 = 0.55; mark_w = round(500*0.55) = 275
    // image width = 100; 275 >> 100 → MarkDoesNotFit
    let mark_wide = tmp.path().join("wide.png");
    std::fs::create_dir_all(&src).unwrap();
    write_synthetic_jpeg(&src.join("photo.jpg"), 100, 80);
    write_synthetic_png(&mark_wide, 500, 20);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            mark_wide.to_str().unwrap(),
            "--mark2",
            mark_wide.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(contains("mark-doesnt-fit: 1"))
        .stderr(contains("written: 0"));

    // No output JPEG should have been written.
    let out_jpg = out.join("photo.jpg");
    assert!(
        !out_jpg.exists(),
        "no output JPEG should be written when mark doesn't fit"
    );
}

/// D2 — strict mode: mark-doesnt-fit → exit 1.
#[test]
fn watermark_mark_doesnt_fit_strict_exits_1() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    let mark_wide = tmp.path().join("wide.png");
    std::fs::create_dir_all(&src).unwrap();
    write_synthetic_jpeg(&src.join("photo.jpg"), 100, 80);
    write_synthetic_png(&mark_wide, 500, 20);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            mark_wide.to_str().unwrap(),
            "--mark2",
            mark_wide.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--strict",
        ])
        .assert()
        .code(1);
}

/// D2 — idempotency: second run skips existing outputs.
#[test]
fn watermark_idempotent_second_run_skips_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    let mark = tmp.path().join("mark.png");
    std::fs::create_dir_all(&src).unwrap();
    write_synthetic_jpeg(&src.join("photo.jpg"), 400, 300);
    write_synthetic_png(&mark, 40, 40);

    let args = [
        "watermark",
        "--source",
        src.to_str().unwrap(),
        "--mark1",
        mark.to_str().unwrap(),
        "--mark2",
        mark.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
    ];

    // First run: written: 1
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(args)
        .assert()
        .code(0)
        .stderr(contains("written: 1"));

    // Second run: skipped-existing: 1
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args(args)
        .assert()
        .code(0)
        .stderr(contains("skipped-existing: 1"));
}

/// D2 — non-destructive: source bytes unchanged after watermark run.
#[test]
fn watermark_source_files_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    let mark = tmp.path().join("mark.png");
    std::fs::create_dir_all(&src).unwrap();

    let src_jpg = src.join("photo.jpg");
    write_synthetic_jpeg(&src_jpg, 400, 300);
    write_synthetic_png(&mark, 40, 40);

    let before_bytes = std::fs::read(&src_jpg).unwrap();
    let before_mtime = std::fs::metadata(&src_jpg).unwrap().modified().unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            mark.to_str().unwrap(),
            "--mark2",
            mark.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(0);

    let after_bytes = std::fs::read(&src_jpg).unwrap();
    let after_mtime = std::fs::metadata(&src_jpg).unwrap().modified().unwrap();
    assert_eq!(before_bytes, after_bytes, "source bytes must be unchanged");
    assert_eq!(before_mtime, after_mtime, "source mtime must be unchanged");
}

/// D2 — non-PNG mark → fatal error, no output files.
#[test]
fn watermark_non_png_mark_is_fatal() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    let jpeg_mark = tmp.path().join("mark.jpg"); // not PNG
    std::fs::create_dir_all(&src).unwrap();
    write_synthetic_jpeg(&src.join("photo.jpg"), 400, 300);
    write_synthetic_jpeg(&jpeg_mark, 40, 40);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            jpeg_mark.to_str().unwrap(),
            "--mark2",
            jpeg_mark.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure();

    // Output directory may not even have been created or should be empty.
    if out.exists() {
        let entries: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            entries.is_empty(),
            "no output files should be written when mark is non-PNG"
        );
    }
}

/// D2 — output nested in source → rejected up-front.
#[test]
fn watermark_output_nested_in_source_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = src.join("output"); // nested inside source
    let mark = tmp.path().join("mark.png");
    std::fs::create_dir_all(&src).unwrap();
    write_synthetic_jpeg(&src.join("photo.jpg"), 400, 300);
    write_synthetic_png(&mark, 40, 40);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            mark.to_str().unwrap(),
            "--mark2",
            mark.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

// =====================================================================
// D3 — rename subcommand integration tests
// =====================================================================

/// Seed an ingested + scored + clustered CR3 into a catalog.
/// Returns (cat_path, cr3_path) where the CR3 is `photo.CR3` inside `tmp`.
fn seed_renamed_catalog(
    tmp: &tempfile::TempDir,
    score: f64,
    cluster_id: i64,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let cat_path = tmp.path().join("catalog.db");
    let cat_str = cat_path.to_str().unwrap();

    // Step 1: proper catalog init via the real ingest subcommand.
    let cr3 = ingest_fake_cr3(cat_str, tmp.path());

    // Step 2: insert cull score + embedding + cluster via raw SQLite.
    {
        let conn = rusqlite::Connection::open(&cat_path).unwrap();
        let photo_id: Vec<u8> = conn
            .query_row("SELECT id FROM photos LIMIT 1", [], |r| r.get(0))
            .unwrap();

        conn.execute(
            "INSERT INTO cull_scores (photo_id, model_slug, aesthetic_score, scored_at_unix_seconds) \
             VALUES (?1, 'nima-aesthetic-v1', ?2, 1700000001)",
            rusqlite::params![photo_id, score],
        ).unwrap();

        // Embeddings FK required before cluster insert.
        let dummy: Vec<u8> = vec![0u8; 512 * 4];
        conn.execute(
            "INSERT INTO embeddings (photo_id, model_slug, dim, quantization, embedding, embedded_at_unix_seconds) \
             VALUES (?1, 'clip-vit-b32-laion2b-v1', 512, 'f32-le', ?2, 1700000001)",
            rusqlite::params![photo_id, dummy],
        ).unwrap();

        conn.execute(
            "INSERT INTO dup_clusters (photo_id, model_slug, cluster_id, similarity_threshold, clustered_at_unix_seconds) \
             VALUES (?1, 'clip-vit-b32-laion2b-v1', ?2, 0.85, 1700000001)",
            rusqlite::params![photo_id, cluster_id],
        ).unwrap();
    }

    (cat_path, cr3)
}

/// D3 — empty source (no matching rows) → matched: 0, exit 0.
#[test]
fn rename_empty_source_exits_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");

    // Seed a catalog pointing at tmp.path().
    let (cat_path, _) = seed_renamed_catalog(&tmp, 7.0, 1);

    // Use a different source dir so no rows match.
    let other_src = tmp.path().join("other_source");
    std::fs::create_dir_all(&other_src).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "rename",
            "--source",
            other_src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(contains("matched: 0"));
}

/// D3 — happy path: ingested CR3 with score+cluster → renamed file exists, source untouched.
#[test]
fn rename_happy_path_copies_raw_with_correct_name() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    let (cat_path, cr3) = seed_renamed_catalog(&tmp, 7.85, 7);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "rename",
            "--source",
            tmp.path().to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(contains("renamed: 1"))
        .stderr(contains("sidecar-absent: 1"));

    // Expected output: Cluster-007_Cull-07.85-photo.CR3
    let expected = out.join("Cluster-007_Cull-07.85-photo.CR3");
    assert!(expected.exists(), "renamed CR3 must exist");

    // Source untouched.
    assert!(cr3.exists(), "source CR3 must still exist");
    let src_bytes = std::fs::read(&cr3).unwrap();
    assert_eq!(src_bytes.len(), 1024 * 1024, "source bytes unchanged");
}

/// D3 — XMP sidecar copied + renamed alongside the RAW.
#[test]
fn rename_copies_xmp_sidecar() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    let (cat_path, cr3) = seed_renamed_catalog(&tmp, 5.0, 7);

    // Place a stub XMP next to the CR3.
    let xmp = cr3.with_extension("xmp");
    std::fs::write(&xmp, b"<x:xmpmeta/>").unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "rename",
            "--source",
            tmp.path().to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(contains("renamed: 1"))
        .stderr(contains("sidecar-copied: 1"));

    // XMP must be renamed to same stem + .xmp.
    let expected_xmp = out.join("Cluster-007_Cull-05.00-photo.xmp");
    assert!(expected_xmp.exists(), "renamed XMP must exist");
    let content = std::fs::read(&expected_xmp).unwrap();
    assert_eq!(content, b"<x:xmpmeta/>", "XMP must be verbatim copy");
}

/// D3 — missing source file → file-missing: 1, exit 2.
#[test]
fn rename_missing_source_file_exits_2() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    let (cat_path, cr3) = seed_renamed_catalog(&tmp, 6.0, 1);

    // Delete the source CR3 after catalog setup.
    std::fs::remove_file(&cr3).unwrap();

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "rename",
            "--source",
            tmp.path().to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(contains("file-missing: 1"));
}

/// D3 — sidecar-copy failure → NO final renamed RAW, sidecar-copy-failed: 1.
#[test]
#[cfg(unix)]
fn rename_sidecar_copy_fail_no_final_raw_committed() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    let (cat_path, cr3) = seed_renamed_catalog(&tmp, 5.0, 3);

    // Place an XMP sidecar and make it unreadable.
    let xmp = cr3.with_extension("xmp");
    std::fs::write(&xmp, b"<x:xmpmeta/>").unwrap();
    std::fs::set_permissions(&xmp, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "rename",
            "--source",
            tmp.path().to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // Restore permissions for cleanup.
    let _ = std::fs::set_permissions(&xmp, std::fs::Permissions::from_mode(0o644));

    let stderr = String::from_utf8_lossy(&result.stderr);
    // Should report sidecar-copy-failed.
    assert!(
        stderr.contains("sidecar-copy-failed: 1"),
        "expected sidecar-copy-failed: 1, got: {stderr}"
    );

    // No renamed RAW should have been committed to the output directory.
    let expected_raw = out.join("Cluster-003_Cull-05.00-photo.CR3");
    assert!(
        !expected_raw.exists(),
        "no output RAW should exist when sidecar copy failed: {}",
        expected_raw.display()
    );
}

// =====================================================================
// D2 — watermark: unsupported format (JXL/HEIC) counted as skipped (R3-H)
// =====================================================================

/// A file with .jxl extension is not supported by the image crate;
/// the watermark subcommand must warn and count it as skipped-unsupported, exit 0.
#[test]
fn watermark_jxl_file_counted_as_skipped_unsupported() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source");
    let out = tmp.path().join("out");
    let mark = tmp.path().join("mark.png");
    std::fs::create_dir_all(&src).unwrap();

    // A dummy .jxl file — SourceKind::classify checks extension, not content.
    std::fs::write(src.join("photo.jxl"), b"not-jxl-content").unwrap();
    write_synthetic_png(&mark, 50, 50);

    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "watermark",
            "--source",
            src.to_str().unwrap(),
            "--mark1",
            mark.to_str().unwrap(),
            "--mark2",
            mark.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(0)
        .stderr(predicates::str::contains("skipped-unsupported: 1"))
        .stderr(predicates::str::contains("unsupported format"));
}

// =====================================================================
// D1 — export: single-pass --mark1-png path (R3-A)
// =====================================================================

/// export --mark1-png applies the mark via the single-pass render path (no second JPEG cycle).
/// Skipped if LFS fixtures are not present.
#[test]
fn export_single_pass_mark1_png_writes_jpeg() {
    let fixture_dir = cr3_fixture_dir();
    if !fixture_dir.join("CRAW_FULL_FRAME.CR3").exists() {
        return; // LFS fixtures not pulled; skip.
    }

    let tmp = tempfile::tempdir().unwrap();
    let cat_path = tmp.path().join("catalog.db");
    let out_dir = tmp.path().join("export_out");
    let mark = tmp.path().join("mark.png");

    // Small PNG mark (50×50 white square).
    write_synthetic_png(&mark, 50, 50);

    let test_cr3_src = fixture_dir.join("CRAW_FULL_FRAME.CR3");
    let test_cr3_dest = tmp.path().join("CRAW_FULL_FRAME.CR3");
    std::fs::copy(&test_cr3_src, &test_cr3_dest).unwrap();

    // Ingest the CR3.
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

    // Export with --mark1-png and --with-shadow (single-pass path).
    Command::cargo_bin("photohelper")
        .unwrap()
        .env("PHOTOHELPER_HEARTBEAT_INTERVAL_MS", "50000")
        .args([
            "--catalog",
            cat_path.to_str().unwrap(),
            "export",
            "--output",
            out_dir.to_str().unwrap(),
            "--min-rating",
            "0",
            "--long-edge",
            "600",
            "--mark1-png",
            mark.to_str().unwrap(),
            "--with-shadow",
        ])
        .assert()
        .code(0)
        .stderr(predicates::str::contains("written: 1"));

    let expected_jpg = out_dir.join("CRAW_FULL_FRAME.jpg");
    assert!(expected_jpg.exists(), "Output JPEG must exist");
    assert!(
        std::fs::metadata(&expected_jpg).unwrap().len() > 0,
        "Output JPEG must not be empty"
    );
}
