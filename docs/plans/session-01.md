# Session 01 — `cli-skeleton-and-ingest`

> **Branch**: `session-01/cli-skeleton-and-ingest`
> **Started**: 2026-05-27
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v1 (initial, `1e636ec`) → v2 (post plan-review Round 1
> — see `docs/code-reviews/session-01-plan-round1.md`) → **v3 (this
> revision; post plan-review Round 2 — see
> `docs/code-reviews/session-01-plan-round2.md`)**

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the thinnest end-to-end slice that proves the workspace architecture: a
real `photohelper ingest <path>` subcommand that walks a directory of files,
recognises RAW extensions, reads EXIF, derives a content-addressed `PhotoId`,
and writes catalog rows to a SQLite database at
`<path>/.photohelper/catalog.db`. All other subcommands (`cull`, `develop`,
`export`, `run`, `models`, `camera`) ship as `clap` stubs that exit with
`69 EX_UNAVAILABLE` (feature not yet implemented) and a stderr "not yet
implemented (session NN)" notice.

This session intentionally does **not** decode any RAW pixels — that lands in
session 02 with the LibRaw FFI. EXIF parsing of CR3 containers via the
chosen reader (see §Dependencies + DN-006 fallback) is sufficient to
populate the catalog.

### Scope expansions vs. the bootstrap plan

Bootstrap plan Phase B names session-01 deliverables in broad strokes. This
plan adds the following, each justified inline:

| Addition | Justification |
|----------|---------------|
| `photohelper-catalog` as an 8th workspace crate | Catalog persistence is a distinct concern from the domain model (Round 1 Theme 3). |
| `--catalog <path>` flag | Operational ergonomics — users with read-only photo trees need to redirect the catalog elsewhere. |
| `--strict` flag on `ingest` | Pairs with the §Observability contract: unknown-camera or per-photo errors escalate to non-zero exit when set. Justified for the v0.1 "AI-first batch processing" audience that scripts the CLI. |
| Magic-byte check on existing `--catalog` target | Path-safety guard against accidentally overwriting an unrelated file (Round 1 Theme 4). |
| Explicit `busy_timeout = 5000` on every SQLite connection | Closes the silent-hang failure mode for concurrent invocations (Round 1 Theme 3). |
| `tracing` level table pinned in the plan | Closes the silent-EXIF-failure failure mode (Round 1 Theme 5). |
| `ExifOrientation` full 8-variant enum | Preserves information for session-05 export rotation (Round 1 Theme 8). |
| Path-escape check (canonical form must `starts_with(ingestion_root)`) | Closes a Round 1 Theme 4 carry-forward caught in Round 2 (Round 2 T5). |
| Compile-time `assert_send_sync::<Arc<Catalog>>()` | Mechanically verifies the Send+Sync claim that Round 1 Theme 3 + Round 2 T6 surfaced. |

v1 → v2 removed: `Pipeline` trait, `PipelineCtx`, `Sidecar` placeholder enum,
`CancellationToken` (all YAGNI per Round 1 Theme 2).

v2 → v3 adjustments (per Round 2 findings): `ingest_one` moved from
`photohelper-core` to `photohelper-cli::commands::ingest` to preserve the
core-stays-storage-agnostic boundary (Round 2 T7); `std::sync::Mutex<Connection>`
chosen over `parking_lot::Mutex` for panic-poison safety (Round 2 T4);
`fs4::try_lock_exclusive` with explicit retry budget chosen over
`fs2::lock_exclusive` blocking call (Round 2 T1 + T3); fatal-error exit code
moved from `2` to `74 EX_IOERR` to avoid the clap-parse-error collision
(Round 2 T8 + T10); stub-subcommand exit code moved from `64 EX_USAGE` to
`69 EX_UNAVAILABLE` for semantic accuracy (Round 2 T10); `ExifOrientation`
variant order corrected to match EXIF canonical tag 1..8 (Round 2 T9);
`camera_id TEXT` synthetic key dropped in favor of existing `make`/`model`
columns + a `camera_known BOOLEAN` (Round 2 T9 — eliminates encoding
ambiguity); `Canonicalize` and `NulByteInPath` Error variants collapsed into
`Io { path, op, source }` (Round 2 T13).

### Deliverables (when the PR merges, the following will exist)

1. **`photohelper-cli` (binary `photohelper`)**
   - `clap` v4 derive API with subcommand handlers: `ingest`, `cull`,
     `develop`, `export`, `run`, `models`, `camera`. Every non-`ingest`
     handler will print `"not yet implemented (planned for session NN)"` to
     stderr and exit `69 EX_UNAVAILABLE`.
   - Global flags: `--verbose/-v` (repeatable, sets `tracing` level per the
     §Observability contract table), `--quiet/-q` (suppresses non-error
     tracing output but **NOT** the end-of-run summary line — see
     §Observability contract), `--threads <N>` (default = `num_cpus`;
     `value_parser` range `1..=1024` — clap exits 2 on out-of-range),
     `--catalog <path>` (default = `<input>/.photohelper/catalog.db`),
     `--no-color`.
   - `ingest` flags: `--recursive/-r` (default `true`), `--strict` (default
     `false`; when set, any unknown camera or per-photo error → non-zero
     exit at end-of-run).
   - `tracing-subscriber` will initialize in `main` with the compact `fmt`
     layer and an `EnvFilter` that honors `RUST_LOG` overrides; the `-v`
     count will map to base level per the §Observability contract.
   - `indicatif` spinner (not progress bar — `par_bridge` is lazy and the
     total file count is unknown without a pre-pass we deliberately don't
     pay for) showing throughput and live counts. End-of-run summary line
     will print via `eprintln!` directly (bypassing the tracing filter, so
     it survives `-q`).
   - A periodic heartbeat thread will fire `tracing::info!("walked {N},
     ingested {M}, in-flight {P}")` every 10 seconds during the ingest
     run, then stop at end-of-walk. Closes the stuck-worker observability
     hole from Round 2 T8.
   - `ingest` driver (in `photohelper-cli::commands::ingest`) will walk
     via `walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok)`,
     filter RAW extensions (lowercased: `.cr3`, `.cr2`, `.arw`, `.nef`,
     `.raf`, `.orf`, `.rw2`, `.dng`), `par_bridge` the iterator into rayon,
     and each worker calls `ingest_one(path: &Path, root: &AbsPath,
     catalog: &Catalog, stats: &IngestStats) -> Result<IngestOutcome,
     Error>` defined alongside in the same module. Lives in `cli`, NOT
     `core`, per Round 2 T7.

