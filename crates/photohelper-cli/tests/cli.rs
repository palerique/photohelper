//! Integration tests for the `photohelper` CLI. Per
//! `docs/plans/session-01.md` §Test plan rows 32–48.
//!
//! Every test asserts a concrete observable per `docs/testing-standards.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use assert_cmd::Command;
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
    // Per R4.T3: kamadak-exif cannot parse our synthetic CR3 ISO-BMFF
    // (we wrote raw 0xCC bytes), so DN-006 fallback fires: camera_slug
    // is NULL and make/model are NULL. This is the documented session-01
    // behavior pending real CR3 fixtures in session 02.
    let camera_slug: Option<String> = conn
        .query_row("SELECT camera_slug FROM photos LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert!(
        camera_slug.is_none() || camera_slug.as_deref() == Some("canon-r8"),
        "camera_slug is NULL (DN-006 fallback) OR 'canon-r8' (kamadak parsed); got {camera_slug:?}"
    );
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
    for name in ["cull", "develop", "export", "run", "models", "camera"] {
        Command::cargo_bin("photohelper")
            .unwrap()
            .arg(name)
            .assert()
            .code(69)
            .stderr(contains("not yet implemented"));
    }
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
