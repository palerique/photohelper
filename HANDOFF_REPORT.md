# photohelper — Handoff Report

> Accumulated summary for stakeholders / the next contributor. Each session
> appends a checkpoint block rather than rewriting history; the git log of this
> file is the versioned timeline. Demote aged blocks to `docs/session-archive/`
> per the rolling-archive convention to keep this file readable.

---

## Checkpoint 0 — bootstrap (2026-05-27)

**Status**: bootstrap
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session)

### What exists
- Engineering protocol adopted from the `maxim/eng-protocol-toolkit/template`
  (stack-agnostic protocol files) layered with a new `stacks/rust.md` adapter.
- Governance files in place: `CLAUDE.md`, `SESSION-STATE.md`,
  `HANDOFF_REPORT.md`, `TECH-DEBT.md`, `docs/quality-assurance.md` (cadence A),
  `docs/session-handoff-format.md`, `docs/discovery-notes.md` (seeded with
  DN-001…DN-005), `justfile`, `.pre-commit-config.yaml`,
  `.github/workflows/ci.yml`, `scripts/verify-state.sh`, `stacks/rust.md`.
- `.claude/skills/` carries the four canonical skills (`session-start`,
  `session-end`, `plan-review`, `eight-agent-review`) copied verbatim from
  the maxim toolkit's `plugins/eng-protocol/skills/` directory.
- Rust workspace scaffolded with 7 member crates (`photohelper-cli`,
  `photohelper-core`, `photohelper-raw`, `photohelper-ai`,
  `photohelper-sidecar`, `photohelper-export`, `photohelper-cameras`); each
  non-binary crate ships a one-line `lib.rs` stub so `cargo test --workspace`
  compiles green.
- Toolchain pinned in `rust-toolchain.toml` (channel `1.85.0`, components
  rustfmt + clippy, minimal profile). Workspace-level lints baseline wired
  (`missing_docs = warn`, `unsafe_code = forbid`, clippy pedantic + the
  `unwrap`/`expect`/`panic`/`indexing` warns).
- Dual MIT/Apache-2.0 license (`LICENSE-MIT`, `LICENSE-APACHE`).
- Remote `origin` points at https://github.com/palerique/photohelper.git
  (public, empty until the bootstrap commit lands).

### What is not yet in place
- No real application code yet — only stubs. The full v0.1 scope (AI culling,
  SCUNet denoise, develop, export, watermark) ships across sessions 01-N per
  the bootstrap plan at
  `/Users/ph/.claude/plans/first-create-a-structure-warm-shell.md`.
- No fixture CR3s committed yet; session 02 introduces a small CC0 RAW pack
  via `git-lfs`.
- No release/distribution wiring (musl static, codesign, Authenticode,
  Homebrew tap, winget) — that's its own session later.

### How to resume
```bash
git switch main && git pull --ff-only origin main
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 1 — session 01 `cli-skeleton-and-ingest` (2026-05-28)

**Status**: implementation complete; session-end review Round 1 done; Round 1 remediation applied; Round 2 pending.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 01)

### What exists
- 8-crate Cargo workspace fully wired (added `photohelper-catalog` to
  the 7 bootstrap stubs): `photohelper-{cli, core, raw, ai, sidecar,
  export, cameras, catalog}`. Four crates implemented (`cli`, `core`,
  `cameras`, `catalog`); the other four remain one-line stubs awaiting
  sessions 02 (raw), 03+ (ai), 04+ (sidecar, export).
- `photohelper-core::model` (~1000 lines, 33 unit tests): `PhotoId`
  (private `[u8; 32]`, BLAKE3 of `file_size_le || clamped_mtime_le ||
  first_64KB || last_64KB`, DISJOINT head/tail windows per R1.T3 fix;
  43-char base64url Display); `AbsPath` (canonicalize + canonicalize_within
  closing path-escape); `Photo` (private fields, fallible
  constructor); `CameraId { Known | Unknown }`; `KnownCamera::CanonR8`
  with slug/from_slug round-trip; `ExifOrientation`
  (#[non_exhaustive], canonical EXIF Transpose/Transverse at slots 5/7,
  from_tag/to_tag); `Aspect` (#[non_exhaustive]); `ExifMetadata`
  (is_empty); `IngestOutcome` (#[non_exhaustive]); `clamp_mtime` to
  static `[1995-01-01, 2100-01-01]`.
- `photohelper-core::error`: #[non_exhaustive] 13-variant `Error` enum
  (added `InvalidExifOrientationTag` per R1.T11 fix), no `#[from]`
  derives, BoxedSourceError for catalog variants keeps core
  storage-agnostic.
