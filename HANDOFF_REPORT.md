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
  AI denoise (model TBD pending session-04 plan-review), develop, export, watermark) ships across sessions 01-N per
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
- `photohelper-core::model` (~990 lines, 30 unit tests in model.rs / 32 across the crate): `PhotoId`
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

---

## Checkpoint 2 — session 01 paused for context refresh (2026-05-28)

**Status**: PAUSED. Last commit on branch: `02d43d1` (harness sync).
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 01)

### What landed since Checkpoint 1
- **Session-end Round 1** complete: full 8-agent suite fired against
  commit `310f753`. Findings consolidated by theme in
  `docs/code-reviews/session-01-round1.md` (7 CRITICAL + 5 HIGH + 4
  MEDIUM + 3 LOW + 7 strengths preserved).
- **R1 remediation** committed at `0f28627`. Applied every CRITICAL +
  HIGH inline; deferred specific MEDIUMs to session 02 via DN-008
  with binding triggers. Test count grew 59 → 63.
- **Harness sync from fox/eng-protocol** committed at `02d43d1`.
  Major upgrade to `eight-agent-review` skill (added §0 precondition
  gate with memoization, §1 plugin-availability detection, §3
  `model: "opus"` pin, §3.a 5-section sentinel-marker template, §6
  9th-agent verifier with verbatim Read-window quotation, §6.b
  verification YAML block). Added `session-pause` skill (5th in our
  roster). Added `.claude/settings.json` + bash hook
  `detect-eng-protocol.sh` + `.claudeignore`. Stack-specific fox
  items (`address-vulnerabilities` npm command, `eng-protocol.mjs`,
  `verify-review-artifact.mjs`) deliberately skipped — TD candidate
  to bash-port the artifact enforcer later.
- **All R1 governance work**: ADR `0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md`,
  decision `0001-catalog-schema-v1.md`, DN-006/007/008,
  TD-002 (rusqlite 0.32 stale).

### Why paused
Context window approaching limit after 4 plan-review rounds + initial
implementation + 1 session-end review round + R1 remediation + harness
sync — total turn count and accumulated context warrants a refresh
before R2 fires (so R2 doesn't run with degraded reasoning quality
caused by context pressure).

### Precise next steps when context restored
1. **Read `SESSION-STATE.md`** (canonical re-orientation per the
   resume prompt).
2. **Read this Checkpoint 2** (you're here).
3. **Read `docs/code-reviews/session-01-round1.md`** to know what R1
   surfaced and what R1 remediation closed.
4. **Skim the R1 remediation diff**: `git show 0f28627` for code
   changes; `git show 02d43d1` for harness changes. The R2 watch-list
   at the bottom of round1.md is the canonical "what R2 must verify."
5. **Fire `/eight-agent-review session 0f28627..HEAD`** (or
   equivalent — scope is the R1 remediation diff against R1's
   findings). The newly-upgraded skill will:
   - Prompt the §0 precondition gate (first invocation in the fresh
     context — answer per your session config; the cache memoizes
     for subsequent rounds).
   - Pin `model: "opus"` on every sub-agent invocation.
   - Run the 9th verifier agent to catch hallucinated findings (this
     is the new mechanism — R1 had 4 hallucinated file:line refs that
     the verifier would have caught).
   - Emit the three YAML blocks (session_config, plugin_availability,
     verification) at the top of the artifact.
6. **Round 2 → remediate → Round 3 if R2 surfaces CRITICAL**. Per
   `docs/quality-assurance.md § Double-review protocol`: never stop
   after Round 1.
7. **After R2 is clean**: commit ledgers + final state; push the
   branch; `gh pr create --base main --head session-01/cli-skeleton-
   and-ingest`; wait for green CI; `gh pr merge --merge --delete-
   branch`; render the two-block handoff per
   `docs/session-handoff-format.md`.

### Resume from a fresh context
```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-01/cli-skeleton-and-ingest
just session-start
```

Then paste the restart prompt rendered at the end of the pause turn
(below the ledger updates).
