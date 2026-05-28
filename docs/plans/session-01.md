# Session 01 — `cli-skeleton-and-ingest`

> **Branch**: `session-01/cli-skeleton-and-ingest`
> **Started**: 2026-05-27
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v1 (initial) → v2 (post R1) → v3 (post R2) → v4 (post R3)
> → **v5 (this revision; post R4 focused — see
> `docs/code-reviews/session-01-plan-round4.md`)**

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the thinnest end-to-end slice that proves the workspace architecture: a
real `photohelper ingest <path>` subcommand that walks a directory of files,
recognises RAW extensions, reads EXIF, derives a content-addressed `PhotoId`,
and writes catalog rows to a SQLite database at
`<path>/.photohelper/catalog.db`. All other subcommands (`cull`, `develop`,
`export`, `run`, `models`, `camera`) ship as `clap` stubs that exit with
`69 EX_UNAVAILABLE`.

This session intentionally does **not** decode any RAW pixels — that lands in
session 02 with the LibRaw FFI. EXIF parsing of CR3 containers via
`kamadak-exif 0.6` is sufficient to populate the catalog. If pre-flight
verifies kamadak-exif cannot parse CR3 ISO-BMFF containers, session 01 ships
with `make`/`model`/`capture_time`/`width`/`height`/`exif_orientation`
columns NULL for CR3 rows (degraded but functional); full EXIF arrives in
session 02 via LibRaw. See DN-006.

### Scope expansions vs. the bootstrap plan (with inline "closes R{N}.T{M}" annotations)

| Addition | Justification |
|----------|---------------|
| `photohelper-catalog` as an 8th workspace crate | Catalog persistence is a distinct concern from the domain model (R1.T3). |
| `--catalog <path>` flag | Operational ergonomics — read-only photo trees. |
| `--strict` flag on `ingest` | Pairs with §Observability; v0.1 audience is scripted batch processing. |
| `--catalog-lock-timeout-seconds <N>` flag (default 60) | Configurable lock wait so CI scenarios can fail fast while real ingests survive concurrent runs (closes R3.T8). |
| Magic-byte check on existing `--catalog` target | Path-safety (R1.T4). |
| `tracing` level table pinned in the plan | Closes silent-EXIF-failure failure mode (R1.T5). |
| `ExifOrientation` full 8-variant enum with canonical EXIF names (`Transpose`/`Transverse` at slots 5/7) | Preserves info for session-05 export rotation (R1.T8); closes R3.T9 naming clarity. |
| Path-escape check via `AbsPath::canonicalize_within` | Closes R2.T5. |
| Compile-time `assert_send_sync::<Arc<Catalog>>()` | Closes R2.T6. |

**Removed in v2**: `Pipeline` trait, `PipelineCtx`, `Sidecar` enum,
`CancellationToken` (R1.T2 YAGNI).

**v3 → v4 must-fix adjustments** (per R3 findings):
- `fs4::FileExt::try_lock` (no `_exclusive` suffix — R3.T1 REGRESSION;
  v3 used the `fs2` method name with the `fs4` crate, code wouldn't compile).
- `CatalogReconstructionToken` **removed** (R3.T2). New approach: `pub(crate)
  fn PhotoId::from_db_bytes` stays inside `core`; catalog row reconstruction
  goes through `core::catalog_glue::photo_id_from_row_bytes(...) -> PhotoId`
  — a `pub fn` in `core` that `photohelper-catalog::PhotoRow::from_row`
  calls. No new type; no DAG cycle; forgery surface confined to one
  named-after-purpose function inside `core` (any caller still has to
  spell the function name and import from `catalog_glue`, signalling
  intent).
- Heartbeat prints via direct `eprintln!` (not `tracing::info!`) so it
  appears at default verbosity (closes R3.T3); implemented as a thread
  spawned via `std::thread::spawn` with its `JoinHandle` retained (NOT
  joined; never blocks the driver) for `handle.is_finished()` status
  checks. The thread reads `Arc<AtomicBool>` stop flag set by the driver
  at end-of-walk (closes R3.T4; R4.T1 terminology clarification — earlier
  drafts said "detached" which conflicted with retaining the handle). Driver checks `handle.is_finished()` at end
  and logs WARN if the heartbeat died early. The `AtomicBool` is not a
  general cancellation primitive — it's a one-shot shutdown signal,
  scoped to this thread; the original "no `CancellationToken`" intent
  (no general cooperative-cancellation infrastructure) holds.
- `Catalog` insert path explicitly: lock → `BEGIN IMMEDIATE;` → execute
  → `COMMIT;`. **On `std::sync::Mutex::PoisonError`**: recover the
  connection via `.into_inner()`, issue `ROLLBACK;` (ignore errors —
  may not be in a transaction), then return `Error::CatalogPoisoned`.
  Closes R3.T5 (without explicit ROLLBACK the next `BEGIN IMMEDIATE`
  returns `SQLITE_ERROR`).
- Schema: `camera_known INTEGER` column **dropped** (closes R3.T6
  redundancy). The canonical predicate is `camera_slug IS NOT NULL`.
- `PhotoId` mtime clamp ceiling pinned to **`2100-01-01`** (static)
  rather than `now() + 1 day` (closes R3.T7 — run-independent hash input).
  Lower bound stays at `1995-01-01`.
- Lock retry budget: default 60 seconds (12 attempts × 5s) instead of
  v3's 2.5s; configurable via `--catalog-lock-timeout-seconds` (closes
  R3.T8).
- `ExifOrientation` slot 5 → `Transpose`; slot 7 → `Transverse` (EXIF
  canonical names; closes R3.T9 ambiguity).