- `photohelper-core::catalog_glue::photo_id_from_row_bytes`: the
  sole `pub fn` minting a `PhotoId` from raw bytes; `PhotoId::from_db_bytes`
  is `pub(crate)`. Closes the forgery surface from R3.T2.
- `photohelper-cameras`: `CameraProfile` trait + `CanonR8` stub +
  `CameraRegistry::for_exif` with NUL/whitespace normalization.
- `photohelper-catalog`: `Catalog` (3-field struct, `Send + Sync`
  compile-time assertion); 10-step `Catalog::open` with fs4 file-lock
  (lock-first ordering), magic-byte check, WAL + busy_timeout
  PRAGMAs, schema-version gate, transactional init,
  wal_checkpoint(TRUNCATE) WARN on recovered frames (now logs PRAGMA
  errors instead of silent unwrap_or(0) per R1.T10);
  `Catalog::upsert` with `BEGIN IMMEDIATE` per insert + `PoisonError
  → ROLLBACK → CatalogPoisoned` recovery (R3.T5); single-helper
  insert path (R1.T14 dedup).
- `photohelper-cli`: clap v4 with global flags including
  `--catalog-lock-timeout-seconds` (1..=3600); 7 subcommands stubbed
  (cull/develop/export/run/models/camera → exit 69 EX_UNAVAILABLE);
  `ingest` does real work via `ingest_one` in
  `cli::commands::ingest`; rayon `par_bridge` + `IngestStats`
  atomics (`no_exif` counter now increments per R1.T1); heartbeat
  thread via `eprintln!` with `Arc<AtomicBool>` stop flag and
  death-WARN before stop-flag set (R1.T2); rayon `build_global`
  failure now WARNs (R1.T10); end-of-run summary via direct
  `eprintln!` survives `-q`.
- 63 tests passing (was 59 at initial implementation; added 4 new
  in R1 remediation: PhotoId disjoint-window 100KB regression,
  PhotoId disjoint-window 128KB regression, no_exif counter
  integration, heartbeat env-override integration).
- `just ci` green at remediation commit.

### Plan-vs-implementation drift (acknowledged, tracked)
- **MSRV bumped 1.85 → 1.88** to consume `time 0.3.47`'s fix for
  RUSTSEC-2026-0009. Tracked: `docs/adr/0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md`.
- **`rusqlite` shipped at 0.32 instead of plan-v5's 0.40 target**.
  Tracked: `TD-002` + `DN-007` with binding trigger
  (bump by 2026-08-01 or before session 02 adds new schema columns).
- **`indicatif` spinner dropped** from plan v5 §Deliverables 1: the
  heartbeat thread (eprintln! every 10s) provides the same liveness
  signal and avoids competing with the heartbeat for the terminal
  line. Recorded in `Cargo.toml` workspace deps comment.
- **`kamadak-exif` confirmed unable to parse synthetic CR3 ISO-BMFF**:
  DN-006 fallback active for session 01. Real CR3 parse-status
  re-verified in session 02 with `git-lfs` fixtures.
- **~12 plan test rows ship without coverage**: tracked in DN-008
  with session-02 owner + binding trigger.

### What is not yet in place
- Real CR3 fixtures (session 02 via git-lfs).
- LibRaw FFI for RAW pixel decode (session 02).
- AI culling / denoise (session 03+).
- XMP sidecar I/O (session 04+).
- develop / export / watermark (session 04–05).
- Windows build verification (v0.2).
- Release-engineering wiring (musl static, codesign, Authenticode,
  winget, Homebrew tap).

### How to resume
```bash
git switch main && git pull --ff-only origin main
just session-start
cat SESSION-STATE.md
```

### Open Round-2 items
Session-end Round 1 surfaced 7 CRITICAL + 5 HIGH; remediation applied
in commits following `310f753` (see git log on
`session-01/cli-skeleton-and-ingest`). Round 2 fires next to verify
the remediation held cleanly.
