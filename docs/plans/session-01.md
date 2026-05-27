# Session 01 — `cli-skeleton-and-ingest`

> **Branch**: `session-01/cli-skeleton-and-ingest`
> **Started**: 2026-05-27
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)

## Session contract (top block — reviewed at plan-review checkpoint)

### Goal

Land the thinnest end-to-end slice that proves the seven-crate workspace
architecture. Concretely: a real `photohelper ingest <path>` subcommand that
walks a directory of files, recognises RAW extensions, reads EXIF, derives a
content-addressed `PhotoId`, and writes catalog rows to a SQLite database at
`<path>/.photohelper/catalog.db`. All other subcommands (`cull`, `develop`,
`export`, `run`, `models`, `camera`) are stubbed in `clap` so the CLI surface
is visible from day one, but only `ingest` does work this session.

This session intentionally does **not** decode any RAW pixels — that lands in
session 02 with the LibRaw FFI. Reading EXIF + the embedded preview header
without pixel decode is sufficient to populate the catalog.

### Deliverables (what will exist when the PR merges)

1. **`photohelper-cli` (binary `photohelper`)**
   - `clap` v4 derive API with subcommand stubs: `ingest`, `cull`, `develop`,
     `export`, `run`, `models`, `camera`.
   - Global flags: `--verbose/-v` (repeatable, sets `tracing` level), `--quiet/-q`,
     `--threads <N>` (default = `num_cpus`), `--catalog <path>` (default =
     `<input>/.photohelper/catalog.db`), `--no-color`.
   - `tracing` subscriber initialized in `main` (compact `fmt` layer; level from
     `-v` count; `tracing-subscriber` with env-filter for opt-in module-level
     overrides).
   - `ingest` flags: `--recursive/-r` (default `true`).
   - All non-`ingest` subcommands print `not yet implemented (session NN)` on
     `stderr` and exit with code `0` (stubs are visible, not errors).
   - `indicatif` progress bar wraps the `ingest` walk and shows count + ETA.

2. **`photohelper-core` (lib)**
   - `module model` with: `PhotoId` (newtype over `[u8; 32]`, content-derived
     via BLAKE3 of `file_size || first_64KB`; renders as 32-char base32); `Photo`
     (id, source_path, file_size, mtime, camera_id: `Option<CameraId>`,
     capture_time: `Option<OffsetDateTime>`, dimensions: `Option<(u32, u32)>`,
     orientation: `Option<Orientation>`); `CameraId` enum with `CanonR8`
     variant + an `Unknown { make, model }` catch-all; `Orientation` enum
     (`Landscape`/`Portrait` derived from EXIF orientation tag).
   - `module error` with a `thiserror`-derived `Error` enum exposing variants
     for IO, EXIF parse, hash mismatch, catalog open/insert. Library returns
     `Result<T, Error>`; binary converts to `anyhow::Result` at the CLI
     boundary only.
   - `module pipeline` with a `Pipeline` trait: `fn run(&self, ctx:
     &PipelineCtx, photo: &Photo) -> Result<Sidecar, Error>` — defined and
     unit-tested even though only `IngestStage` will implement it this
     session. `PipelineCtx` is a struct with `catalog: &Catalog`,
     `tracing_span: &tracing::Span`, `cancel: &CancellationToken`. `Sidecar`
     is a placeholder enum with one variant `Ingested { photo_id }` — full
     XMP comes later.

3. **`photohelper-cameras` (lib)**
   - `CameraProfile` trait with stub methods (`id`, `make_model`,
     `base_iso`, `sensor_layout`). Real per-ISO noise model and color matrix
     methods land in session 02 with `panic!` `todo!()` bodies guarded behind
     `unimplemented!`.
   - `CanonR8` struct implementing `CameraProfile` with the EXIF
     identification path only (make = `"Canon"`, model = `"Canon EOS R8"`).
   - `CameraRegistry` with `fn for_exif(&self, make: &str, model: &str) ->
     Option<Arc<dyn CameraProfile>>`. Registry initially holds only `CanonR8`;
     unknown bodies return `None` and `ingest` records `CameraId::Unknown`
     in the catalog (a deliberate non-fatal soft fail — we still catalogue
     the file).

4. **Catalog (in `photohelper-core::catalog`)**
   - `rusqlite` (bundled feature for the SQLite amalgamation, avoiding a
     system libsqlite3 dependency on Linux/Windows).
   - Minimum schema: a `photos` table (columns: `id BLOB PRIMARY KEY`,
     `source_path TEXT NOT NULL UNIQUE`, `file_size INTEGER NOT NULL`,
     `mtime_unix_ns INTEGER NOT NULL`, `make TEXT`, `model TEXT`,
     `camera_id TEXT`, `capture_time_unix_ns INTEGER`, `width INTEGER`,
     `height INTEGER`, `orientation TEXT`, `ingested_at_unix_ns INTEGER NOT
     NULL`); a `schema_version` table (`version INTEGER PRIMARY KEY`)
     seeded with `1`. Indices: `idx_photos_source_path` (UNIQUE),
     `idx_photos_camera_id`. UPSERT on source_path so re-running `ingest` is
     idempotent.
   - One forward migration helper (`fn migrate(conn: &mut Connection) ->
     Result<(), Error>`) that reads `schema_version`, applies pending
     migrations in order. Session 01 ships only migration `0 -> 1`.
   - DN-005 (catalog schema) — this session is its owner; closes by recording
     the final schema in a `docs/decisions/0001-catalog-schema-v1.md`.