- `IngestOutcome::Inserted` payload simplified to `Inserted(PhotoId)`
  only — no boolean flags (closes R3.T10 "enum + atomics worst of both
  worlds"). The driver reads the written row's columns (`camera_slug IS
  NOT NULL`, `mtime_anomalous`, `capture_time_unix_seconds IS NOT NULL`)
  to increment the right `IngestStats` atomics. Single source of truth
  per fact.
- EXIF reader **committed** to `kamadak-exif 0.6` (no more "TBD" in the
  deliverable contract — closes R3.T11). DN-006 fallback documented.

### Deliverables (when the PR merges, the following will exist)

1. **`photohelper-cli` (binary `photohelper`)**
   - `clap` v4 derive API with subcommand handlers: `ingest`, `cull`,
     `develop`, `export`, `run`, `models`, `camera`. Each non-`ingest`
     handler will print `"not yet implemented (planned for session NN)"`
     to stderr and exit `69 EX_UNAVAILABLE`.
   - Global flags: `--verbose/-v` (repeatable; sets `tracing` level per
     §Observability), `--quiet/-q` (suppresses non-error tracing output
     but NOT the end-of-run summary line or heartbeat), `--threads <N>`
     (default = `num_cpus`; `value_parser = clap::value_parser!(u32).range(1..=1024)`),
     `--catalog <path>` (default = `<input>/.photohelper/catalog.db`),
     `--catalog-lock-timeout-seconds <N>` (default `60`;
     `value_parser = clap::value_parser!(u32).range(1..=3600)` — closes
     R4.T2: rejects `0` and silly-large values; min 1s, max 1hr),
     `--no-color`.
   - `ingest` flags: `--recursive/-r` (default `true`), `--strict`
     (default `false`).
   - `tracing-subscriber` initialized with compact `fmt` + `EnvFilter`
     (honors `RUST_LOG`); `-v` count maps per §Observability.
   - `indicatif` spinner (not progress bar — `par_bridge` is lazy).
     Final summary via direct `eprintln!` (bypasses tracing filter).
   - Heartbeat: a `std::thread::spawn`-ed thread reads `Arc<AtomicBool>`
     stop flag; every 10 seconds writes via
     `eprintln!("[heartbeat] walked {N}, ingested {M}, in-flight {P}")`.
     Driver retains the `JoinHandle` (NEVER calls `.join()` — would block),
     sets the stop flag at end-of-walk, and checks `handle.is_finished()`
     — logs WARN if heartbeat died before the walk completed (closes
     R4.T1 terminology bug — earlier drafts called this "detached" but a
     truly detached thread has no `JoinHandle`).
   - `ingest` driver in `photohelper-cli::commands::ingest`: walks via
     `walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok)`,
     filters RAW extensions (lowercased: `.cr3`, `.cr2`, `.arw`, `.nef`,
     `.raf`, `.orf`, `.rw2`, `.dng`), `par_bridge` into rayon, each worker
     calls `ingest_one(path: &Path, root: &AbsPath, catalog: &Catalog,
     stats: &IngestStats) -> Result<IngestOutcome, Error>` defined in
     the same module. `IngestStats` is `pub(crate)` inside
     `cli::commands::ingest`.

2. **`photohelper-core` (lib)** — domain types only. No persistence, no
   pipeline trait, no `ingest_one`.

   - `module model`:
     - `PhotoId(/* private */ [u8; 32])` content-derived per §PhotoId
       derivation. Constructors: `pub fn derive(path: &Path) ->
       Result<Self, Error>` (canonical) and
       `pub(crate) fn from_db_bytes(raw: [u8; 32]) -> Self` (catalog
       reconstruction; only callable from inside `photohelper-core`).
       `Display` renders 43-char `base64url` no-pad (via
       `base64::engine::general_purpose::URL_SAFE_NO_PAD`).
     - `Photo`: fields private; built via `Photo::from_filesystem(
       canonical: AbsPath, file_size: u64, clamped_mtime: i64,
       exif: ExifMetadata) -> Result<Self, Error>` enforcing
       `file_size > 0`. Accessors return references.
     - `AbsPath`: newtype over `PathBuf`. `AbsPath::canonicalize(path:
       impl AsRef<Path>) -> Result<Self, Error>` rejects NUL bytes
       (`Error::Io { op: "canonicalize-nul-check" }`) and non-existent
       paths (`Error::Io { op: "canonicalize" }`). `impl AsRef<Path>`;
       `pub fn as_path(&self) -> &Path`. Plus `AbsPath::canonicalize_within(
       root: &AbsPath, path: impl AsRef<Path>) -> Result<Self, Error>`
       rejecting `!canonical.starts_with(root.as_path())` with
       `Error::PathEscapesRoot { path, root }`.
     - `CameraId`: `enum CameraId { Known(KnownCamera), Unknown { make:
       String, model: String } }`.
     - `KnownCamera`: `#[non_exhaustive] enum KnownCamera { CanonR8 }`.
       `pub fn slug(&self) -> &'static str` (`CanonR8 => "canon-r8"`);
       `pub fn from_slug(slug: &str) -> Option<Self>`.
     - `ExifOrientation`: `#[non_exhaustive] enum ExifOrientation`
       matching EXIF canonical tag 1..=8 with the canonical names:
       ```
       1 = Normal
       2 = MirrorHorizontal
       3 = Rotate180
       4 = MirrorVertical
       5 = Transpose                  (mirror H + rotate 90 CW)
       6 = Rotate90Cw
       7 = Transverse                 (mirror H + rotate 90 CCW)
       8 = Rotate90Ccw
       ```
       `pub fn from_tag(tag: i64) -> Result<Self, Error>` returns
       `Error::Exif` outside 1..=8. `pub fn to_tag(&self) -> i64`.
       Round-trip test asserts `from_tag(N).unwrap().to_tag() == N` for
       N ∈ 1..=8.
     - `Aspect`: `#[non_exhaustive] enum Aspect { Landscape, Portrait,
       Square }` (closes R4.T5 — every other domain enum is
       `#[non_exhaustive]`; this one was the lone exception). `Photo::aspect
       (&self) -> Aspect` derived from `(width, height, exif_orientation)`.
     - `ExifMetadata` (closes R4.T4 — named in `Photo::from_filesystem`
       but never spec'd in earlier revisions):
       ```rust
       pub struct ExifMetadata {
           pub make: Option<String>,
           pub model: Option<String>,
           pub capture_time_unix_seconds: Option<i64>,
           pub width: Option<u32>,
           pub height: Option<u32>,
           pub orientation: Option<ExifOrientation>,
       }
       impl ExifMetadata {
           /// True iff every field is None — the signal `ingest_one` uses
           /// to route to `IngestOutcome::NoExifFields`.
           pub fn is_empty(&self) -> bool { /* all None */ }
       }
       ```
     - `IngestOutcome`: `#[non_exhaustive] enum IngestOutcome {
       Inserted(PhotoId), SupersededPrevious { new: PhotoId, old: PhotoId },
       AlreadyCatalogued(PhotoId), SkippedNonRaw, SkippedHashWindowTooSmall,
       NoExifFields }`. Per R3.T10: no boolean flags; the driver computes
       summary-tally booleans (`camera_known`, `no_exif_fields`,
       `mtime_anomalous`) by reading the written catalog row's columns.

   - `module catalog_glue` (new, per R3.T2 fix): `pub fn
     photo_id_from_row_bytes(raw: [u8; 32]) -> PhotoId` is the single
     `pub` factory that calls the `pub(crate) PhotoId::from_db_bytes`.
     Catalog calls `core::catalog_glue::photo_id_from_row_bytes` from
     `PhotoRow::from_row`. Forgery surface = one named function inside
     `core`; any other caller must spell it out — strong intent signal.

   - `module error`: `#[non_exhaustive] enum Error` (derive
     `thiserror::Error`, `Debug`). **No `#[from]` derives**; every site
     uses `.map_err`. Variants:
     - `Io { path: PathBuf, op: &'static str, source: io::Error }` —
       absorbs canonicalize, NUL-check, ingest-time IO. Op tags:
       `"canonicalize"`, `"canonicalize-nul-check"`, `"read-prefix"`,
       `"stat"`, `"mkdir-p"`.
     - `Exif { path: PathBuf, source: Box<dyn std::error::Error + Send + Sync> }`.
     - `HashWindowTooSmall { path: PathBuf, len: u64 }`.
     - `CatalogOpen { path: PathBuf, source: rusqlite::Error }`.
     - `CatalogInsert { photo_id: PhotoId, source: rusqlite::Error }`.
     - `CatalogPathIsDirectory { path: PathBuf }`.
     - `CatalogPathNotSqlite { path: PathBuf }`.
     - `CatalogLockHeld { path: PathBuf, attempts: u32, total_ms: u64 }`.
     - `CatalogSchemaTooNew { found: i64, expected: i64 }` — CLI boundary
       wraps with `.context("update photohelper or use --catalog with a
       fresh path")`.
     - `CatalogPoisoned { path: PathBuf }` — surfaced after `PoisonError`
       recovery + `ROLLBACK`.
     - `PathEscapesRoot { path: PathBuf, root: PathBuf }`.
     - `CameraProfileNotImplemented { method: &'static str, camera_id: CameraId }`.

   Library returns `Result<T, Error>`. CLI boundary uses `anyhow::Result`
   with mandatory `.with_context()` at per-photo loop (`|| format!(
   "ingesting {}", path.display())`) and catalog-open call site
   (`|| format!("opening catalog at {}", catalog_path.display())`).

3. **`photohelper-cameras` (lib)**
   - `CameraProfile` trait; stub methods (`base_iso`, `sensor_layout`,
     `color_matrix_d65`, `noise_model`) return `Err(Error::
     CameraProfileNotImplemented { method, camera_id })` — never
     `todo!()`/`unimplemented!()`/`panic!()`.
   - `CanonR8` implementing `CameraProfile` (EXIF identification only).
   - `CameraRegistry` with `fn for_exif(&self, make: &str, model: &str)
     -> Option<Arc<dyn CameraProfile>>`. Input normalization: trims
     whitespace and trailing NUL bytes; case-sensitive on `model`.

4. **`photohelper-catalog` (lib, 8th workspace crate)** — SQLite-backed
   catalog persistence.

   - **`Catalog` struct** (3 fields, all Send+Sync):
     ```
     pub struct Catalog {
         conn: std::sync::Mutex<rusqlite::Connection>,
         _lock_handle: std::fs::File,           // held for Catalog lifetime
         canonical_path: AbsPath,
     }
     ```
     Compile-time assertion in test module:
     `const _: fn() = || { fn assert_send_sync<T: Send + Sync>() {}
     assert_send_sync::<Arc<Catalog>>(); };`.

   - **`Catalog::open(catalog_path: impl AsRef<Path>, lock_timeout_seconds:
     u32) -> Result<Self, Error>`** sequence:
     1. Compute `lock_path = <parent>/.photohelper/catalog.db.lock`.
     2. Create `<parent>/.photohelper/` (logged INFO); failure →
        `Error::Io { op: "mkdir-p" }` naming the failing component.
     3. Open `lock_path` (create if missing).
     4. Acquire exclusive lock via `fs4::FileExt::try_lock()` in a loop:
        attempt every 5 seconds for `lock_timeout_seconds` total
        (default 12 attempts × 5s = 60s); WARN per retry; on budget
        exhaustion return `Error::CatalogLockHeld { path, attempts,
        total_ms }`. (Note: 12 WARNs over 60s is acceptable for the
        concurrent-ingest edge case; `-q` suppresses them — closes
        R4.T7.)
     5. Verify existing catalog file (if any): existing-as-directory →
        `Error::CatalogPathIsDirectory`; existing non-empty file whose
        first 16 bytes are NOT `"SQLite format 3\0"` →
        `Error::CatalogPathNotSqlite`.
     6. Open `rusqlite::Connection`; failure → `Error::CatalogOpen`.
     7. Set PRAGMAs: `journal_mode = WAL`, `synchronous = NORMAL`,
        `busy_timeout = 5000`.
     8. Read `PRAGMA user_version`. If `0`, run init (transactional). If
        `1`, OK. If `> 1`, `Error::CatalogSchemaTooNew { found, expected: 1 }`.
     9. Run `PRAGMA wal_checkpoint(TRUNCATE)`; if recovered frames > 0,
        WARN "previous shutdown was unclean; recovered N WAL frames."
     10. Construct `Catalog { conn, _lock_handle, canonical_path }`.

   - **Schema init transactional**:
     ```sql
     BEGIN IMMEDIATE;
       CREATE TABLE IF NOT EXISTS photos (
         id BLOB PRIMARY KEY,
         source_path TEXT NOT NULL,
         file_size INTEGER NOT NULL,
         mtime_unix_seconds INTEGER NOT NULL,
         mtime_anomalous INTEGER NOT NULL DEFAULT 0,
         make TEXT,
         model TEXT,
         camera_slug TEXT,
         capture_time_unix_seconds INTEGER,
         width INTEGER,
         height INTEGER,
         exif_orientation INTEGER,
         ingested_at_unix_seconds INTEGER NOT NULL,
         superseded_at_unix_seconds INTEGER
       );
       CREATE INDEX IF NOT EXISTS idx_photos_source_path ON photos(source_path);
       CREATE INDEX IF NOT EXISTS idx_photos_camera_slug ON photos(camera_slug);
       PRAGMA user_version = 1;
     COMMIT;
     ```
     `camera_known` column **dropped** vs v3 — the predicate is
     `camera_slug IS NOT NULL`. Schema documented authoritatively in
     `docs/decisions/0001-catalog-schema-v1.md` (Deliverable 8).

   - **`PhotoRow` struct** in `photohelper-catalog::row` with explicit
     `from_row(&Row) -> Result<Self, Error>` (calls
     `core::catalog_glue::photo_id_from_row_bytes`) and `to_params(&self)
     -> impl Params`. Column-name knowledge confined here.

   - **Insert** keyed by `id` PRIMARY KEY. **Each insert is wrapped in
     `BEGIN IMMEDIATE; ...; COMMIT;`**. On `std::sync::Mutex::PoisonError`
     in a subsequent caller: recover with `.into_inner()`, issue
     `ROLLBACK;` (ignore errors — txn may not be open), return
     `Error::CatalogPoisoned { path: self.canonical_path.as_path().to_path_buf() }`.
     Catalog is treated as permanently dead — subsequent inserts also
     return `Error::CatalogPoisoned`.

     When `source_path` matches an existing row but `id` (PhotoId)
     differs (content changed): new row inserts, old row's
     `superseded_at_unix_seconds` set to `now()`. Both retained (audit).
     `INSERT OR IGNORE` on identical `id` (same content, possibly
     different path → hardlink/duplicate import) → no insert, INFO log.

   - **Concurrency**: `Arc<Catalog>` shared across rayon workers;
     internally `std::sync::Mutex<Connection>` so writes serialize at
     the SQLite layer AND a panicking worker poisons the mutex.

   - **Test infrastructure (`cfg(test)` knobs)** so tests are
     deterministic and CI-fast:
     - `LOCK_RETRY_DELAY_MS` — `pub(crate) const`, overridden to ~50ms
       under `#[cfg(test)]`. **All catalog-lock-exercising tests MUST
       use this override** to avoid 60s sleep time (closes R4.T8).
     - `HEARTBEAT_INTERVAL_MS` — same pattern (in `cli::commands::ingest`).
     - A `#[cfg(test)] fn poison_for_testing(&self)` on `Catalog` that
       forces a panic-inside-the-mutex via a closure, used by test 18.
     - A `#[cfg(test)] fn fail_init_after_create_table(&mut self)` knob
       gated by a test-only feature flag, used by test 12.

5. **`ingest_one` function** in `photohelper-cli::commands::ingest`:
   - `fn ingest_one(path: &Path, root: &AbsPath, catalog: &Catalog, stats:
     &IngestStats) -> Result<IngestOutcome, Error>`.
   - Workflow: canonicalize via `AbsPath::canonicalize_within(root, path)`;
     compute `clamped_mtime` ONCE (clamp to `[1995-01-01, 2100-01-01]`);
     `PhotoId::derive` with that clamped value; on `HashWindowTooSmall`
     log WARN + return `Ok(SkippedHashWindowTooSmall)`; parse EXIF with
     `kamadak-exif`; if parse yields zero fields return
     `Ok(NoExifFields)` + WARN; else call `Catalog::upsert(...)` passing
     the clamped mtime + EXIF; map result to outcome variant.
   - Driver reads the written catalog row's `camera_slug IS NOT NULL`,
     `mtime_anomalous`, and EXIF-presence to increment the right
     `IngestStats` atomics — single source of truth (R3.T10 fix).

6. **Integration test suite** (`crates/photohelper-cli/tests/cli.rs`).
7. **Unit tests** per crate.
8. **Decision artifact** `docs/decisions/0001-catalog-schema-v1.md`.

### PhotoId derivation (locked spec)

```text
PhotoId = BLAKE3(
    file_size_u64.to_le_bytes()              // 8 bytes, little-endian
 || clamped_mtime_i64.to_le_bytes()          // 8 bytes; clamped to [1995-01-01, 2100-01-01]
 || first_64KB                                // exactly min(65536, file_size) bytes
 || last_64KB                                 // exactly min(65536, file_size) bytes; empty if file_size ≤ 65536
)
```

- Endianness explicitly little-endian. Documented in `PhotoId::derive`
  docstring.
- `clamped_mtime_i64` computed **ONCE** by the caller (`ingest_one`),
  passed to both `PhotoId::derive` AND the catalog insert — ensures hash
  input matches stored value (R3 MEDIUM avoided).
- Clamp ceiling pinned to `2100-01-01` (static, not `now() + 1 day`) so
  hash input is run-independent (closes R3.T7).
- Files ≤ 65536 bytes: `first_64KB = whole file`; `last_64KB = empty`.
- `file_size = 0` → `Error::HashWindowTooSmall { path, len: 0 }`,
  surfaced by the driver as `Ok(SkippedHashWindowTooSmall)` + WARN.
- Render: 43-char `base64url` no-pad via
  `base64::engine::general_purpose::URL_SAFE_NO_PAD`.

**Intentional trade-off**: hashing mtime + content means two
byte-identical copies via tools that stamp different mtimes (e.g. `cp`
default vs `rsync -t`) produce **different PhotoIds**. Deliberate — the
photographer-relevant identity is "this captured frame at this moment";
preserving the original mtime is part of identity, not noise. Users who
reorganize archives with mtime-mangling tools are outside the v0.1 happy
path; a `--hash-policy={content-only|content-and-mtime}` flag can be
added if a real user files for it.

### Path safety contract

- Every `source_path` canonicalized via `AbsPath::canonicalize_within(
  root, path)` before catalog insert. Rejects: NUL bytes
  (`Error::Io { op: "canonicalize-nul-check" }`); non-existent
  (`Error::Io { op: "canonicalize" }`); paths whose canonical form
  escapes the root (`Error::PathEscapesRoot`); symlink loops (surfaces
  as `io::Error` in `Io { op: "canonicalize" }`).
- `--catalog <path>`: missing parent dirs created (logged INFO; mkdir-p
  partial-failure → `Error::Io { op: "mkdir-p" }` naming the failing
  component). Existing-as-directory → `Error::CatalogPathIsDirectory`.
  Existing non-SQLite file → `Error::CatalogPathNotSqlite`.

### Observability contract

End-of-run summary printed via direct `eprintln!` (NOT `tracing`) — always
prints regardless of `-q`:

```text
walked: <N>, ingested: <M>, superseded: <S>, already-catalogued: <A>,
unknown-camera: <U>, no-exif: <X>, mtime-anomalous: <Y>,
skipped (non-RAW): <K>, skipped (too-small): <T>, errored: <E>
```

Heartbeat (also via `eprintln!`, always prints) every 10 seconds during
the ingest run.

Exit codes:

| Condition | Exit code |
|-----------|----------:|
| All RAWs ingested successfully | `0` |
| `walked > 0 && (ingested + superseded + already_catalogued) == 0` (likely wrong directory) | `64` (EX_USAGE) |
| `--strict && (unknown_camera > 0 \|\| mtime_anomalous > 0 \|\| errored > 0)` | `1` (POSIX generic failure) |
| Stub subcommand | `69` (EX_UNAVAILABLE) |
| Fatal (catalog open, lock budget exhausted, schema-too-new, poisoned, path-escape, IO at root, mkdir-p partial failure) | `74` (EX_IOERR) |
| `clap` parse failure | `2` (clap default — distinct from our `74`) |

Per-photo errors (`Error::Io` op-tagged, `Error::Exif`,
`Error::HashWindowTooSmall`) → skip + log + increment `errored`,
continue. `--strict` escalates at end-of-run.

Tracing-level table (defaults; `RUST_LOG` overrides):

| Event | Level |
|-------|-------|
| EXIF parse failure / parse-succeeded-empty | `WARN` |
| Unknown camera first-seen per make/model; subsequent | `WARN` / `INFO` |
| Skipped non-RAW / Skipped too-small | `INFO` / `WARN` |
| Superseded (same path, new content) / hardlink dedup | `INFO` / `INFO` |
| Per-photo success | `DEBUG` |
| `mtime` clamped (anomalous) / file-lock retry / WAL recovery on open | `WARN` |

Default `-v=0` surfaces `WARN+` to stderr. `-v`→INFO, `-vv`→DEBUG,
`-vvv`→TRACE. `-q` mutes everything below `ERROR` for tracing — summary
and heartbeat use `eprintln!` and always print.

`mtime` validation: filesystem mtime outside `[1995-01-01, 2100-01-01]`
clamped to nearest boundary, `mtime_anomalous` column → 1, WARN logged.

### Concurrency

- `rayon` default pool sized via `--threads` (clap-validated 1..=1024).
- `walkdir` → `par_bridge` → workers call `ingest_one`.
- `Arc<Catalog>` shared; internally `std::sync::Mutex<Connection>`.
  Mutex poisons on worker panic → `Error::CatalogPoisoned` after explicit
  `ROLLBACK` recovery. Each insert wrapped in `BEGIN IMMEDIATE;...;COMMIT;`.
- Heartbeat thread: detached, reads `Arc<AtomicBool>` stop flag set by
  driver at end-of-walk. Driver `is_finished()` check + WARN if dead
  early. (`AtomicBool` is a one-shot shutdown signal scoped to this
  thread; the "no `CancellationToken`" intent — no general cooperative-
  cancellation infrastructure — holds.)
- No `crossbeam-channel`, no dedicated writer thread, no general
  cancellation primitive this session.

### Out of scope (8 items)

1. LibRaw FFI / RAW pixel decode (session 02).
2. ONNX, `ort`, AI models (sessions 03+).
3. XMP sidecar I/O (sessions 04+).
4. `develop`, `export`, watermark, JPEG encode (sessions 04–05).
5. `photohelper-cameras` per-ISO noise model + color matrix bodies
   (Err-stubs only; session 02).
6. Windows build verification (v0.2).
7. `git-lfs` fixture CR3s (session 02).
8. Migration framework (single-table v1 schema; framework with 1→2 in
   session 02).

### Test plan

Every test asserts a concrete observable per `docs/testing-standards.md`.

| # | Area | Test type | Concrete assertion |
|--:|------|-----------|--------------------|
| 1 | PhotoId stability | unit | same file twice → identical bytes |
| 2 | PhotoId distinguishability | unit | identical first/last 64KB but different file_size → different IDs |
| 3 | PhotoId small files | unit | 100-byte: succeeds; 0-byte: `Error::HashWindowTooSmall { len: 0 }` |
| 4 | PhotoId LE-endian regression | unit | fixed inputs (size, mtime, content) → assert exact BLAKE3 output |
| 5 | PhotoId Display + from_db_bytes | unit | 43-char base64url-nopad; round-trips via `core::catalog_glue::photo_id_from_row_bytes` |
| 6 | PhotoId::from_db_bytes visibility | compile-test (trybuild) | `cli` cannot call `PhotoId::from_db_bytes` directly; only `core::catalog_glue::photo_id_from_row_bytes` is `pub` |
| 7 | AbsPath canonicalize rejections | unit | NUL byte → `Io { op: "canonicalize-nul-check" }`; non-existent → `Io { op: "canonicalize" }` |
| 8 | AbsPath canonicalize_within escape | unit `cfg(unix)` | tempdir `/foo`; symlink `/foo/evil → /etc/passwd`; `canonicalize_within("/foo", "/foo/evil")` → `Error::PathEscapesRoot` |
| 9 | AbsPath canonicalize_within root-is-path | unit | `canonicalize_within(root, root.as_path())` succeeds (root is trivially under itself) |
| 10 | Catalog magic-byte check | unit | text file → `CatalogPathNotSqlite`; directory → `CatalogPathIsDirectory` |
| 11 | Catalog schema-version check | unit | `user_version = 2` → `CatalogSchemaTooNew { found: 2, expected: 1 }`; `0` → init; `1` → OK |
| 12 | Catalog schema init transactional | unit (uses `fail_init_after_create_table` test knob) | inject failure between CREATE TABLE and PRAGMA user_version; next open re-runs init successfully (idempotent) |
| 13 | Catalog file-lock cross-process | integration (uses `LOCK_RETRY_DELAY_MS=50ms` test override) | spawn `photohelper ingest <tempdir>` twice via `std::process::Command`; second completes with `Error::CatalogLockHeld { attempts, total_ms }` |
| 14 | Catalog WAL-checkpoint warn | integration `cfg_attr(target_os = "macos", ignore)` | kill mid-ingest via SIGKILL; reopen; stderr contains "recovered N WAL frames" |
| 15 | Catalog insert: same path new content | unit | insert PhotoId-B at source_path that has PhotoId-A → both rows exist; A's `superseded_at_unix_seconds` set |
| 16 | Catalog insert: same content same path | unit | second insert of identical PhotoId → INSERT OR IGNORE; row count unchanged; INFO log |
| 17 | Catalog insert: same content different path (hardlink) | integration | hardlink → one row in photos; stderr contains hardlink-dedup INFO line at `-v` |
| 18 | Catalog mutex poison + ROLLBACK | unit (uses `poison_for_testing` knob) | force panic-inside-mutex during BEGIN IMMEDIATE; next insert returns `Error::CatalogPoisoned`; SELECT COUNT(*) confirms no partial row; second post-poison insert also returns `CatalogPoisoned` (no silent recovery) |
| 19 | Catalog insert transactional | unit | inject failure between BEGIN IMMEDIATE and COMMIT; assert no row visible (rollback) |
| 20 | Catalog Send+Sync compile-time | compile-test | `assert_send_sync::<Arc<Catalog>>()` in catalog::tests compiles |
| 21 | CameraRegistry for_exif known | unit | `("Canon", "Canon EOS R8")` → `Some(...)` with `id() == CameraId::Known(KnownCamera::CanonR8)` |
| 22 | CameraRegistry for_exif normalization | unit | trailing NUL bytes + surrounding whitespace stripped |
| 23 | CameraRegistry for_exif unknown | unit | `("Acme", "X1")` → `None` (not a panic) |
| 24 | KnownCamera::slug + from_slug round-trip | unit | `CanonR8.slug() == "canon-r8"`; `from_slug("canon-r8") == Some(CanonR8)`; `from_slug("nope") == None` |
| 25 | CameraProfile stub methods | unit | `CanonR8::base_iso()` → `Err(Error::CameraProfileNotImplemented { method: "base_iso", ... })` (NOT a panic) |
| 26 | ExifOrientation from_tag/to_tag round-trip | unit | for N ∈ 1..=8: `from_tag(N).unwrap().to_tag() == N`; tags 0/9 → `Error::Exif` |
| 27 | ExifOrientation slot-5/7 names | unit | tag 5 = `ExifOrientation::Transpose`; tag 7 = `ExifOrientation::Transverse` (R3.T9 EXIF canonical) |
| 28 | mtime clamp | unit | mtime = -1 → clamped to 1995-01-01, anomalous=true; mtime = 2200-01-01 → clamped to 2100-01-01, anomalous=true; in-range → unchanged, anomalous=false |
| 29 | ingest_one happy path | unit (`cli::commands::ingest`) | `.cr3` file → `IngestOutcome::Inserted(...)`; catalog row matches |
| 30 | ingest_one non-RAW filter | unit | `.jpg` → `IngestOutcome::SkippedNonRaw`; row count unchanged |
| 31 | ingest_one 0-byte | unit | 0-byte `.cr3` → `IngestOutcome::SkippedHashWindowTooSmall` |
| 32 | ingest CLI happy path | integration | tempdir with `a.cr3` (fixture mtime pinned to `2020-01-01` via `filetime::set_file_mtime` to avoid CI clock-drift flakiness — R4.T6) + `b.jpg`; exit 0; stderr contains `walked: 2`, `ingested: 1`, `skipped (non-RAW): 1`; SQL row: `source_path` ends `a.cr3`, `file_size` matches, `id` is 32 bytes, `mtime_anomalous = 0`, and `camera_slug` is either `'canon-r8'` (kamadak-exif parsed CR3 EXIF, default expectation) **OR** `NULL` with `make`/`model` also NULL (DN-006 fallback active — kamadak-exif could not parse CR3 ISO-BMFF). Pre-flight verdict at implementation start decides which branch the test asserts. Closes R4.T3 (test row 32 used to assume kamadak-exif always works, contradicting the DN-006 fallback). |
| 33 | ingest CLI summary survives `-q` | integration | `-q` + ingest; assert summary line still in stderr |
| 34 | ingest CLI `.with_context()` boundary | integration | force per-photo read error; stderr contains `ingesting <path>` |
| 35 | ingest CLI idempotency | integration | run twice; second stderr `already-catalogued: 1, ingested: 0`; row count stays at 1 |
| 36 | ingest CLI content change | integration | ingest; rewrite bytes; ingest again; two rows for same source_path; first has `superseded_at_unix_seconds` set |
| 37 | ingest CLI empty / wrong dir | integration | dir of only `.jpg` → exit `64` (EX_USAGE); `ingested: 0` |
| 38 | ingest CLI truly empty dir | integration | empty tempdir → exit `0`; `walked: 0` |
| 39 | ingest CLI unknown camera + `--strict` | integration | synthesized fake-EXIF file → without `--strict`: exit 0, `camera_slug IS NULL` in row, `make`/`model` populated; with `--strict`: exit 1 |
| 40 | ingest CLI walker edges (consolidated) | integration | tempdir with: hidden `.foo.cr3` (cataloged); symlink loop (handled); deeply nested empty subdir; non-UTF-8 path. No panic + sensible row count. |
| 41 | ingest CLI mtime-anomalous summary | integration | mtime=0 file → stderr `mtime-anomalous: 1`; row `mtime_anomalous = 1` |
| 42 | ingest CLI tracing per-event-class mapping | integration (parameterized) | EXIF failure → WARN; two same-make/model unknowns → one WARN + one INFO; mtime clamp → WARN. Default `-v=0`. |
| 43 | ingest CLI fatal exit codes | integration (parameterized) | catalog-path-is-directory → exit `74`; schema-too-new → exit `74`; lock-budget-exhausted → exit `74` |
| 44 | ingest CLI clap parse exit | integration | `--threads 0` → exit `2`; `--threads 2000` → exit `2`; `--catalog-lock-timeout-seconds 0` → exit `2` (closes R4.T2); `--catalog-lock-timeout-seconds 5000` → exit `2`; `--catalog /etc` (existing dir) → exit `74` |
| 45 | Stub subcommands | integration (parameterized) | each → exit `69`; stderr contains `not yet implemented` |
| 46 | CLI `--verbose` mapping | integration | `-v` enables INFO; `-vv` DEBUG; `-q` mutes WARN tracing but NOT summary or heartbeat |
| 47 | CLI `--catalog` override | integration | `--catalog /tmp/explicit.db` → DB at exact path, NOT `<input>/.photohelper/catalog.db` |
| 48 | Heartbeat appears at default verbosity | integration (uses `HEARTBEAT_INTERVAL_MS=100` test override) | ingest with synthetic delay; stderr contains at least one `[heartbeat]` line at default `-v=0` |
| 49 | `BEGIN IMMEDIATE SQLITE_BUSY` | unit | simulate busy on BEGIN IMMEDIATE → returns `Error::CatalogInsert` preserving rusqlite source |
| 50 | Workspace gates | `just ci` | fmt-check, clippy `-D warnings`, test, audit, verify-state all green |

**50 tests total** (exact count).

### Checkpoints firing this session (Cadence A)

| Checkpoint | When | Tier | Agents | Double-review? |
|------------|------|------|--------|----------------|
| Session start | done | Tier 1 | 1 — `general-purpose` | No |
| **Plan-review** | this checkpoint; Rounds 1–3 done; R4 pending user decision | Tier 5 | Full 8 per round | Yes |
| Sub-component review | first non-scaffold public API or >300 LoC | Tier 4 | 3–5 | Yes |
| **Session end** | before commit + push | Tier 5 | Full 8 | Yes |

### Expected discovery items

- **Partially resolves DN-005** (catalog schema v1).
- **DN-006** (EXIF reader for CR3): `kamadak-exif 0.6` committed; if
  pre-flight at start of impl shows CR3 ISO-BMFF parsing fails, file
  DN-006 and ship session 01 with NULL EXIF columns for CR3s.
- **Potential DN-007** (`rusqlite` static-link binary size impact).
- **Potential DN-008** (`std::sync::Mutex<Connection>` throughput vs
  dedicated-writer-thread).
- **Potential DN-009** (proptest for PhotoId collisions).

### Tech-debt entries created or touched this session (preview)

- No new TDs anticipated.
- TD-001 (action pinning) untouched.

### Session-end housekeeping (NOT tech-debt — was misfiled in v3, fix per R3.T14)

- Update `SESSION-STATE.md` Component progress to list `photohelper-catalog`.
- Update root `Cargo.toml [workspace] members` to include the 8th crate.
- Update `HANDOFF_REPORT.md` Checkpoint 1.

### Dependencies introduced this session

Verified on crates.io at plan authoring time.

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
| `base64` | 0.22 | core | `URL_SAFE_NO_PAD` engine |
| `kamadak-exif` | 0.6 | core | (none) — committed; DN-006 fallback noted |
| `time` | 0.3 | core, catalog | `macros` (rusqlite `time` feature deliberately OFF — catalog uses raw INTEGER Unix-seconds for cross-binding stability) |
| `rusqlite` | 0.40 | catalog | `bundled` |
| `fs4` | 1 | catalog | `sync` (method is `try_lock()`, NOT `try_lock_exclusive`) |
| `num_cpus` | 1 | cli | (none) |
| `assert_cmd` | 2 | cli (dev-dep) | (none) |
| `predicates` | 3 | cli (dev-dep) | (none) |
| `tempfile` | 3 | cli, catalog (dev-dep) | (none) |
| `static_assertions` | 1 | catalog (dev-dep, optional) | (none) |
| `trybuild` | 1 | core (dev-dep) | (none) — for compile-fail test row 6 |
| `filetime` | 0.2 | cli (dev-dep) | (none) — pins fixture mtime in test row 32 (closes R4.T6 flakiness risk) |

`cargo tree --workspace --depth 1` + `cargo audit` run at session-end;
if transitive count > 120 OR audit flags any advisory, file a TD.

### Non-goals

- Not optimizing ingest throughput (Mutex<Connection> is simple + correct;
  DN-008 reserved if profiling shows it).
- No AI work (needs decoded pixels → session 02).
- No SIGINT / cooperative cancellation (the heartbeat AtomicBool is a
  scoped shutdown signal, not general cancellation).
- No benchmark suite.

---

*Implementation notes (pseudocode, exact API signatures, sub-component
review plan) appended below this line **after** plan-review reaches green
(R4 if user requests; otherwise R3 closes with the v4 remediation above).
Until then, the contract above is the load-bearing artifact.*

---

## Post-R1 / Post-R2 amendments (2026-05-28)

The R1 + R2 remediation cycles deliberately tightened or relaxed several
plan items vs. plan-v5 as written. This section is the canonical
plan-vs-implementation diff a session-02 contributor reads to know what
the v0.1 contract actually says today. Per R2-T10 / `docs/code-reviews/
session-01-round2.md`.

### Dependencies — actual vs. v5 table

| Plan v5 said | Implementation shipped | Why | Tracking |
|---|---|---|---|
| `indicatif 0.17` | dep removed entirely (R1.T8) | heartbeat thread covers the same UX without competing with itself for the terminal line | HANDOFF Checkpoint 1; R1 review §T8 |
| `rusqlite 0.40 + bundled` | `rusqlite 0.32 + bundled` | 0.40 wasn't trivially buildable under the rest of the dep graph on 2026-05-28; deferred bump | TD-002 + DN-007 (binding trigger 2026-08-01) |
| `kamadak-exif 0.6` in `core` | `kamadak-exif` removed from `core` (R2-T26) | unused there; pulling format-specific parsers into the domain crate breaks the "core → ⊥" invariant | R2 §R2-T26 |
| `tracing 0.1` in `core` | `tracing` removed from `core` (R2-T26) | unused there; binary/CLI layer only | R2 §R2-T26 |
| `trybuild 1` in `core (dev-dep)` | kept but unused | plan row 6 (`PhotoId::from_db_bytes` compile-fail) deferred to session 02 | DN-008 |
| MSRV `1.85` | MSRV bumped to `1.88` | `time 0.3.47` requires it; consumes RUSTSEC-2026-0009 fix | ADR-0001 |

### Deliverables — actual vs. v5

| Plan v5 deliverable | Status post-R2 | Notes |
|---|---|---|
| §Deliverables 1: `indicatif` spinner | DROPPED (R1.T8) | heartbeat covers the same UX |
| §Deliverables 5: per-photo `.with_context()` boundary | DROPPED (R1.T10) | replaced with structured `Error::Io { path }` + `Error::CatalogInsert { photo_id }` variants; `ContextForPath` trait deleted as no-op |
| §Test infrastructure: `LOCK_RETRY_DELAY_MS` knob | partially landed (`Catalog::open_with_retry_delay` exists as `pub fn #[doc(hidden)]`, NO test calls it; R2-T15 / DN-008 binding trigger for session-02 row 13 cross-process test) | dead public API awaiting consumer |
| §Test infrastructure: `HEARTBEAT_INTERVAL_MS` knob | landed; test row 48 deterministic post-R2-T6 rewrite | env-var override `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` |
| §Test infrastructure: `poison_for_testing` | NOT landed | DN-008 binding trigger for session 02 |
| §Test infrastructure: `fail_init_after_create_table` | NOT landed | DN-008 binding trigger for session 02 |
| §Type-design: `MtimeFacts { clamped, anomalous }` newtype (R1.T13 sub-fix) | NOT landed | DN-011 binding trigger for next session touching `model.rs` clamp_mtime callers |

### Behavioral contract — actual vs. v5

| Plan v5 said | Implementation reality | Why |
|---|---|---|
| `--strict` fails on unknown camera / mtime anomalous / errored | `--strict` ALSO fails on `no_exif > 0` (R2-T12 expansion) | the "EXIF entirely missing" case is operationally equivalent to "unrouted photo"; the prior shape was fail-open for this case (user's prod trace surfaced) |
| Heartbeat fires at `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` env value | Now genuinely fires at the env value (R2-T4 fix to granularity) | prior `granularity = 100ms` hardcode silently floored sub-100ms requests; env-var advertised 10ms minimum but loop fired at 100ms |
| Per-CR3 EXIF behavior | DN-006 fallback (NULL EXIF) is the DEFAULT for ALL real Canon R8 CR3, not just synthetic fixtures (DN-011) | kamadak-exif cannot parse CR3 ISO-BMFF at all in v0.1; LibRaw EXIF in session 02 is critical path |

### Plan rows — actual coverage vs. v5

Plan v5 §Test plan declared 50 rows. Coverage post-R2:
- **closed**: rows 32, 33, 35-38, 40, 41, 44-47, 48 (R2-T6 deterministic rewrite), 50 = ~38 rows
- **deferred per DN-008** (binding trigger = session 02): rows 6, 12, 13, 14, 17, 18, 19, 34, 39, 42, 43, 49 = 12 rows
- **R2-T19 PhotoId discriminating-window test** replaces the prior 128KB non-discriminating test

This list is the authoritative coverage-state for the session-02
plan-review to compare against — NOT the historical R1 review's count
(which had a triple-drift per R2-T22, since reconciled in DN-008 +
SESSION-STATE + this amendments table).
