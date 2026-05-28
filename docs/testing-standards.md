# photohelper — Testing Standards

> Repo-local restatement of the assertion-quality rules so plans and reviews
> don't depend on any external file. Reference these from session plans and
> code reviews instead of `~/.claude/CLAUDE.md` (a private path that won't
> resolve for external contributors or fresh CI runners).

## Meaningful assertions only

The following assertions are **never** acceptable and should be flagged as
blocking issues in code reviews:

- `assert!(true)` / `assert_eq!(1, 1)` / `expect(true).toBe(true)` / `expect(1).toBe(1)` and any equivalent.
- `assert!(x.is_some())` / `assert!(result.is_ok())` without verifying the actual
  inner value when the inner value is meaningful (it almost always is).
- Tests that exercise code without an `assert` of any kind — "didn't panic" is
  not a test, it is a smoke run.
- Snapshot-style tests of unstable values (timestamps, hashes generated from
  current time, allocation addresses) that exist to "have a test" without
  catching regression.

These provide false confidence and are noise.

## Every test must

1. **Verify actual behavior** — assert on real outputs, real database state,
   real file contents, real exit codes, real stderr substrings.
2. **Be specific** — assert exact values, exact column counts, exact error
   variants, exact `tracing` events. "Not null" is rarely enough.
3. **Fail when the code is broken** — if removing the test subject doesn't
   break the test, the test is invalid.
4. **Have a description that matches its assertion** — `it("should not open
   dialog when event is null") { ... }` and the assertion checks dialog-open
   spy. Mismatched intent = silent rot.

## Coverage expectations

- **Unit tests** for every new business-logic function, every new type with an
  invariant, every new error variant.
- **Integration tests** for every CLI subcommand surface, every cross-crate
  boundary that ships a public API.
- **Edge cases** must be tested explicitly: null/None/empty paths, boundary
  numeric values, filesystem edge cases (FAT32 mtime granularity, symlinks,
  hidden files, non-UTF-8 paths), Unicode-heavy strings.
- **Error paths** must be tested, not just happy paths. Each `thiserror`
  variant has a test that produces it.
- **Async / concurrent code** — explicit tests for cancellation, deadlock,
  panic-in-worker, dropped-receiver.

## Examples

### Unacceptable

```rust
#[test]
fn ingest_works() {
    photohelper_cli::ingest("/tmp/photos").unwrap();
    assert!(true);                                       // MEANINGLESS
}

#[test]
fn catalog_exists() {
    let cat = Catalog::open("/tmp/cat.db").unwrap();
    assert!(cat.path().is_some());                       // TOO VAGUE
}

#[test]
fn handles_unknown_camera() {
    let result = registry.for_exif("Acme", "X1");
    let _ = result;                                      // NO ASSERTION
}
```

### Required

```rust
#[test]
fn ingest_writes_one_row_per_raw_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.cr3"), &[0u8; 1024]).unwrap();
    std::fs::write(dir.path().join("b.jpg"), &[0u8; 1024]).unwrap();

    let assert = assert_cmd::Command::cargo_bin("photohelper")
        .unwrap()
        .args(["ingest", dir.path().to_str().unwrap()])
        .assert()
        .success();

    assert
        .stderr(predicates::str::contains("walked: 2"))
        .stderr(predicates::str::contains("ingested: 1"))
        .stderr(predicates::str::contains("skipped (non-RAW): 1"));

    let conn = rusqlite::Connection::open(dir.path().join(".photohelper/catalog.db")).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "exactly one RAW row should be cataloged");

    let row_path: String = conn
        .query_row("SELECT source_path FROM photos LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert!(row_path.ends_with("a.cr3"), "row should be the .cr3, got {row_path}");
}

#[test]
fn registry_for_exif_returns_canon_r8_for_canon_eos_r8() {
    let registry = CameraRegistry::default();
    let profile = registry
        .for_exif("Canon", "Canon EOS R8")
        .expect("Canon R8 must be registered");
    assert_eq!(profile.id(), CameraId::Known(KnownCamera::CanonR8));
}

#[test]
fn registry_for_exif_returns_none_for_unknown_body() {
    let registry = CameraRegistry::default();
    assert!(registry.for_exif("Acme", "X1").is_none());
}
```

## Code-review policy

- **Block merge** if any meaningless assertion is found.
- **Block merge** if tests don't verify actual behavior.
- **Request changes** if a test description doesn't match its assertion.
- **Request changes** if edge cases or error paths aren't covered.

This document is the canonical reference; cite it (not external files) from
session plans, code-review artifacts, and PR descriptions.
