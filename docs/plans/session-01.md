# Session 01 — `cli-skeleton-and-ingest`

> **Branch**: `session-01/cli-skeleton-and-ingest`
> **Started**: 2026-05-27
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v1 (initial, committed `1e636ec`) → v2 (this revision,
> post plan-review Round 1 remediation — see
> `docs/code-reviews/session-01-plan-round1.md`)

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the thinnest end-to-end slice that proves the workspace architecture: a
real `photohelper ingest <path>` subcommand that walks a directory of files,
recognises RAW extensions, reads EXIF, derives a content-addressed `PhotoId`,
and writes catalog rows to a SQLite database at
`<path>/.photohelper/catalog.db`. All other subcommands (`cull`, `develop`,
`export`, `run`, `models`, `camera`) ship as `clap` stubs that exit with
`EX_USAGE (64)` and a stderr "not yet implemented (session NN)" notice, so
scripted pipelines detect the no-op and the CLI surface is visible from day
one.

This session intentionally does **not** decode any RAW pixels — that lands in
session 02 with the LibRaw FFI. EXIF parsing of CR3 containers (via
`kamadak-exif` or equivalent — see §Dependencies) is sufficient to populate
the catalog.

### Scope expansions vs. the bootstrap plan

The bootstrap plan (`/Users/ph/.claude/plans/first-create-a-structure-warm-shell.md`
Phase B) names session-01 deliverables in broad strokes. This plan adds the
following, each justified inline so the "plan as contract" discipline holds:

| Addition | Justification |
|----------|---------------|
| `photohelper-catalog` as an 8th workspace crate | Catalog persistence is a distinct concern from the domain model. Keeping it in `photohelper-core` would couple every future storage swap (kv-store, remote DB) to the domain crate. See Theme 3, Round 1. |
| `--catalog <path>` flag | Operational ergonomics — users with read-only photo trees need to redirect the catalog elsewhere. |
| `--strict` flag on `ingest` | Pairs with the §Observability contract: unknown-camera or per-photo errors escalate to non-zero exit when `--strict` is set. |
| Magic-byte check on existing `--catalog` target | Path-safety guard against accidentally overwriting an unrelated file (Theme 4, Round 1). |
| Explicit `busy_timeout = 5000` on every SQLite connection | Closes the silent-hang failure mode for concurrent `photohelper ingest` invocations (Theme 3, Round 1). |
| `tracing` level table pinned in the plan | Closes the silent-EXIF-failure failure mode (Theme 5, Round 1). |
| `ExifOrientation` full 8-variant enum | Preserves information for session-05 export rotation — collapsing to Landscape/Portrait at ingest is unrecoverable (Theme 8, Round 1). |

Capabilities the v1 plan introduced but **this v2 removes** because they
weren't justified by the v0.1 scope (post-Round-1 simplification): the
`Pipeline` trait, `PipelineCtx` struct, `Sidecar` placeholder enum, and
`CancellationToken`. All four were single-implementer abstractions / dead
invariants that will land in the session that genuinely needs them (sessions
02–04 per the bootstrap-plan roadmap).

### Deliverables (what will exist when the PR merges)