2. **`photohelper-core` (lib)** — domain types only. No persistence, no
   pipeline trait, no `ingest_one` function this session.
   - `module model` will expose private-field, constructor-validated types:
     - `PhotoId(/* private */ [u8; 32])` — content-derived per the locked
       §PhotoId derivation spec below. Constructors will be
       `PhotoId::derive(path: &Path) -> Result<Self, Error>` (canonical)
       and **`pub(crate) fn from_db_bytes(raw: [u8; 32]) -> Self`** — NOT
       public. Catalog-layer reconstruction will go through a sealed
       constructor: `pub fn PhotoId::reconstruct_from_catalog(token:
       CatalogReconstructionToken, raw: [u8; 32]) -> Self`, where
       `CatalogReconstructionToken` is a unit struct that **only**
       `photohelper-catalog` can construct (its constructor is
       `pub(crate)` inside `photohelper-catalog`). Closes the forgery
       bypass from Round 2 T2 without forcing catalog into core's module
       tree. `Display` will render as 43-char `base64url` no-pad.
     - `Photo` — fields private; will be constructed via
       `Photo::from_filesystem(canonical: AbsPath, file_size: u64,
       mtime_unix_seconds: i64, exif: ExifMetadata) -> Result<Self,
       Error>` enforcing `file_size > 0`. Accessors will return `&Path`,
       `u64`, etc.
     - `AbsPath` — newtype over `PathBuf` enforcing canonical absolute
       paths. Constructor: `AbsPath::canonicalize(path: impl AsRef<Path>)
       -> Result<Self, Error>` will reject NUL bytes (returns
       `Error::Io { op: "canonicalize-nul-check", ... }`) and non-existent
       paths (returns `Error::Io { op: "canonicalize", ... }`) and return
       the canonicalized form via `std::fs::canonicalize`. **Will impl
       `AsRef<Path>` and expose `pub fn as_path(&self) -> &Path`** —
       closes ergonomics gap from Round 2 T9. Field is private.
       Additional constructor `AbsPath::canonicalize_within(root: &AbsPath,
       path: impl AsRef<Path>) -> Result<Self, Error>` that rejects with
       `Error::PathEscapesRoot { path, root }` if `canonical.starts_with
       (root.as_path())` is false. **Restores the Round 1 Theme 4
       escape-check that was dropped in v2 (Round 2 T5).**
     - `CameraId` — `enum CameraId { Known(KnownCamera), Unknown { make:
       String, model: String } }`.
     - `KnownCamera` — `#[non_exhaustive] enum KnownCamera { CanonR8 }`.
       Will provide `pub fn slug(&self) -> &'static str` (`CanonR8 =>
       "canon-r8"`) and `pub fn from_slug(slug: &str) -> Option<Self>`.
       Round-tripped via unit test.
     - `ExifOrientation` — `#[non_exhaustive] enum ExifOrientation`
       matching the **EXIF canonical tag mapping 1..=8**:
       ```
       1 = Normal              (no transform)
       2 = MirrorHorizontal
       3 = Rotate180
       4 = MirrorVertical
       5 = MirrorHorizontalRotate90Cw   (transpose)
       6 = Rotate90Cw
       7 = MirrorHorizontalRotate90Ccw  (transverse)
       8 = Rotate90Ccw
       ```
       `pub fn from_tag(tag: i64) -> Result<Self, Error>` returns
       `Error::Exif` for tag values outside 1..=8. `pub fn to_tag(&self)
       -> i64`. Unit test asserts `from_tag(N).unwrap().to_tag() == N`
       for N ∈ 1..=8 — closes Round 2 T9 correctness bug (v2's
       MirrorHRotate270 at slot 5 was wrong). Plus a `pub fn aspect
       (&self, width: u32, height: u32) -> Aspect` method on `Photo`
       deriving `Aspect::Landscape | Portrait | Square` for callers
       wanting the high-level question.
     - `IngestOutcome` — `#[non_exhaustive] enum IngestOutcome { Inserted
       { photo_id: PhotoId, camera_known: bool, no_exif_fields: bool,
       mtime_anomalous: bool }, SupersededPrevious { photo_id: PhotoId },
       AlreadyCatalogued { photo_id: PhotoId }, SkippedNonRaw,
       SkippedHashWindowTooSmall, NoExifFields }`. The boolean flags on
       `Inserted` route into the §Observability summary tallies for
       unknown-camera, no-EXIF, and mtime-anomalous without requiring
       separate variants.

   - `module error` will expose `#[non_exhaustive] enum Error` (derive
     `thiserror::Error`, `Debug`) with **explicit per-call-site mapping**
     — NO `#[from]` derives anywhere (closes Round 2 T9 ambiguity). Every
     error site will use `.map_err(|e| Error::Io { path, op: "...",
     source: e })`. Variants:
     - `Io { path: PathBuf, op: &'static str, source: io::Error }` —
       absorbs canonicalize, NUL-check, ingest-time IO. Op tags include
       `"canonicalize"`, `"canonicalize-nul-check"`, `"read-prefix"`,
       `"stat"`.
     - `Exif { path: PathBuf, source: Box<dyn std::error::Error + Send + Sync> }`
       (chosen lib's error type boxed to keep `Error` Sized).
     - `HashWindowTooSmall { path: PathBuf, len: u64 }` — returned by
       `PhotoId::derive`; the driver maps to
       `IngestOutcome::SkippedHashWindowTooSmall` with a WARN log (not a
       fatal). Explicitly: `PhotoId::derive` produces the `Err`; the
       driver in `cli::commands::ingest` does the skip mapping.
     - `CatalogOpen { path: PathBuf, source: rusqlite::Error }` — fatal.
     - `CatalogInsert { photo_id: PhotoId, source: rusqlite::Error }` —
       per-photo skip in non-strict mode.
     - `CatalogPathIsDirectory { path: PathBuf }` — fatal.
     - `CatalogPathNotSqlite { path: PathBuf }` (magic-byte check failed)
       — fatal.
     - `CatalogLockHeld { path: PathBuf }` — fatal after retry budget
       exhausted.
     - `CatalogSchemaTooNew { found: i64, expected: i64 }` — fatal.
     - `CatalogPoisoned { path: PathBuf }` — surfaced when a worker
       panicked while holding the mutex (std::sync::Mutex poison —
       closes Round 2 T4).
     - `PathEscapesRoot { path: PathBuf, root: PathBuf }` — emitted by
       `AbsPath::canonicalize_within` (closes Round 2 T5).
     - `CameraProfileNotImplemented { method: &'static str, camera_id: CameraId }`
       — returned by stub `CameraProfile` methods so calling them is a
       typed `Err`, not a panic.

   Library returns `Result<T, Error>`. The CLI boundary in
   `photohelper-cli::main` and `photohelper-cli::commands::ingest`
   converts to `anyhow::Result` with mandatory `.with_context(...)` at
   the per-photo loop (`|| format!("ingesting {}", path.display())`) and
   catalog-open call site (`|| format!("opening catalog at {}",
   catalog_path.display())`).

3. **`photohelper-cameras` (lib)**
   - `CameraProfile` trait with method stubs for session-02 work
     (`base_iso`, `sensor_layout`, `color_matrix_d65`, `noise_model`).
     Stubs will return `Err(Error::CameraProfileNotImplemented { method:
     "...", camera_id: self.id() })` — never `todo!()` / `unimplemented!()`
     / `panic!()`.
   - `CanonR8` struct implementing `CameraProfile` with the EXIF
     identification path only (`id()` returns `CameraId::Known(KnownCamera::
     CanonR8)`; `make_model()` returns `("Canon", "Canon EOS R8")`).
   - `CameraRegistry` with `fn for_exif(&self, make: &str, model: &str) ->
     Option<Arc<dyn CameraProfile>>`. Input normalization will trim
     whitespace and trailing NUL bytes; case-sensitive on `model` (Canon's
     EXIF strings are stable). Registry initially holds only `CanonR8`;
     unknown bodies return `None` and the catalog row's `make`/`model`
     columns are populated with the EXIF strings and `camera_known = 0`.

4. **`photohelper-catalog` (lib, NEW 8th workspace crate)** — SQLite-backed
   catalog persistence. Carved out of `photohelper-core` per Round 1
   Theme 3.
   - **`Catalog` struct** (explicit field list per Round 2 T6):
     ```
     pub struct Catalog {
         conn: std::sync::Mutex<rusqlite::Connection>,
         _lock_handle: std::fs::File,   // held for lifetime of Catalog
         canonical_path: AbsPath,
     }
     ```
     All three fields are `Send + Sync`. Compile-time assertion in the
     test module: `const _: fn() = || { fn assert_send_sync<T: Send +
     Sync>() {} assert_send_sync::<Arc<Catalog>>(); };`. Closes Round 2 T6.
   - `Catalog::open(catalog_path: impl AsRef<Path>) -> Result<Self, Error>`
     will run in this exact order to close TOCTOU + lock-ordering
     (Round 2 T3):
     1. Compute `lock_path = <parent>/.photohelper/catalog.db.lock`.
     2. Create `<parent>/.photohelper/` if missing (logged INFO);
        partial failure → `Error::Io { op: "mkdir-p", ... }` naming the
        exact failing component (closes Round 2 T8 partial-success).
     3. Open `lock_path` (create if missing).
     4. Acquire exclusive file lock with retry budget:
        `fs4::FileExt::try_lock_exclusive` in a loop — up to 5 attempts
        at 500ms each, WARN logged per retry, then `Error::CatalogLockHeld`
        if budget exhausted. Bounded wait, user-visible.
     5. Verify the existing catalog file (if any): existing-as-directory
        → `Error::CatalogPathIsDirectory`; existing non-empty
        file whose first 16 bytes are NOT `"SQLite format 3\0"` →
        `Error::CatalogPathNotSqlite`.
     6. Open `rusqlite::Connection`; on failure return `Error::CatalogOpen`.
     7. Set PRAGMAs: `journal_mode = WAL`, `synchronous = NORMAL`,
        `busy_timeout = 5000`.
     8. Read `PRAGMA user_version`. If `0`, run init (transactional —
        see below). If `1`, OK. If `> 1`,
        `Error::CatalogSchemaTooNew { found, expected: 1 }`.
     9. Run `PRAGMA wal_checkpoint(TRUNCATE)`; if it reports recovered
        frames (> 0), log `WARN` "previous shutdown was unclean;
        recovered N WAL frames" — closes Round 2 T4 power-loss
        observability.
     10. Construct `Catalog { conn, _lock_handle, canonical_path }`.
   - **Schema init transactional** (closes Round 2 T12):
     ```sql
     BEGIN IMMEDIATE;
       CREATE TABLE IF NOT EXISTS photos (
         id BLOB PRIMARY KEY,                          -- PhotoId raw bytes
         source_path TEXT NOT NULL,                    -- canonical absolute
         file_size INTEGER NOT NULL,
         mtime_unix_seconds INTEGER NOT NULL,          -- 2s-floored
         mtime_anomalous INTEGER NOT NULL DEFAULT 0,
         make TEXT,                                    -- raw EXIF make
         model TEXT,                                   -- raw EXIF model
         camera_known INTEGER NOT NULL DEFAULT 0,      -- 1 iff KnownCamera lookup succeeded
         camera_slug TEXT,                             -- 'canon-r8' iff known; NULL otherwise
         capture_time_unix_seconds INTEGER,
         width INTEGER,
         height INTEGER,
         exif_orientation INTEGER,                     -- raw 1..=8
         ingested_at_unix_seconds INTEGER NOT NULL,
         superseded_at_unix_seconds INTEGER
       );
       CREATE INDEX IF NOT EXISTS idx_photos_source_path ON photos(source_path);
       CREATE INDEX IF NOT EXISTS idx_photos_camera_slug ON photos(camera_slug);
       PRAGMA user_version = 1;
     COMMIT;
     ```
     Note: dropped the synthetic `camera_id TEXT` key from v2 — `make` +
     `model` (raw EXIF) + `camera_known` (BOOLEAN) + `camera_slug`
     (NULL when unknown) carry the same information without the colon-
     delimiting ambiguity (closes Round 2 T9). DN-005 v1 schema is
     authoritatively documented in **`docs/decisions/0001-catalog-schema-v1.md`**
     (deliverable artifact — listed explicitly in §Deliverables 8 below
     so it doesn't get lost at session-end).
   - **`PhotoRow` struct** in `photohelper-catalog::row` with explicit
     `from_row(&rusqlite::Row) -> Result<Self, Error>` and `to_params(&self)
     -> impl rusqlite::Params` boundary; column-name knowledge confined to
     this module.
   - **Insert behavior** keyed by `id` (PhotoId PRIMARY KEY). Wrapped in
     explicit `BEGIN IMMEDIATE; ... COMMIT;` per insert (closes Round 2
     T4 WAL-frame loss). When a file at the same `source_path` has changed
     content (different PhotoId), the new row inserts and the previous
     row's `superseded_at_unix_seconds` is set to `now()`. Both rows are
     retained (audit trail). `INSERT OR IGNORE` semantics on `id`
     conflict (same content, possibly different path → second path is a
     hardlink/duplicate; no insert, log INFO).
   - **Concurrency**: `Arc<Catalog>` shared across rayon workers;
     internally `std::sync::Mutex<rusqlite::Connection>` so all writes
     serialize at the SQLite layer **AND a panicking worker poisons the
     mutex, surfacing `Error::CatalogPoisoned` to all subsequent
     workers** (closes Round 2 T4). The trade-off vs `parking_lot`
     (slightly slower) is explicit and chosen for fail-loud semantics.
     Document in code that the BEGIN IMMEDIATE wrap means panic →
     ROLLBACK on `std::sync::Mutex` poison detection in the next worker.

5. **`ingest_one` function** (in `photohelper-cli::commands::ingest`, NOT
   in `photohelper-core` — closes Round 2 T7):
   - `fn ingest_one(path: &Path, root: &AbsPath, catalog: &Catalog, stats: &IngestStats) -> Result<IngestOutcome, Error>`.
   - `stats` is a small `IngestStats { walked: AtomicU64, ingested:
     AtomicU64, superseded: AtomicU64, already_catalogued: AtomicU64,
     unknown_camera: AtomicU64, no_exif: AtomicU64, mtime_anomalous:
     AtomicU64, skipped_non_raw: AtomicU64, skipped_too_small: AtomicU64,
     errored: AtomicU64 }` updated in-place by both the driver loop and
     `ingest_one` based on the outcome variant + flags.
   - Workflow: canonicalize via `AbsPath::canonicalize_within(root, path)`
     (closes path-escape per R2 T5); compute `PhotoId::derive`; on
     `HashWindowTooSmall` log WARN + return `Ok(SkippedHashWindowTooSmall)`;
     parse EXIF with the chosen reader; clamp mtime to `[1995-01-01,
     now() + 1 day]` (anomalous flag set when clamped); call
     `Catalog::upsert`; map `Catalog::upsert` result to the appropriate
     `IngestOutcome` variant.

6. **Integration test suite** (`crates/photohelper-cli/tests/cli.rs` via
   `assert_cmd` + `tempfile` + `predicates`).
   - See §Test plan for exhaustive list of test rows.

7. **Unit tests** per crate (must run under `cargo test --workspace`).
   - See §Test plan.

8. **Decision artifact**: `docs/decisions/0001-catalog-schema-v1.md` —
   authoritative schema record per DN-005's partial closure. Listed
   here explicitly so it can't be missed at session-end (closes Round 2
   T12). Content: the exact CREATE TABLE SQL above, the index
   rationale, the supersede-row + `camera_known` flag justification,
   and the v1 → v2 migration policy ("session 02 will add tables for
   cull-scores and dup-groups; this is migration 1→2, framework
   introduced then").

### PhotoId derivation (locked spec, intentional trade-off documented)

```text
PhotoId = BLAKE3(
    file_size_u64.to_le_bytes()              // 8 bytes, explicit little-endian
 || clamped_mtime_i64.to_le_bytes()          // 8 bytes, 2s-floored, clamped per below
 || first_64KB                                // exactly min(65536, file_size) bytes
 || last_64KB                                 // exactly min(65536, file_size) bytes; empty if file_size ≤ 65536
)
```

- Endianness explicitly little-endian. Documented in `PhotoId::derive`
  docstring.
- `clamped_mtime_i64` = filesystem mtime in seconds, **clamped to
  `[1995-01-01, now() + 1 day]`** before hashing AND before storing in
  the catalog. Out-of-range mtimes set the `mtime_anomalous` flag and
  log WARN.
- Content + size + clamped-mtime gives enough entropy to distinguish
  Canon R8 burst frames (same camera, near-identical headers, different
  sensor data in `last_64KB`).
- Files where `file_size ≤ 65536`: `first_64KB = whole file`; `last_64KB
  = empty`. The `file_size` prefix still distinguishes a 100-byte file
  from a 200-byte file with the same first 100 bytes.
- Files of size `0` produce `Error::HashWindowTooSmall { path, len: 0 }`
  and are skipped by `ingest_one` with a WARN.

**Intentional trade-off** (closes Round 2 T2): hashing mtime alongside
content means that two byte-identical copies created via tools that
stamp different mtimes (e.g. `cp` with default `--no-preserve` vs `rsync
-t`) will produce **different PhotoIds**. This is deliberate. For a
photo-management tool, the photographer-relevant identity is "this
specific captured frame at this moment" — preserving the original capture
mtime is part of the photo's identity, not noise. The supersede semantics
handle the user-re-saved case (same `source_path`, different PhotoId →
insert new row, mark old as superseded). Users who reorganize archives
with mtime-mangling tools are explicitly outside the v0.1 happy path; a
future session can revisit by exposing a `--hash-policy={content-only |
content-and-mtime}` flag if a real user files for it.

### Path safety contract

- Every `source_path` is canonicalized via `AbsPath::canonicalize_within(
  root, path)` before catalog insert. Constructor rejects: NUL bytes
  (`Error::Io { op: "canonicalize-nul-check" }`); non-existent paths
  (`Error::Io { op: "canonicalize" }`); paths whose canonical form
  escapes the ingestion root (`Error::PathEscapesRoot { path, root }` —
  closes Round 2 T5); symlink loops (surfaces as `io::Error` in `Io { op:
  "canonicalize" }`).
- `--catalog <path>`: missing parent dirs created (logged INFO, with
  best-effort + explicit failing-component error per Round 2 T8).
  Existing-as-directory → `Error::CatalogPathIsDirectory` (fatal).
  Existing non-SQLite file → `Error::CatalogPathNotSqlite` (fatal,
  magic-byte check looks for `"SQLite format 3\0"` first 16 bytes).

### Observability contract

End-of-run summary printed via direct `eprintln!` (NOT `tracing::info!`)
so it survives the tracing-level filter and a `-q` invocation:

```text
walked: <N>, ingested: <M>, superseded: <S>, already-catalogued: <A>,
unknown-camera: <U>, no-exif: <X>, mtime-anomalous: <Y>,
skipped (non-RAW): <K>, skipped (too-small): <T>, errored: <E>
```

The summary line **always prints**. Closes Round 2 T8 silent-quiet hole.

Exit codes:

| Condition | Exit code |
|-----------|----------:|
| All RAWs ingested successfully | 0 |
| `walked > 0 && (ingested + superseded + already_catalogued) == 0` (likely wrong directory) | `64 EX_USAGE` |
| `--strict && (unknown_camera > 0 \|\| errored > 0)` | `1` |
| Stub subcommand (`cull`, `develop`, `export`, `run`, `models`, `camera`) | `69 EX_UNAVAILABLE` |
| Fatal error (catalog open, lock-budget-exhausted, schema-too-new, mutex-poisoned, path-escape, IO at root, mkdir-p partial failure) | `74 EX_IOERR` |
| `clap` parse failure (invalid flag, out-of-range `--threads`) | `2` (clap default) |

Codes 64 / 69 / 74 chosen per sysexits.h conventions; code 2 is reserved
for `clap`-owned parse errors so the test plan can distinguish them
(closes Round 2 T8 collision + T10 semantic mismatch).

Heartbeat: a dedicated thread fires `tracing::info!("walked {N},
ingested {M}, in-flight {P}")` every 10 seconds during the ingest run,
then exits on driver completion. Distinguishes "still working" from
"stuck on a slow EXIF parse" for the user (closes Round 2 T8).

Tracing-level table (defaults; `RUST_LOG` overrides):

| Event | Level |
|-------|-------|
| EXIF parse failure on a single file | `WARN` |
| EXIF parse succeeded but yielded zero fields | `WARN` (closes Round 2 T8 silent-empty-EXIF) |
| Unknown camera first-seen (per make/model pair) | `WARN` |
| Unknown camera subsequent occurrences (same make/model) | `INFO` |
| Skipped non-RAW extension | `INFO` |
| Skipped (hash window too small) | `WARN` |
| File at same path with different content (superseded) | `INFO` |
| File at different path with same content (hardlink / duplicate) | `INFO` |
| Per-photo successful ingest | `DEBUG` |
| `mtime` clamped (anomalous) | `WARN` |
| File-lock retry (per attempt in the 5×500ms budget) | `WARN` |
| `wal_checkpoint(TRUNCATE)` recovered frames at open | `WARN` |
| Heartbeat | `INFO` |

Default `-v` count = `0` surfaces `WARN` and above to stderr. `-v` →
`INFO`, `-vv` → `DEBUG`, `-vvv` → `TRACE`. `-q` mutes everything below
`ERROR` *for tracing events*; the summary line is `eprintln!` and always
prints.

`mtime` validation: filesystem mtime outside `[1995-01-01, now() + 1
day]` is clamped to the nearest boundary, `mtime_anomalous` column set
to 1, summary slot `mtime-anomalous: <Y>` reflects the count, and a
WARN is logged per occurrence.

### Concurrency

- `rayon`'s default thread pool sized via `--threads` (default
  `num_cpus`, clap-validated to 1..=1024).
- `walkdir::WalkDir` iterator → `par_bridge` → workers call `ingest_one`.
- `Arc<Catalog>` shared across workers; internally
  `std::sync::Mutex<rusqlite::Connection>`. A worker panic inside the
  insert path will poison the mutex; subsequent workers will surface
  `Error::CatalogPoisoned`. Each insert is wrapped in
  `BEGIN IMMEDIATE; ...; COMMIT;` so a panicked transaction rolls back
  cleanly (closes Round 2 T4).
- No `crossbeam-channel`, no dedicated writer thread, no
  `CancellationToken` this session.
- A separate heartbeat thread (per §Observability) — owned by the driver,
  joined at end-of-walk.

### Out of scope (8 items — deferrals; anything dropped here goes to TECH-DEBT.md only if it leaves a stop-gap behind)

1. LibRaw FFI or any RAW pixel decode (session 02).
2. ONNX, `ort`, any AI model or model registry (sessions 03+).
3. XMP sidecar read/write — neither `crs:` nor `ph:` namespaces (sessions 04+).
4. `develop`, `export`, watermark, JPEG encode (sessions 04–05).
5. `photohelper-cameras` per-ISO noise model + color matrix bodies
   (`Err(Error::CameraProfileNotImplemented)` stubs only; session 02
   fills them).
6. Windows build verification (v0.1 target is Linux + macOS; Windows
   in v0.2).
7. `git-lfs` fixture CR3s (session 02 introduces them; this session
   uses synthesized 64+KB blobs in `tempfile`).
8. Migration framework (single-table v1 schema needs no framework;
   framework lands with migration 1→2 in session 02 per DN-005).

### Test plan

Every test asserts a concrete observable per `docs/testing-standards.md`
(repo-local). No `assert!(true)`, no "didn't panic", no `assert!
(result.is_ok())` without checking the inner value.

| # | Area | Test type | Concrete assertion |
|--:|------|-----------|--------------------|
| 1 | `PhotoId::derive` stability | unit (`core::model`) | same file twice → identical PhotoId bytes |
| 2 | `PhotoId::derive` distinguishability | unit | two files with identical first/last 64KB but different `file_size` → different PhotoIds |
| 3 | `PhotoId::derive` small files | unit | 100-byte file: succeeds (head = whole file, tail = empty); 0-byte file: returns `Error::HashWindowTooSmall { len: 0 }` |
| 4 | `PhotoId::derive` cross-platform endian | unit | inject fixed `file_size` + `clamped_mtime` + content; assert exact BLAKE3 output bytes |
| 5 | `PhotoId::Display` render | unit | 43-char base64url-nopad output; round-trips via `from_db_bytes` |
| 6 | `PhotoId::from_db_bytes` visibility | compile-test (in `core` test module) | `PhotoId::from_db_bytes` is not callable from outside `photohelper-core` (compile-fail test in `tests/ui/` or `pub(crate)` enforcement test) |
| 7 | `AbsPath::canonicalize` rejections | unit | NUL byte → `Error::Io { op: "canonicalize-nul-check" }`; non-existent → `Error::Io { op: "canonicalize" }` |
| 8 | `AbsPath::canonicalize_within` escape | unit | tempdir `/foo`; symlink `/foo/evil -> /etc/passwd`; `canonicalize_within("/foo".into(), "/foo/evil")` → `Error::PathEscapesRoot` |
| 9 | `Catalog::open` magic-byte check | unit | text file at catalog path → `Error::CatalogPathNotSqlite`; directory at catalog path → `Error::CatalogPathIsDirectory` |
| 10 | `Catalog::open` schema-version check | unit | DB with `PRAGMA user_version = 2` → `Error::CatalogSchemaTooNew { found: 2, expected: 1 }`; version = 0 → init runs; version = 1 → OK |
| 11 | `Catalog::open` schema init idempotency | unit | running init twice → identical schema; `user_version` stable at 1 |
| 12 | `Catalog::open` schema init transactional | unit | simulate failure mid-init via injectable hook → next open sees `user_version = 0`, re-runs init successfully (closes Round 2 T12) |
| 13 | `Catalog::open` file-lock cross-process | integration | spawn `photohelper ingest <tempdir>` twice via `std::process::Command`, second invocation completes with `Error::CatalogLockHeld` after the 5×500ms retry budget (closes Round 2 T11) |
| 14 | `Catalog::open` wal_checkpoint warn | integration | open DB; kill mid-ingest via SIGKILL; reopen; assert stderr contains "previous shutdown was unclean; recovered" (closes Round 2 T4) |
| 15 | `Catalog` insert: new content same path | unit | inserting PhotoId-B at a `source_path` that has PhotoId-A → both rows exist, A's `superseded_at_unix_seconds` is set |
| 16 | `Catalog` insert: same content same path | unit | second insert of identical PhotoId → INSERT OR IGNORE, log INFO, row count unchanged |
| 17 | `Catalog` insert: same content different path (hardlink) | integration | hardlink the same file to two paths; ingest; assert one row in `photos`; assert stderr contains INFO about duplicate (closes Round 2 T11) |
| 18 | `Catalog` mutex poisoning | unit | force a worker panic while holding the mutex (e.g. via `std::panic::catch_unwind` + a poisoned helper); next insert returns `Error::CatalogPoisoned` |
| 19 | `Catalog` insert transactional | unit | inject failure between BEGIN IMMEDIATE and COMMIT; assert no row visible after rollback |
| 20 | `Catalog` Send+Sync compile-time assertion | compile-test | the `assert_send_sync::<Arc<Catalog>>()` test in `catalog::tests` compiles (closes Round 2 T6) |
| 21 | `CameraRegistry::for_exif` known body | unit | `("Canon", "Canon EOS R8")` → `Some(profile)` with `id() == CameraId::Known(CanonR8)` |
| 22 | `CameraRegistry::for_exif` normalization | unit | trailing NUL bytes + surrounding whitespace stripped before lookup |
| 23 | `CameraRegistry::for_exif` unknown body | unit | `("Acme", "X1")` → `None` (NOT a panic) |
| 24 | `KnownCamera::slug` + `from_slug` round-trip | unit | `KnownCamera::CanonR8.slug() == "canon-r8"`; `from_slug("canon-r8") == Some(CanonR8)`; `from_slug("unknown") == None` |
| 25 | `CameraProfile` stub methods | unit | `CanonR8::base_iso()` → `Err(Error::CameraProfileNotImplemented { method: "base_iso", camera_id: ... })` — NOT a panic |
| 26 | `ExifOrientation::from_tag` / `to_tag` round-trip | unit | for N in 1..=8: `from_tag(N).unwrap().to_tag() == N`; tag 0 and 9 → `Error::Exif` |
| 27 | `ExifOrientation` variant correctness | unit | tag 5 (`MirrorHorizontalRotate90Cw`) produces the transpose mapping (closes Round 2 T9 correctness bug) |
| 28 | `mtime` clamp function | unit | mtime = -1 → clamped to 1995-01-01 + anomalous=true; mtime = now() + 7 days → clamped to now()+1d + anomalous=true; in-range → unchanged + anomalous=false (closes Round 2 T11) |
| 29 | `ingest_one` happy path | unit (`cli::commands::ingest`) | `.cr3` file → `IngestOutcome::Inserted { camera_known: true, ... }`; catalog row matches |
| 30 | `ingest_one` non-RAW filter | unit | `.jpg` file → `IngestOutcome::SkippedNonRaw`; row count unchanged |
| 31 | `ingest_one` 0-byte file | unit | 0-byte `.cr3` → `IngestOutcome::SkippedHashWindowTooSmall`; row count unchanged (closes Round 2 T11) |
| 32 | `ingest` CLI happy path | integration | tempdir with `a.cr3` + `b.jpg`; assert exit 0; stderr contains `walked: 2`, `ingested: 1`, `skipped (non-RAW): 1`; exactly one row in `photos` with `source_path` ending `a.cr3`, `file_size` matches fixture, `id` is 32 bytes |
| 33 | `ingest` CLI summary survives `-q` | integration | `-q` flag set; assert end-of-run summary line still appears in stderr (closes Round 2 T8) |
| 34 | `ingest` CLI per-photo `.with_context()` | integration | force a per-photo read error; stderr contains `"ingesting "` followed by the path (closes Round 2 T11) |
| 35 | `ingest` CLI idempotency | integration | run twice; second stderr contains `already-catalogued: 1, ingested: 0`; row count stays at 1 |
| 36 | `ingest` CLI content change | integration | ingest a file; rewrite with different bytes; ingest again; two rows for the same `source_path`; first has `superseded_at_unix_seconds` set |
| 37 | `ingest` CLI empty / wrong directory | integration | dir of only `.jpg` files → exit `64`; stderr contains `ingested: 0` |
| 38 | `ingest` CLI truly empty directory | integration | empty tempdir → exit `0`; stderr contains `walked: 0` (closes Round 2 T11) |
| 39 | `ingest` CLI `--strict` with unknown camera | integration | synthesized fake-EXIF file → without `--strict`: exit 0; with `--strict`: exit 1 |
| 40 | `ingest` CLI walker edges | integration (consolidated) | tempdir containing: hidden `.foo.cr3` (cataloged); symlink loop (handled, no infinite recursion); deeply nested empty subdir (no error); non-UTF-8 path (handled). Assert no panic + sensible row count. |
| 41 | `ingest` CLI mtime-anomalous summary slot | integration | synthesize file with mtime=0 → stderr contains `mtime-anomalous: 1` (closes Round 2 T11) |
| 42 | `ingest` CLI tracing per-event-class mapping | integration (parameterized over event classes) | EXIF failure → stderr contains WARN; two unknown-camera same-make/model → stderr contains one WARN + one INFO; mtime clamp → stderr contains WARN. All at default `-v=0` (closes Round 2 T11) |
| 43 | `ingest` CLI fatal exit codes | integration (parameterized) | catalog-path-is-directory → exit `74`; schema-too-new → exit `74`; lock-budget-exhausted → exit `74` (closes Round 2 T11) |
| 44 | `ingest` CLI usage exit code | integration | `--threads 0` → exit `2` (clap default); `--threads 2000` → exit `2`; `--catalog /some/dir/` (existing dir) → exit `74` (fatal, distinct from clap) |
| 45 | Stub subcommands | integration (parameterized over `cull`/`develop`/`export`/`run`/`models`/`camera`) | each → exit `69 EX_UNAVAILABLE`; stderr contains `not yet implemented` (closes Round 2 T10) |
| 46 | CLI `--verbose` count mapping | integration | `-v` enables INFO (verify INFO event in stderr); `-vv` enables DEBUG; `-q` mutes WARN tracing events but NOT the summary |
| 47 | CLI `--catalog` override | integration | `--catalog /tmp/explicit.db` → DB created at that exact path, NOT at `<input>/.photohelper/catalog.db` |
| 48 | Heartbeat thread | integration | ingest a tempdir with synthesized slow EXIF parses; assert stderr contains at least one heartbeat line during a >10s run |
| 49 | Workspace gates | `just ci` | fmt-check, clippy `-D warnings`, test, audit, verify-state all green |

**49 tests total** (exact count, closes Round 2 T11 "approximately" looseness).

### Checkpoints firing this session (Cadence A)

| Checkpoint | When | Tier | Agents | Double-review? |
|------------|------|------|--------|----------------|
| Session start | done | Tier 1 | 1 — `general-purpose` (alignment) | No |
| **Plan-review** | this checkpoint; Rounds 1, 2 complete; Round 3 pending after this v3 commit | Tier 5 | **Full 8** per round | **Yes — Round 3 triggered by R2's CRITICALs** |
| Sub-component review | invoked only if a module/file crosses the trigger from `docs/quality-assurance.md § Sub-component review protocol` (first non-scaffold public API; file > ~300 LoC non-test). Realistic this session: `photohelper-catalog` first public API; `photohelper-cli::commands::ingest` driver if it grows past 300 LoC | Tier 4 | 3–5 per boundary | Yes |
| **Session end** | before commit + push | Tier 5 | **Full 8** | **Yes** |

### Expected discovery items

- **Partially resolves DN-005** (catalog schema): lands v1 minimal schema
  slice + `docs/decisions/0001-catalog-schema-v1.md`. DN-005 stays open
  — session 02 still owes the dup-group and culling-score tables.
- **Potential DN-006** (EXIF reader for CR3): default is the chosen
  reader pinned in §Dependencies; if implementation surfaces a CR3
  ISO-BMFF container-parsing gap, file DN-006 and revisit in session 02
  when LibRaw provides an alternate source. **Pre-flight check** (small
  synthesized CR3 fixture against the reader) runs at the start of
  implementation before any production code is written.
- **Potential DN-007**: `rusqlite` static-link binary size impact
  (~1.5 MB). If this becomes a release-engineering concern, file as DN.
- **Potential DN-008**: `std::sync::Mutex<Connection>` write
  serialization throughput vs the deferred dedicated-writer-thread
  pattern. If profiling on 10k+ photo runs shows the mutex is the
  bottleneck, file DN and revisit.
- **Potential DN-009**: proptest / quickcheck coverage for PhotoId
  collision space. v0.1 uses example-based tests; if a later session
  debates the hash input shape, file DN.

### Tech-debt entries created or touched this session (preview — finalized at session-end)

- No new TDs anticipated for the planned scope; every item above is a
  real deliverable, not a stop-gap.
- TD-001 (action pinning) not touched — gated on "before first external
  contributor or first release," neither triggered yet.
- Session-end housekeeping additions (per Round 2 T11 + T12): update
  `SESSION-STATE.md` "Component progress" to list the 8th crate
  `photohelper-catalog`; update `Cargo.toml [workspace] members` to
  include it; update `HANDOFF_REPORT.md` checkpoint 1.

### Dependencies introduced this session

All deps declared in `[workspace.dependencies]` at the root `Cargo.toml`,
then per-crate via `dependencies = { workspace = true }`. Versions are
caret-ranged unless noted. Versions verified against crates.io at plan
authoring time (per Round 2 T1 health check).

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
| `base64` | 0.22 | core | (none) — for `base64::engine::general_purpose::URL_SAFE_NO_PAD` PhotoId render |
| EXIF reader | TBD (pre-flight verdict; default `kamadak-exif 0.6`, fallback noted in DN-006) | core | (none) |
| `time` | 0.3 | core, catalog | `macros` |
| `rusqlite` | 0.40 (current line; bumped from v2's 0.32 per Round 2 T1) | catalog | `bundled` — static SQLite amalgamation |
| `fs4` | 1 (replaced v2's stale `fs2 0.4` per Round 2 T1) | catalog | `sync` |
| `num_cpus` | 1 | cli | (none) — `--threads` default |
| `assert_cmd` | 2 | cli (dev-dep) | (none) |
| `predicates` | 3 | cli (dev-dep) | (none) |
| `tempfile` | 3 | cli, catalog (dev-dep) | (none) |
| `static_assertions` | 1 | catalog (dev-dep, optional) | (none) — for compile-time `assert_send_sync::<Arc<Catalog>>()` |

`cargo tree --workspace --depth 1` and `cargo audit` run at session-end;
if the transitive crate count exceeds 120 OR `cargo audit` flags any
advisory, file a TD.

**Note on `parking_lot` removal**: v2 listed `parking_lot 0.12` for
`Mutex<Connection>`; v3 uses `std::sync::Mutex` instead (panic-poison
semantics per Round 2 T4). `parking_lot` removed from the dep list.

### Non-goals (clarifying boundary)

- This session does **not** optimize ingest throughput. `std::sync::Mutex
  <Connection>` is simple, correct, and fail-loud on panic; if profiling
  shows the mutex is the bottleneck on 10k+ photo runs, that's session
  02+ (DN-008 reserved).
- This session does **not** start AI work. AI features need decoded
  pixels which require LibRaw (session 02).
- This session does **not** implement cooperative cancellation / SIGINT
  handling. Lands in the session that genuinely needs to interrupt a
  long-running develop/export run.
- This session does **not** ship a benchmark suite. `criterion` benches
  arrive when there's a real performance question to answer.

---

*Implementation notes (pseudocode, exact API signatures, sub-component
review plan) will be appended below this line **after** plan-review
Round 3 + remediation are clean. Until then, the contract above is the
load-bearing artifact.*