5. **Concurrency**
   - `rayon` `par_bridge` over a `walkdir` iterator for the ingest walk.
   - SQLite writes are serialized through a single writer thread (a
     `crossbeam-channel` mpsc; rayon workers send `IngestedRow` messages,
     writer consumes). Reasoning: SQLite write contention with N parallel
     writers is worse than one dedicated writer with N producers (the standard
     pattern). Reads (UPSERT-pre-existence check) go through the same channel
     to keep ordering deterministic.

6. **Integration test**
   - `crates/photohelper-cli/tests/cli.rs` using `assert_cmd` + `tempfile`.
   - Test: place 2 fake files (`a.cr3`, `b.jpg`) in a temp dir; run
     `photohelper ingest <tempdir>`; assert exit 0; open the resulting
     `<tempdir>/.photohelper/catalog.db` with `rusqlite` and assert one row
     in `photos` (only `a.cr3` matches the RAW extension filter, `b.jpg` is
     skipped).
   - Second test: run `ingest` twice on the same directory; assert the
     second run reports "0 new, 1 already catalogued" and the row count
     stays at 1 (idempotency).

7. **Unit tests** (per crate, must run under `cargo test --workspace`)
   - `photohelper-core::model::PhotoId` round-trips through its render+parse
     and produces stable hashes across identical inputs.
   - `photohelper-core::catalog` migration `0 -> 1` is idempotent on
     re-application.
   - `photohelper-cameras::registry::for_exif("Canon", "Canon EOS R8")`
     returns `Some(CanonR8)`; unknown make/model returns `None`.

### Out of scope (deferrals — anything dropped here goes to TECH-DEBT.md if it leaves a stop-gap behind)

- LibRaw FFI or any RAW pixel decode (session 02).
- ONNX, `ort`, any AI model or model registry (session 03+).
- XMP sidecar read/write (session 04+).
- `develop`, `export`, watermark, JPEG encode (sessions 04–05).
- `photohelper-cameras` per-ISO noise model + color matrix bodies (left as
  `todo!()` stubs guarded by `unimplemented!`; session 02 fills them).
- Windows build verification (the v0.1 target is Linux + macOS; Windows
  catches up in v0.2 per the bootstrap plan).
- `git-lfs` fixture CR3s (session 02 introduces them).

### Test plan (how each deliverable is verified before session-end)

| Layer | Test type | What it catches |
|-------|-----------|-----------------|
| `PhotoId` derivation | unit (`#[test]` in `core/src/model.rs`) | hash stability across identical-content files; renders are 32 chars |
| Catalog migration | unit (`#[test]` in `core/src/catalog.rs`) | re-application of migration 0→1 is a no-op |
| `CameraRegistry::for_exif` | unit | known Canon R8 matches; unknown make returns None (not a panic) |
| `ingest` happy path | integration (`tests/cli.rs`) | files walked, RAW filter applied, catalog row written |
| `ingest` idempotency | integration (`tests/cli.rs`) | second run does not duplicate rows |
| Workspace gates | `just ci` | fmt-check, clippy -D warnings, test, audit, verify-state all green |

No `expect(true).toBe(true)` / no-assertion tests (per global testing
standards in `~/.claude/CLAUDE.md`). Every test asserts a concrete observable
behaviour.

### Checkpoints firing this session (Cadence A)

| Checkpoint | When | Agents (Cadence A) |
|------------|------|--------------------|
| Session start | done (this read-and-declare step) | 1 (alignment) — implicit in the manual session-start protocol |
| **Plan review** | after this contract commits, before any code | **Full 8** (mandatory at plan review) |
| Sub-component review | at each crate boundary that introduces non-scaffold public API: `photohelper-core::model`, `photohelper-core::catalog`, `photohelper-cli` ingest path, `photohelper-cameras::registry` | 3–5 per boundary (Cadence A multi-file change) |
| **Session end** | before commit + push | **Full 8** (mandatory at session end) |

Round 1 → remediate → Round 2 → remediate is enforced at every checkpoint
(`docs/quality-assurance.md § Double-review protocol`). Never stop after
Round 1.

### Expected discovery items (DN-NNN) — flagged up-front so they aren't surprises

- **Closes DN-005** (catalog schema) by landing v1 schema + a
  `docs/decisions/0001-catalog-schema-v1.md` record. If the v1 schema turns
  out to need fields we didn't anticipate (e.g. a `xmp_sidecar_path` we want
  to populate eagerly), DN-005 stays open with a sub-note.
- **Potential new DN-006**: EXIF reader choice (`kamadak-exif` vs `nom-exif`
  vs `little_exif`). `kamadak-exif` is the obvious pick (most mature, pure
  Rust, MIT) but if its CR3-container EXIF support is incomplete we may
  surface a gap to revisit in session 02 when libraw provides an alternate
  source.
- **Potential new DN-007**: `rusqlite` static-link size impact. Bundled SQLite
  adds ~1.5 MB to the binary. If that materially affects the v0.1
  size-budget conversation, file as DN and reconsider in the release-eng
  session.

### Tech-debt entries created or touched this session (preview — finalised at session end)

- No new TDs anticipated for the planned scope; everything above is a real
  deliverable, not a stop-gap.
- TD-001 (action pinning) is **not** touched this session — it's gated on
  "before first external contributor or first release," neither of which
  trigger yet.

### Non-goals (clarifying boundary)

- This session does not optimise ingest throughput. Single-writer SQLite is
  simple and correct; if profiling shows the writer is the bottleneck on
  10k+ photo runs, that's a session-02+ concern.
- This session does not start the AI work. Even though v0.1 is "AI-first,"
  AI features need a `Photo` + decoded pixels to feed them. We need the
  catalog first.

---

## (Below this line — implementation notes, pseudocode, package picks)

This bottom half lands AFTER plan-review Round 2 + remediation. The
session-start protocol only requires the contract above to be committed.
Plan-review fires next; implementation begins after Round 2 + remediation
are clean.