1. **`photohelper-cli` (binary `photohelper`)**
   - `clap` v4 derive API with subcommand handlers: `ingest`, `cull`,
     `develop`, `export`, `run`, `models`, `camera`. Each non-`ingest`
     subcommand has an explicit handler arm that prints
     `"not yet implemented (planned for session NN)"` to stderr and exits
     `64 (EX_USAGE)` so scripts (`photohelper cull && photohelper export`)
     detect the no-op rather than seeing exit-0 silence.
   - Global flags: `--verbose/-v` (repeatable, sets `tracing` level per the
     §Observability table), `--quiet/-q` (suppresses non-error output;
     mutually exclusive with `-v`), `--threads <N>` (default = `num_cpus`;
     `0` and `> 1024` rejected with `clap`'s `value_parser` range), `--catalog
     <path>` (default = `<input>/.photohelper/catalog.db`), `--no-color`.
   - `ingest` flags: `--recursive/-r` (default `true`), `--strict` (default
     `false` — when set, any unknown camera or per-photo error → non-zero
     exit at end-of-run).
   - `tracing-subscriber` initialized in `main` with the compact `fmt` layer
     and an `EnvFilter` that honors `RUST_LOG` overrides; the `-v` count maps
     to base level per §Observability.
   - `indicatif` **spinner** (not progress bar — `par_bridge` consumes
     lazily, so total count is unknown without a pre-pass we deliberately
     don't pay for) showing throughput and live counts. Final summary line
     printed via `tracing::info!` once the walk completes.

2. **`photohelper-core` (lib)** — domain types only; no persistence, no
   pipeline trait this session.
   - `module model` exposing **private-field, constructor-validated** types:
     - `PhotoId(/* private */ [u8; 32])` — content-derived (see §PhotoId
       derivation below). Constructors: `PhotoId::derive(path: &Path) ->
       Result<Self, Error>` (canonical) and `PhotoId::from_db_bytes([u8; 32])
       -> Self` (catalog reconstruction; no validation — bytes already
       trusted from our DB). `Display` impl renders as 43-char `base64url`
       no-pad (BLAKE3 is 32 bytes; base64url-nopad is path-safe and shorter
       than base32's 52 chars).
     - `Photo` — fields private; constructed via `Photo::from_filesystem(
       canonical: AbsPath, /* … */) -> Result<Self, Error>` which enforces
       `file_size > 0` and `canonical` is absolute. Accessors return `&Path`,
       `u64`, etc.
     - `AbsPath` — newtype over `PathBuf` enforcing canonical absolute
       paths. Constructor: `AbsPath::canonicalize(path: impl AsRef<Path>) ->
       Result<Self, Error>` rejects NUL bytes, non-existent paths, and
       returns the canonicalized form via `std::fs::canonicalize`.
     - `CameraId` — `enum CameraId { Known(KnownCamera), Unknown { make:
       String, model: String } }`.
     - `KnownCamera` — `#[non_exhaustive] enum KnownCamera { CanonR8 }`.
       Adding Canon R5 in session 02 is non-breaking.
     - `ExifOrientation` — `#[non_exhaustive] enum ExifOrientation { Normal,
       MirrorH, Rotate180, MirrorV, MirrorHRotate270, Rotate90Cw,
       MirrorHRotate90, Rotate90Ccw }` (full EXIF orientation tag 1..8).
       Plus a `pub fn aspect(&self, width: u32, height: u32) -> Aspect`
       method on `Photo` that derives `Aspect::Landscape | Portrait |
       Square` for callers that just want the high-level question.

   - `module error` exposing `#[non_exhaustive] enum Error` (derive
     `thiserror::Error`, `Debug`). Variants per failure mode:
     - `Io { path: PathBuf, op: &'static str, source: io::Error }` —
       structured context so the user sees *which* file and *which*
       operation failed.
     - `Exif { path: PathBuf, source: <exif lib's error> }`.
     - `HashWindowTooSmall { path: PathBuf, len: u64 }` — surfaced when a
       file is too small to derive a meaningful `PhotoId`; ingest treats it
       as a per-photo skip with a WARN, not a fatal.
     - `CatalogOpen { path: PathBuf, source: rusqlite::Error }` — fatal.
     - `CatalogInsert { photo_id: PhotoId, source: rusqlite::Error }` —
       per-photo skip in non-strict mode.
     - `CatalogPathIsDirectory { path: PathBuf }`.
     - `CatalogPathNotSqlite { path: PathBuf }` (magic-byte check failed).
     - `CatalogLockHeld { path: PathBuf }` (another process holds the
       file lock).
     - `Canonicalize { path: PathBuf, source: io::Error }`.
     - `NulByteInPath { path: PathBuf }`.
     - `CameraProfileNotImplemented { method: &'static str, camera_id: CameraId }`.

   Library returns `Result<T, Error>`. The CLI boundary (in
   `photohelper-cli::main`) converts to `anyhow::Result` with explicit
   `.with_context(|| format!("ingesting {}", path.display()))` and
   `.with_context(|| format!("opening catalog at {}", catalog_path.display()))`
   wrappers at the per-photo loop and catalog-open call sites.

3. **`photohelper-cameras` (lib)**
   - `CameraProfile` trait with method stubs for session-02 work
     (`base_iso`, `sensor_layout`, `color_matrix_d65`, `noise_model`).
     Stubs return `Err(Error::CameraProfileNotImplemented { method: "...",
     camera_id: self.id() })` — **never** `todo!()` / `unimplemented!()` /
     `panic!()` (would fail the workspace `panic = "warn"` clippy lint
     under `-D warnings` and violate `CLAUDE.md § Rust-specific gates` no-
     panics-on-production-paths rule). Search-and-replace target for
     session 02.
   - `CanonR8` struct implementing `CameraProfile` with the EXIF
     identification path only (`id()` returns `CameraId::Known(KnownCamera::
     CanonR8)`; `make_model()` returns `("Canon", "Canon EOS R8")`).
   - `CameraRegistry` with `fn for_exif(&self, make: &str, model: &str) ->
     Option<Arc<dyn CameraProfile>>`. Input normalization: trims whitespace
     and trailing NUL bytes; case-sensitive on `model` (Canon's EXIF strings
     are stable; we document the assumption). Registry initially holds only
     `CanonR8`; unknown bodies return `None` and `ingest` records
     `CameraId::Unknown { make, model }` in the catalog (a non-fatal soft
     fail — see §Observability for the user-visible accounting).

4. **`photohelper-catalog` (lib, NEW 8th workspace crate)** — SQLite-backed
   catalog persistence. Carved out of `photohelper-core` per Round 1
   Theme 3 so `core` stays storage-agnostic.
   - `Catalog::open(path: &AbsPath) -> Result<Self, Error>` — opens
     (or creates) the SQLite DB. Magic-byte check on existing files
     rejects non-SQLite blobs with `Error::CatalogPathNotSqlite`. Rejects
     existing-as-directory with `Error::CatalogPathIsDirectory`. Creates
     missing parent dirs (logged INFO).
   - File-lock: acquires an exclusive `fs2::FileExt::lock_exclusive` on
     a sibling `.photohelper/catalog.db.lock` for the duration of the
     ingest run — second concurrent process exits with
     `Error::CatalogLockHeld`. Closes the TOCTOU race from Round 1
     Theme 3.
   - SQLite session settings on open: `PRAGMA journal_mode = WAL`
     (concurrent reads while writing), `PRAGMA synchronous = NORMAL`
     (safe enough for a catalog; full FSYNC isn't justified), `PRAGMA
     busy_timeout = 5000`.
   - **Schema (v1, owned by this session via
     `docs/decisions/0001-catalog-schema-v1.md`):**
     - `photos` table (columns: `id BLOB PRIMARY KEY` (PhotoId raw bytes),
       `source_path TEXT NOT NULL`, `file_size INTEGER NOT NULL`,
       `mtime_unix_seconds INTEGER NOT NULL` (truncated to 2-second
       boundary per Theme 7 — FAT32 reality), `mtime_anomalous INTEGER
       NOT NULL DEFAULT 0` (set when mtime was clamped out of
       `[1995-01-01, now()+1 day]`), `make TEXT`, `model TEXT`,
       `camera_id TEXT NOT NULL` ('canon-r8' / 'unknown:<make>:<model>'),
       `capture_time_unix_seconds INTEGER`, `width INTEGER`, `height
       INTEGER`, `exif_orientation INTEGER` (raw 1..8), `ingested_at_unix_seconds
       INTEGER NOT NULL`, `superseded_at_unix_seconds INTEGER`).
     - Indices: `idx_photos_source_path` (NOT UNIQUE — same path can
       legitimately have multiple rows when content changes; see Theme 7
       resolution below); `idx_photos_camera_id`.
     - One-statement init: `CREATE TABLE IF NOT EXISTS photos (...);
       CREATE INDEX IF NOT EXISTS ...; PRAGMA user_version = 1;`. **No
       migration framework this session** (deferred per Round 1 Theme 3
       — framework lands when migration 1→2 is on the docket).
     - On open: read `PRAGMA user_version`. If `0`, run init. If `1`, OK.
       If `> 1`, error with `Error::CatalogSchemaTooNew { found, expected: 1 }`
       — closes the forward-incompatibility silent-downgrade hole from
       Round 1 Theme 9.
   - `PhotoRow` struct in `photohelper-catalog::row` with explicit
     `from_row(&rusqlite::Row) -> Result<Self, Error>` and `to_params(&self) ->
     impl rusqlite::Params` boundary. Column-name knowledge confined to this
     module so column reorders don't silently break positional reads.
   - Insert behavior: keyed by `id` (PhotoId PRIMARY KEY). When a file at
     the same `source_path` has changed content (different PhotoId), the new
     row inserts and the old row's `superseded_at_unix_seconds` is set to
     `now()`. Both rows are retained — closes the silent-content-change hole
     from Round 1 Theme 5/Theme 7. `INSERT OR IGNORE` semantics on `id`
     conflict (same content, possibly different path → second path is a
     hardlink/duplicate; no insert, log INFO).
   - **Concurrency model**: `Catalog` wraps `parking_lot::Mutex<rusqlite::
     Connection>` and is `Send + Sync`. Shared across rayon workers via
     `Arc<Catalog>`. Per Round 1 Theme 3 — `Mutex<Connection>` is correct
     for v0.1; the dedicated-writer-thread pattern is deferred until
     profiling justifies it (TD filed at session-end if profiling-driven
     reassessment is needed).

5. **`photohelper-core::ingest` — the ingest worker function** (plain
   function, no trait abstraction this session per Round 1 Theme 2):
   - `pub fn ingest_one(path: &Path, catalog: &Catalog) -> Result<IngestOutcome,
     Error>` where `IngestOutcome` is `enum IngestOutcome { Inserted(PhotoId),
     SupersededPrevious(PhotoId), AlreadyCatalogued(PhotoId),
     SkippedNonRaw, SkippedHashWindowTooSmall }`. Each variant feeds the
     summary tallies in §Observability.
   - The driver (in `photohelper-cli::commands::ingest`) walks via
     `walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok)`,
     filters RAW extensions (lowercased: `.cr3`, `.cr2`, `.arw`, `.nef`,
     `.raf`, `.orf`, `.rw2`, `.dng`), `par_bridge`s the iterator into rayon,
     and each worker calls `ingest_one`.

### PhotoId derivation (locked spec per Round 1 Theme 1)

```text
PhotoId = BLAKE3(
    file_size_u64.to_le_bytes()              // 8 bytes, explicit little-endian
 || mtime_unix_seconds_i64.to_le_bytes()     // 8 bytes, 2s-floored for FAT32
 || first_64KB                                // exactly min(65536, file_size) bytes
 || last_64KB                                 // exactly min(65536, file_size) bytes; empty if file_size ≤ 65536
)
```

- Endianness explicitly little-endian (most-common platform default; documented
  in `PhotoId::derive` docstring).
- `file_size`, `mtime_unix_seconds`, and head+tail content together give enough
  entropy to distinguish Canon R8 burst frames (same camera, near-identical
  headers, but different sensor data lands in `last_64KB`).
- For files where `file_size ≤ 65536`: `first_64KB = whole file`; `last_64KB =
  empty`. The `file_size` prefix still distinguishes a 100-byte file from a
  200-byte file with the same first 100 bytes.
- Render: 43-char `base64url` no-padding (BLAKE3 32 bytes ÷ 6 bits/char = 42.67).
- Edge case: files of size `0` produce `Error::HashWindowTooSmall { path,
  len: 0 }` and are skipped by `ingest_one` with a WARN — zero-byte RAWs are
  almost always copy artifacts.

### Path safety contract (per Round 1 Theme 4)

- Every `source_path` is canonicalized via `AbsPath::canonicalize` before
  catalog insert. Canonicalization rejects: NUL bytes
  (`Error::NulByteInPath`), non-existent paths (`Error::Canonicalize`),
  symlink loops (surfaces as `io::Error` wrapped in `Error::Canonicalize`).
- `--catalog <path>`: missing parent dirs are created (logged INFO).
  Existing-as-directory → `Error::CatalogPathIsDirectory` (fatal).
  Existing-as-non-SQLite-file → `Error::CatalogPathNotSqlite` (fatal,
  magic-byte check looks for "SQLite format 3\0").

### Observability contract (per Round 1 Theme 5)

End-of-run summary printed via `tracing::info!` at session end, *always*:

```text
walked: <N>, ingested: <M>, superseded: <S>, already-catalogued: <A>,
unknown-camera: <U>, skipped (non-RAW): <K>, skipped (too-small): <T>,
errored: <E>
```

Exit-code semantics:

| Condition | Exit code |
|-----------|----------:|
| All RAWs ingested successfully | 0 |
| `walked > 0 && (ingested + superseded + already_catalogued) == 0` (likely wrong directory) | `64 EX_USAGE` |
| `--strict && (unknown_camera > 0 \|\| errored > 0)` | `1` |
| Fatal error (catalog open, lock contention, schema-too-new, IO at root) | `2` |

Tracing-level table (defaults; `RUST_LOG` overrides):

| Event | Level |
|-------|-------|
| EXIF parse failure on a single file | `WARN` |
| Unknown camera first-seen (per make/model) | `WARN` |
| Unknown camera subsequent occurrences (same make/model) | `INFO` |
| Skipped non-RAW extension | `INFO` |
| Skipped (hash window too small) | `WARN` |
| File at same path with different content (superseded) | `INFO` |
| Per-photo successful ingest | `DEBUG` |
| `mtime` clamped (anomalous) | `WARN` |

Default `-v` count = `0` surfaces `WARN` and above to stderr. `-v` → `INFO`,
`-vv` → `DEBUG`, `-vvv` → `TRACE`. `-q` mutes everything below `ERROR`.

`mtime` validation: incoming filesystem mtime outside `[1995-01-01,
now() + 1 day]` is clamped to the nearest boundary, `mtime_anomalous` column
set to 1, and a `WARN` is logged.

### Concurrency (per Round 1 Theme 3 — simplified)

- `rayon`'s default thread pool sized via `--threads` (default `num_cpus`).
- `walkdir::WalkDir` iterator → `par_bridge` → workers call `ingest_one`.
- `Arc<Catalog>` shared across workers; internally uses `parking_lot::Mutex<
  rusqlite::Connection>` so all writes serialize at the SQLite layer
  (correct for v0.1 with WAL journal mode; deferred dedicated-writer-thread
  pattern logged as a potential future TD if profiling justifies it).
- No `crossbeam-channel`, no dedicated writer thread, no `CancellationToken`
  this session — three speculative abstractions removed per Round 1 Theme 2
  and Theme 3.

### Out of scope (deferrals — anything dropped here goes to TECH-DEBT.md only if it leaves a stop-gap behind)

1. LibRaw FFI or any RAW pixel decode (session 02).
2. ONNX, `ort`, any AI model or model registry (sessions 03+).
3. XMP sidecar read/write — neither `crs:` nor `ph:` namespaces (sessions
   04+).
4. `develop`, `export`, watermark, JPEG encode (sessions 04–05).
5. `photohelper-cameras` per-ISO noise model + color matrix bodies
   (`Err(Error::CameraProfileNotImplemented)` stubs only; session 02 fills
   them).
6. Windows build verification (v0.1 target is Linux + macOS; Windows
   catches up in v0.2 per the bootstrap plan).
7. `git-lfs` fixture CR3s (session 02 introduces them; this session uses
   synthesized 64+KB blobs in `tempfile` for tests).
8. Migration framework (single-table v1 schema needs no framework;
   framework lands with migration 1→2 — Round 1 Theme 3).

### Test plan (how each deliverable is verified before session-end)

All tests assert concrete observable behavior per `docs/testing-standards.md`
(repo-local). No `assert!(true)`, no "didn't panic", no
`assert!(result.is_ok())` without checking the inner value.

| Area | Test type | Concrete assertion |
|------|-----------|--------------------|
| `PhotoId::derive` stability | unit (`core::model`) | same file twice → identical PhotoId bytes |
| `PhotoId::derive` distinguishability | unit | two files with identical first/last 64KB but different `file_size` → different PhotoIds (the `file_size` prefix actually distinguishes) |
| `PhotoId::derive` small files | unit | 100-byte file: succeeds (head = whole file, tail = empty); 0-byte file: returns `Error::HashWindowTooSmall { len: 0 }` |
| `PhotoId::derive` cross-platform | unit | mock-control test that injects a fixed `file_size` + `mtime_seconds` + content and asserts the exact BLAKE3 output (regression-proofs the LE byte order) |
| `PhotoId::Display` render | unit | 43-char base64url-nopad output; round-trips through `from_db_bytes` |
| `AbsPath::canonicalize` rejections | unit | NUL byte → `Error::NulByteInPath`; non-existent path → `Error::Canonicalize`; absolute relative input → returns canonical absolute |
| `Catalog::open` magic-byte check | unit (catalog) | text file at the catalog path → `Error::CatalogPathNotSqlite`; directory at the catalog path → `Error::CatalogPathIsDirectory` |
| `Catalog::open` schema-version check | unit | DB with `PRAGMA user_version = 2` → `Error::CatalogSchemaTooNew { found: 2, expected: 1 }`; DB with version = 0 → init runs; version = 1 → OK |
| `Catalog::open` schema init idempotency | unit | running init twice produces identical schema (PRAGMA user_version stable at 1) |
| `Catalog::open` file-lock | integration | two concurrent `Catalog::open` on the same path → second errors with `Error::CatalogLockHeld` |
| `Catalog` insert: new content same path | unit | inserting a PhotoId for a `source_path` that already has a (different-PhotoId) row → new row inserts, old row's `superseded_at_unix_seconds` set |
| `Catalog` insert: same content same path | unit | second insert of identical PhotoId → INSERT OR IGNORE, log INFO, row count unchanged |
| `CameraRegistry::for_exif` known body | unit | `("Canon", "Canon EOS R8")` → `Some(CanonR8)`; `("canon", "...")` (lowercase) → `None` (documented case-sensitive contract) |
| `CameraRegistry::for_exif` normalization | unit | trailing NUL bytes and surrounding whitespace stripped before lookup |
| `CameraRegistry::for_exif` unknown body | unit | `("Acme", "X1")` → `None` (NOT a panic) |
| `CameraProfile` stub methods | unit | `CanonR8::base_iso()` → `Err(Error::CameraProfileNotImplemented { method: "base_iso", camera_id: ... })` (NOT a panic; verifies the lint-clean stub pattern) |
| `ExifOrientation` parse | unit | all 8 EXIF orientation tag values map to the correct variant; out-of-range value (0, 9) → `Error::Exif` |
| `ingest_one` happy path | unit (core::ingest) | `.cr3` file → `IngestOutcome::Inserted(photo_id)`; catalog has one row matching |
| `ingest_one` non-RAW filter | unit | `.jpg` file → `IngestOutcome::SkippedNonRaw`; catalog row count unchanged |
| `ingest` CLI happy path | integration (`tests/cli.rs`) | 2 files (one `.cr3`, one `.jpg`) in tempdir; assert exit 0; assert stderr contains `walked: 2`, `ingested: 1`, `skipped (non-RAW): 1`; assert exactly one row in `photos`; assert that row's `source_path` ends with `a.cr3` AND `file_size` matches the test fixture AND `id` is 32 bytes |
| `ingest` CLI idempotency | integration | run twice; second run stderr contains `already-catalogued: 1, ingested: 0`; row count stays at 1 |
| `ingest` CLI content change | integration | ingest a file; rewrite the file with different bytes; ingest again; assert two rows in `photos` for the same `source_path`, the first with `superseded_at_unix_seconds` set |
| `ingest` CLI empty / wrong directory | integration | dir of only `.jpg` files; assert exit code = `64 EX_USAGE`; stderr contains `ingested: 0` |
| `ingest` CLI `--strict` with unknown camera | integration | tempdir with a synthesized "RAW" whose EXIF says `("Acme", "X1")`; without `--strict` → exit 0; with `--strict` → exit 1 |
| `ingest` CLI walker edges | integration (single consolidated test) | a tempdir containing: a hidden `.foo.cr3` (cataloged — we treat hidden files normally), a symlink loop (handled — no infinite recursion via `walkdir`'s default symlink behavior), a deeply nested empty subdir (no error). Assert no panic + sensible row count. |
| Stub subcommands | integration (parameterized over `cull`/`develop`/`export`/`run`/`models`/`camera`) | each → exit `64`; stderr contains `not yet implemented` |
| CLI `--verbose` mapping | integration | `-v` enables INFO (verify an INFO event appears in stderr); `-vv` enables DEBUG; `-q` mutes everything below ERROR |
| CLI `--threads` boundaries | integration | `--threads 0` → clap rejects with usage error (exit 2); `--threads 2000` → clap rejects |
| CLI `--catalog` override | integration | `--catalog /tmp/explicit.db` → DB created at exactly that path, not at `<input>/.photohelper/catalog.db` |
| Workspace gates | `just ci` | fmt-check, clippy `-D warnings`, test, audit, verify-state all green |

Approximately 28 tests total (unit + integration). The volume is justified
by the breadth of edge cases the Round 1 review surfaced — every CRITICAL
hole has a test.

### Checkpoints firing this session (Cadence A)

| Checkpoint | When | Cadence-A tier | Agents | Double-review? |
|------------|------|----------------|--------|----------------|
| Session start | done | Tier 1 | 1 — `general-purpose` (alignment) | No |
| **Plan-review** | this checkpoint; Round 1 complete, Round 2 pending | Tier 5 | **Full 8** | **Yes** |
| Sub-component review | invoked only if a module/file crosses the trigger from `docs/quality-assurance.md § Sub-component review protocol` (first non-scaffold public API; file > ~300 LoC non-test). Realistic candidates this session: `photohelper-catalog` first public API; `photohelper-cli::commands::ingest` driver if it grows past 300 LoC | Tier 4 | 3–5 per boundary | Yes |
| **Session end** | before commit + push | Tier 5 | **Full 8** | **Yes** |

Round 1 → remediate → Round 2 → remediate is enforced at every checkpoint
(`docs/quality-assurance.md § Double-review protocol`). Never stop after
Round 1. If Round 2 surfaces CRITICAL-class regressions, add Round 3.

### Expected discovery items

- **Partially resolves DN-005** (catalog schema): lands v1 minimal schema
  slice plus the `docs/decisions/0001-catalog-schema-v1.md` decision doc.
  DN-005 stays **open** — session 02 still owes the dup-group and
  culling-score tables.
- **Potential new DN-006**: EXIF reader choice. Plan defaults to
  `kamadak-exif` (most-stars Rust EXIF crate on crates.io as of plan
  drafting, MIT, pure Rust). If session-01 implementation surfaces gaps
  with CR3 container EXIF, file DN-006 and revisit in session 02 when
  LibRaw provides an alternate source.
- **Potential new DN-007**: `rusqlite` static-link binary size impact
  (~1.5 MB). If this becomes a release-engineering concern, file as DN.
- **Potential new DN-008**: `parking_lot::Mutex<Connection>` write
  serialization throughput. If profiling shows the mutex is the bottleneck
  on 10k+ photo runs, file DN and revisit the dedicated-writer-thread
  pattern.

### Tech-debt entries created or touched this session (preview — finalized at session end)

- No new TDs anticipated for the planned scope; every item above is a real
  deliverable, not a stop-gap.
- TD-001 (action pinning) is **not** touched this session — gated on "before
  first external contributor or first release," neither triggered yet.

### Dependencies introduced this session

All deps declared in `[workspace.dependencies]` at the root `Cargo.toml`, then
per-crate `dependencies = { workspace = true }` references. Version ranges
follow caret (`^x.y`) per Cargo defaults unless noted.

| Crate | Version | Used by | Features |
|-------|--------:|---------|----------|
| `clap` | 4 | cli | `derive` |
| `tracing` | 0.1 | cli, core, catalog | (none) |
| `tracing-subscriber` | 0.3 | cli | `env-filter`, `fmt` |
| `anyhow` | 1 | cli | (none) |
| `thiserror` | 2 | core, catalog | (none) |
| `walkdir` | 2 | cli | (none) |
| `rayon` | 1 | cli | (none) |
| `indicatif` | 0.17 | cli | (none) |
| `blake3` | 1 | core | (none) |
| `base64` | 0.22 | core | (none) — for the base64url-nopad PhotoId render |
| `kamadak-exif` | 0.6 | core | (none) — EXIF reader; default choice; DN-006 reserves the right to switch |
| `time` | 0.3 | core, catalog | `macros` |
| `rusqlite` | 0.32 | catalog | `bundled` — static SQLite amalgamation; closes system-libsqlite3 version-skew risk per Round 1 Theme 3 NOTE |
| `parking_lot` | 0.12 | catalog | (none) — `Mutex<Connection>` |
| `fs2` | 0.4 | catalog | (none) — file lock for TOCTOU |
| `num_cpus` | 1 | cli | (none) — `--threads` default |
| `assert_cmd` | 2 | cli (dev-dep) | (none) |
| `predicates` | 3 | cli (dev-dep) | (none) — for `assert_cmd` stderr substring matching |
| `tempfile` | 3 | cli, catalog (dev-dep) | (none) |

A `cargo tree --workspace --depth 1` audit runs at session-end; if the
transitive crate count exceeds 120, file a TD and audit the heaviest
subtrees (per Round 1 Theme 3 dep-cliff finding).

### Non-goals (clarifying boundary)

- This session does **not** optimize ingest throughput. `Mutex<Connection>`
  is simple and correct; if profiling shows the mutex is the bottleneck on
  10k+ photo runs, that's a session-02+ concern (DN-008 reserved).
- This session does **not** start the AI work. Even though v0.1 is
  "AI-first," AI features need a `Photo` + decoded pixels to feed them. We
  need the catalog first.
- This session does **not** implement cooperative cancellation / SIGINT
  handling. `CancellationToken` was removed during Round 1 remediation; it
  lands in the session that genuinely needs to interrupt a long-running
  develop/export run.

---

**Implementation notes (added after plan-review Round 2 + remediation; not
in scope for this contract block):**

Pseudocode, package picks beyond the locked deps above, concrete SQL DDL
strings, and the sub-component review plan will be appended below this line
*after* Round 2 of plan-review is clean. Until then, the contract above is
the load-bearing artifact.
