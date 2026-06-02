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

---

## Checkpoint 3 — session 02 paused for context refresh (2026-05-28; plan-review complete)

**Status**: PAUSED. Plan-review iteration COMPLETE (R1 + R2 + R3 + targeted R3 remediation). No code yet — pre-implementation pause.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 02 plan-review)

### What landed this window

Plan-review for session 02 ran 3 rounds with full agent suite (Cadence A Tier 5 — 8-agent suite at R1/R2, 7-agent at R3 with type-design-analyzer skipped post-closure). 10 plan-review commits on `session-02/libraw-cr3-decode`:

- `b377aed` — plan v1 (initial contract)
- `354406f` — plan-review Round 1 artifact
- `b64425f` — SESSION-STATE.md drift cleanup (closes PR1-T22)
- `5d5dc9a` — R1 remediation cross-doc fixes (PR1-T8 decision-doc 0001 amendment; PR1-T17 LGPL §6(a) correction; DN-013/014/015 + TD-004 filed)
- `69b6a5b` — plan v2 R1 remediation (closes 16 CRITICAL + 17 HIGH)
- `c80acf3` — plan-review Round 2 artifact
- `0e54129` — R2 remediation cross-doc filings (DN-016/017/018 filed; SCUNet residue scrubbed)
- `dc41dee` — plan v3 R2 remediation (closes 9 CRITICAL + 14 HIGH)
- `37373f4` — plan-review Round 3 artifact
- `dd62166` — R3 remediation plan v3.1 + TD-005/006/007 + SESSION-STATE update

**Plan**: `docs/plans/session-02.md` v3.1 (1027 lines). Goal: LibRaw FFI for Canon R8 CR3 — EXIF read (DN-011 critical path) + RAW pixel decode + TD-002 rusqlite bump + 6-of-12 DN-008 rows + R2-T18 4/4 closure via env-var heartbeat hatch.

**Self-criticism (preserved in audit trail)**: While remediating R2-T1 (phantom PR1-T# IDs in v2), I introduced 7 new phantom R2-* IDs in v3 (R2-S2, R2-T26, R2-PT2/3/4/7/8). R3 caught it; v3.1 corrected. Recorded as a "new bug class" pattern in `docs/code-reviews/session-02-plan-round3.md § R3-T1` + the new-bug-classes note: "phantom-ID recurrence at every round."

**Diminishing returns observation**: each plan-review round closes most prior-round CRITICALs but introduces ~3-7 new ones (R1 → 16 CRITICAL; R2 → 9 inside R1 remediation; R3 → 7 inside R2 remediation). R4 NOT fired per 6-of-7 R3 agent consensus + recognition that further rounds would surface derivatives without substantive plan improvement.

### CI gate state at pause time

`just session-end` (= `just ci`) FAILS on a single test: `crates/photohelper-cli/tests/cli.rs::heartbeat_fires_during_ingest_when_interval_is_short`. Verified 5/5 failures on apple-silicon. Root cause is **TD-003** (heartbeat thread not `.join()`-ed) manifesting empirically — ingest of 80 stub CR3s completes in ~0.28s; spawned heartbeat thread doesn't flush its first `eprintln!` before parent process exits. NOT a regression from this session's docs-only work; the test was always borderline (session-01 R2 just got lucky). TD-003's binding trigger explicitly includes "test-flake surfaces on CI from stderr-ordering instability" — that trigger is NOW FIRED. Recorded as `docs/discovery-notes.md § DN-019`. Session 02 implementation MUST close TD-003 before Acceptance criterion 1 (`just ci` green) can be satisfied — fold into Deliverable 0 or the FFI module commit per TD-003's existing fundamental-fix spec (~15 LoC).

### Why paused

Plan-review iteration consumed substantial context across R1+R2+R3+remediation cycles. Pausing here to refresh context BEFORE implementation begins so Deliverable 0 (pre-flight LibRaw EXIF + CVE-posture audit) is authored with full reasoning quality, not under context pressure.

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** (canonical re-orientation per the resume prompt).
2. **Read this Checkpoint 3** (you're here).
3. **Read `docs/plans/session-02.md`** (1027 lines; the implementation contract — pay special attention to Deliverable 0 pre-flight + Deliverable 1's FFI strategy lock + Deliverables 4-7 sequencing).
4. **Skim `docs/code-reviews/session-02-plan-round3.md`** for the R3 watch-list (specifically TD-005/006/007 that session 02 implementation MUST address inline as code is written).
5. **Read `docs/discovery-notes.md § DN-019`** for the TD-003 empirical-trigger note + remediation requirement.
6. **Begin implementation per plan v3.1's sequencing**:
   - **Deliverable 0 (pre-flight; first commit `chore(libraw): pre-flight EXIF + CVE-posture audit`)**: invoke chosen LibRaw entry against `/Users/ph/Pictures/tests` 371-CR3 set; verify LibRaw 0.21.4 (or actually-current 0.21.x latest) extracts Make/Model/Orientation/CaptureTime; check MITRE CVE feed for any open CVE on the chosen version; produce `docs/analysis/ANL-001-libraw-cr3-preflight.md`. ABORT if >5% extraction failure OR any open CVE → raise plan-review v4.
   - **Address TD-003 in lockstep** (~15 LoC heartbeat `.join()` cleanup; unblocks Acceptance criterion 1).
   - Then Deliverables 1 (FFI/exif/decode/Error) → 2 (build-system + ADR-0002) → 3 (Git LFS fixtures + sanitize) → 4 (ingest rewire atomic commit) → 5 (rusqlite bump) → 6 (test infra including TD-005/006/007 inline addressing) → 7 (DN-012 polish).
   - **Sub-component reviews** fire at FFI boundary + LGPL build-system per the Checkpoints table.

### Resume from a fresh context

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-02/libraw-cr3-decode
just session-start
```

Then paste the restart prompt rendered at the end of this pause turn.

---

## Checkpoint 4 — session 02 paused for context refresh (2026-05-28; CI green, Deliverable 0 done, Deliverable 1a scaffolded)

**Status**: PAUSED. Last commit on branch: `440388a` (Deliverable 1a scaffolding). `just ci` GREEN end-to-end. Pre-implementation gates closed.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 02, post-Checkpoint-3 implementation window)

### What landed this window

Four commits on `session-02/libraw-cr3-decode`, all conventional, no force-pushes:

- `bb87735` — **`fix(session-02): close TD-003 (heartbeat join) per DN-019 trigger`**. Replaces `AtomicBool` + `thread::sleep` with `HeartbeatStop` (`Mutex<bool>` + `Condvar`) and a named `thread::Builder::new().name("ph-heartbeat").spawn(...)` handle. `run_ingest` now calls `stop.signal()` then `heartbeat_handle.join()` so every `[heartbeat]` line flushes BEFORE the summary. DN-019's race (test failed 5/5 on apple-silicon) also forced restructuring `heartbeat_loop` to tick-first-wait-after: a fast ingest can finish + signal stop before the heartbeat thread is scheduled, leaving the first `wait_for_stop` to observe the signal immediately and return without printing. Tick-first guarantees one liveness signal per `interval`. Production behavior (interval=10s, ticks=100) unchanged. Test passes 10/10 (was 0/5). Also restored DN-018's section header that was accidentally deleted in commit `31781e4`. TD-003 → Closed; DN-019 → closed.

- `e6d53fb` — **`chore: gitignore .serena/ per-machine MCP state`**. Three-line `.gitignore` addition matching the existing `.eng-protocol/` / `.photohelper/` exclusions for per-machine runtime caches.

- `0d4a7f7` — **`chore(libraw): pre-flight EXIF + CVE-posture audit (Deliverable 0)`**. Probe via Homebrew `raw-identify -v` (LibRaw 0.22.1) against `/Users/ph/Pictures/tests` 370-CR3 corpus. Extraction pass-rate 370/370 (100%; Make + Model + Width + Height + Timestamp + Flip on every fixture). CVE-posture: MITRE NVD = 0 CVEs since 2023-01-01, LibRaw GHSA = 0 advisories. Decision: **PROCEED with `=0.22.1` pin** (escalated from plan-default `=0.21.4` because 0.22.1 ships six TALOS-2026-* fixes + two CR3-parser-specific hardenings that did NOT backport to 0.21.5b; user-consulted under No-Acceptable-Trade-offs Policy). Artifact at `docs/analysis/ANL-001-libraw-cr3-preflight.md`. Plan amended in-place to v3.2. DN-018 → closed. Commit message carries the required `cve-posture: clean (versus MITRE feed 2026-05-28)` + `pass-rate: 370/370 (>=95% threshold; 100% actual)` lines.

- `440388a` — **`chore(session-02): photohelper-raw lint scaffolding + unsafe-isolation gate (Deliverable 1a setup)`**. Foundation for the FFI body that lands next window. Three-layer defense:
  1. Crate `[lints.rust]` overrides workspace `forbid(unsafe_code)` to `allow` (priority 1); every workspace lint restated explicitly (Cargo's per-key override does not merge).
  2. `src/exif.rs` + `src/decode.rs` carry file-level `#![forbid(unsafe_code)]`; `src/ffi.rs` carries `#![deny(unsafe_op_in_unsafe_fn)]`. `src/lib.rs` does NOT carry a file-level forbid because rustc forbids downgrading `forbid` in submodules — plan v3.1 listed lib.rs in that group but it's incompatible with letting `ffi.rs` contain unsafe; plan v3.2 amendment corrects.
  3. `scripts/check-unsafe-isolation.sh` + new `just ci unsafe-isolation` recipe greps `crates/photohelper-raw/src/` for any `unsafe { ... }` / `unsafe fn` / `unsafe trait` / `unsafe impl` outside `ffi.rs`. Workspace clippy `undocumented_unsafe_blocks = "deny"` requires `// SAFETY:` on every unsafe block.

`src/{ffi,exif,decode}.rs` ship as empty module stubs carrying only the lint setup + module doc-comment. Body work (1d Error enum, 1b RawExif, 1c RawImage families, 1a FFI bindings) lands in next-window commits.

### CI gate state at pause time

`just session-end` (= `just ci`) GREEN end-to-end on apple-silicon. fmt clean, clippy zero-warnings, 63 tests passing (was 62/63 with the heartbeat flake before this window), cargo audit clean, `unsafe-isolation` gate clean, prek hooks all pass. Acceptance criterion 1 (`just ci` green) is now actually satisfiable; the remaining gating work is the LibRaw FFI implementation itself.

### Why paused

The Deliverable 1 body work is substantial (Error enum + RawExif + RawImage + 5 companion types + ~15 FFI function bindings; each with R2-T6 / R3-T5 / R3-T7 invariants the plan-review found). This window already absorbed the re-orientation reads (heavy SESSION-STATE + plan + R3 artifact + DN-019 + ANL-001 authoring), the heartbeat race investigation, the LibRaw version-pin investigation + user consultation, and the lint scaffolding + plan-design correction. Continuing into the FFI body without a fresh context would risk degraded design quality on the unsafe-heavy code. Pausing here so the next window opens fresh on Deliverable 1d → 1b → 1c → 1a body sequencing.

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** (canonical re-orientation per the resume prompt). The "Current session" + "Action" + "Status" blocks reflect this checkpoint.
2. **Read this Checkpoint 4** (you're here).
3. **Skim `docs/plans/session-02.md` §§ Deliverable 1d / 1b / 1c / 1a body** for the type-family invariants the plan locked across plan-review R1/R2/R3.
4. **Skim `docs/analysis/ANL-001-libraw-cr3-preflight.md § EXIF extraction § Field mapping`** for the LibRaw API → photohelper field correspondences (`libraw_get_iparams` → make/model/timestamp; `libraw_get_iwidth/iheight` → width/height; `imgdata.sizes.flip` → orientation). This is what the FFI module 1a will bind.
5. **Read `crates/photohelper-raw/src/{lib,ffi,exif,decode}.rs`** to see the current scaffolding. They're tiny — module docs + lint setup only.
6. **Begin Deliverable 1 body work in the plan's stated order**:
   - **1d Error enum** first (`crates/photohelper-raw/src/lib.rs`): `Error`, `RawExifCause`, `RawDecodeCause` per plan §1d; all `#[non_exhaustive]`; `thiserror::Error` derives; carries `path: PathBuf` + `op: &'static str` + `libraw_code: i32` per R2-T13. Add TD-007 inline addressing (constructor signatures take `path: &Path` first so the constructors can populate the field with the real path; closes the `PathBuf::new()` stop-gap). Add `Error::RawInvalidBitDepth { value: u8 }` variant per R3-T5.
   - **1b RawExif** (`src/exif.rs`): private-fields + fallible constructor + accessor methods; `static_assertions::assert_impl_all!(RawExif: Send, Sync)` at module scope. UTC timestamp assumption documented inline + cross-ref DN-016. Add `static_assertions` as a regular workspace dep (not dev-dep) in `crates/photohelper-raw/Cargo.toml`.
   - **1c RawImage + companions** (`src/decode.rs`): `BayerPlane` (length-invariant; `row(y) → Option<&[u16]>` / `pixel(x,y) → Option<u16>` / `rows() → impl Iterator`); `CfaPattern` (4-variant `#[non_exhaustive]`); `SensorBitDepth` (8..=16 constrained); `SensorLevels` (black<white, dynamic-range floor 256, bit-depth bound); `WhiteBalance` (R/G1/B/G2 Canon order; reject all-zero / NaN / negative); `CamRgbToXyzD65Matrix` (reject identity / NaN). Address TD-007 inline on each constructor.
   - **1a FFI body** (`src/ffi.rs`): ~15 LibRaw C-API accessor functions per plan §1a + `RawPath` newtype (interior-NUL / non-UTF-8 / Windows long-path handling). Every `unsafe { ... }` block carries `// SAFETY:` comment. `cargo clippy` enforces via `undocumented_unsafe_blocks = "deny"`.
7. **Sub-component review fires** when `ffi.rs` first exposes a non-scaffold public API (per plan § Checkpoints): `docs/code-reviews/session-02-photohelper-raw-ffi-round{1,2}.md`.
8. **Then Deliverables 2-7 + session-end double-review + ship PR**.

### Resume from a fresh context

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-02/libraw-cr3-decode
just session-start
```

Then paste the restart prompt rendered at the end of this pause turn.

---

## Checkpoint 5 — session 02 SHIPPED (2026-05-28; full LibRaw FFI for Canon R8 CR3)

**Status**: shipped. Session-02 GOAL fully met end-to-end on `session-02/libraw-cr3-decode`. PR open + merge pending CI green.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 02, continued from Checkpoint 4)

### What landed since Checkpoint 4 (9 additional commits)

* `a59ef66` — **Deliverable 1d Error enum**: `photohelper-raw::Error` + `RawExifCause` + `RawDecodeCause` (all `#[non_exhaustive]`; every variant carries `path` where applicable; `LibRawCallFailed { op, libraw_code }` discriminator for log-grep triage per R2-T13).
* `c42ce2f` — **Deliverable 1b RawExif types-only slice**: type + accessors, `assert_impl_all!(RawExif: Send, Sync)` at module scope.
* `8b6b9e8` — **Deliverable 1c RawImage + companions**: `BayerPlane` / `CfaPattern` / `SensorLevels` / `SensorBitDepth` / `WhiteBalance` / `CamRgbToXyzD65Matrix` with the R2-T6 invariants (dynamic-range floor 256, NaN/identity rejection, etc.); TD-007 closure via constructor-takes-path discipline; TD-008 filed for the transient `#[allow(dead_code)]` on constructors.
* `51905be` — **Deliverable 2 build-system + ADR-0002**: LibRaw 0.22.1 tarball vendored (`vendor/libraw-0.22.1.tar.gz` + SHA-256 sidecar). build.rs runs `./configure && make` (autoconf — not cmake; LibRaw 0.22.1 ships only autoconf scripts, the plan was amended in lockstep to v3.3). `cc::Build` compiles `cpp/photohelper_libraw_shim.c` against LibRaw headers from OUT_DIR. ADR-0002 LGPL §6(a) decision-doc as DRAFT.
* `092383f` — **Deliverable 1a-exif body**: FFI bindings (14 extern "C" + 8 C shim), `RawPath` newtype (NUL/non-UTF-8/empty rejection), `LibrawGuard` RAII handle, `parse_libraw_fields` orchestration, `RawExif::from_libraw_fields` + `read_cr3` public entry. Smoke-tested against `_MG_9625.CR3` (Canon EOS R8, 6022×4024, Normal).
* `f8238f4` — **Deliverable 1a-decode body**: `parse_libraw_image` + `read_raw` + `RawImage::new` + `cfa_pattern_from_filters` (LIBRAW_COLOR macro impl in Rust) + `bit_depth_from_white` derivation. **TD-008 closed** — every transient `#[allow(dead_code)]` removed. Smoke-tested: 6188×4120 RAW, RGGB, black=2047 white=16383 bit_depth=14, valid WB+matrix.
* `7907ca8` — **Deliverable 3 fixtures + sanitize-check + integration tests**: Git LFS initialized, two CC0 Canon R8 CR3 fixtures from raw.pixls.us committed under `tests/fixtures/cr3/`, exiftool-sanitized, `scripts/sanitize-check.sh` allow-list gate wired into `just ci`, `fixture_is_real_cr3` helper, 3 integration tests against real CR3 sensor data, GitHub Actions workflow updated with `lfs: true` + system deps + sanitize-check + unsafe-isolation gates. R3-T8 stage-2 embedded-preview re-check deferred via TD-009.
* `203f58d` — **Deliverable 4 atomic kamadak-exif removal + ingest LibRaw rewire**: `parse_exif` deleted; `parse_cr3_exif` wraps `photohelper_raw::exif::read_cr3`; `RAW_EXTS = ["cr3"]`; `kamadak-exif` workspace dep removed; `unused_crate_dependencies = "warn"` lint added (caught + removed unused `time` / `thiserror` / `photohelper-core` declarations in 5 stub crates); `CanonR8::make_model` updated to `("Canon", "EOS R8")` to match LibRaw's normalized model string. **DN-006 + DN-011 closed**: end-to-end acceptance test passes against the user's 370-CR3 corpus (was 0/371 ingested, now 370/371 ingested with `--strict` exit 0).
* `2323b6b` — **Deliverable 5 rusqlite partial bump**: 0.32 → 0.34 (plan target was 0.40; rusqlite ≥ 0.36 needs `libsqlite3-sys ≥ 0.38` which requires MSRV 1.92, ours is 1.88). TD-002 partial closure; revised remediation depends on MSRV bump.
* `63002e5` — **Deliverable 7 polish + Deliverable 6 deferral**: `KnownCamera::display_name()` + `Display` impl; `UpsertOutcome::#[non_exhaustive]` deliberately NOT added (cross-crate match in ingest would need wildcard; lands together with `InsertedWithPartialExif`). **TD-010 filed** for Deliverable 6 test infrastructure.

### What was deferred (filed as TDs with binding triggers)

* **TD-009** — `scripts/sanitize-check.sh` stage-2 embedded-preview re-check (R3-T8 stage 2).
* **TD-010** — Deliverable 6 test infrastructure as a coherent unit (poison_for_testing, R2-T18 4-WARN regressions, DN-008 6 rows, R2-T3 heartbeat env-var panic, R2-M8 silent ROLLBACK fix).
* **TD-011** — session-end 8-agent multi-agent review deferred to a focused follow-up session because of context-budget exhaustion at session end. Filed for a focused follow-up review session before any v0.1 Release tag.
* **Plan §4b–§4f ingest enhancements**: `ExifCompleteness` predicate + `partial_exif`/`cr3_exif_absent` counters + per-`RawExifCause` dispatch table + `IngestOutcome::InsertedWithPartialExif`. Lands together with TD-010 work.

### CI gate state at PR-creation time

* `just ci` GREEN on apple-silicon: fmt clean, clippy zero-warnings (with the new `unused_crate_dependencies` + `undocumented_unsafe_blocks` + the `unsafe-isolation` script gate), 118 workspace tests passing (incl. 3 integration tests against the new LFS CR3 fixtures), cargo audit clean, sanitize-check clean (2 fixtures pass), prek hooks pass.
* Acceptance criterion 2b smoke verified locally:
  ```
  $ photohelper ingest "$HOME/Pictures/tests" --strict
  walked: 371, ingested: 370, superseded: 0, already-catalogued: 0,
  unknown-camera: 0, no-exif: 0, mtime-anomalous: 0,
  skipped (non-RAW): 1, skipped (too-small): 0, errored: 0
  $ echo $?
  0
  ```

### Closure summary

Closes:
* **DN-006** (kamadak-exif can't parse CR3) → closed.
* **DN-011** (DN-006 extends to 370 real R8s) → closed.
* **DN-018** (LibRaw CVE-posture audit owner) → closed.
* **DN-019** (heartbeat test fails 5/5 on apple-silicon) → closed.
* **TD-003** (heartbeat .join) → closed.
* **TD-008** (decode constructor dead_code) → closed.
* **TD-002** (rusqlite stale) → partial (0.32 → 0.34; full closure needs MSRV bump).

Files new:
* **TD-009** (sanitize-check stage 2 embedded preview).
* **TD-010** (Deliverable 6 test infrastructure).
* **TD-011** (deferred session-end 8-agent review).

Plan amendments:
* **v3.2**: LibRaw pin escalated `=0.21.4` → `=0.22.1` (Deliverable 0).
* **v3.2**: `lib.rs` removed from file-level `#![forbid(unsafe_code)]` list (rustc forbid cannot be downgraded by submodule).
* **v3.3**: build.rs uses autoconf, not cmake (LibRaw 0.22.1 ships only autoconf scripts).
* **v3.3**: `unused_crate_dependencies` lint surfaces collateral cleanups in 5 stub crates' Cargo.toml.

### How to land

```bash
git push -u origin session-02/libraw-cr3-decode
gh pr create --base main --head session-02/libraw-cr3-decode \
    --title "session 02: LibRaw FFI for Canon R8 CR3 (EXIF + decode)" \
    --body "..."  # see PR for full body
gh pr checks --watch
gh pr merge --merge --delete-branch
```

Then render the two-block session handoff per `docs/session-handoff-format.md` in the PR review thread.

---

## Checkpoint 6 — session 02 + two operator sub-sessions all SHIPPED on main (2026-05-28)

**Status**: SHIPPED. PR #2 merged at `67bd882`; PR #3 merged at `2f22094`; PR #4 merged at `1d31316`. All three on `main`. Working tree clean. CI green.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, continuing from Checkpoint 5)

### What landed since Checkpoint 5

Two operator-requested follow-ups to session 02, each shipped as its own tiny sub-session PR per `CLAUDE.md § Branch-name convention` (decimal-suffix sub-sessions for tiny chores too small for a full session number):

* **PR #3 — `session-02.5/cleanup-catalog-script`** (merge `2f22094`).
  Adds `scripts/photohelper-clean-catalog.sh` (safe-by-default catalog wipe; dry-run unless `--yes`; refuses to clean directories without a `catalog.db` typo-guard) + `just clean-catalog <args>` recipe + README `Reset a catalog` quickstart section. Triggered by the user asking how to reset the catalog when the bare `rm -rf .photohelper` recipe was the only existing option.
* **PR #4 — `session-02.5/list-catalog-script`** (merge `1d31316`).
  Adds `scripts/photohelper-list-catalog.sh` (read-only SQLite inspector with four modes: `--list` default, `--count`, `--by-camera`, `--paths-only`; supports `--all` / `--limit N` / `--sort capture|path|ingested` / `--catalog <db-path>`) + `just list-catalog <args>` recipe + README `List ingested photos` quickstart section. Schema-sanity check refuses to query a DB without a `photos` table. Triggered by the user asking how to list already-ingested files.

### Discovery findings from these sub-sessions

* **Stub-subcommand documentation drift**: every stubbed CLI subcommand (`camera`, `cull`, `develop`, `export`, `run`, `models`) still emits `"planned for session 02"` even though session 02 shipped without implementing them. Plan-§7 polish committed `KnownCamera::Display` + `UpsertOutcome::#[non_exhaustive]` but missed this drift. Worth a one-line message update in each stub source file as session 03's first chore commit (very small).
* **Two-shell PATH drift footgun**: the user's interactive zsh hit `zsh: no such file or directory` for a script that exists on disk + main — most likely because their terminal's shell hadn't pulled the merged PR yet. The current Quickstart doesn't surface "remember to `git pull` after a sub-session merge if you're using both Claude Code and a separate terminal."

### Why I'm checkpointing now

User invoked `/session-pause` to roll the context window. The skill mandates ledger updates + commit + a restart-prompt block. The skill's literal "commit" step couldn't apply on `main` per CLAUDE.md's never-commit-to-main rule, so this commit lands as its own tiny `session-02.5/ledger-catchup-post-ship` PR — the smallest possible PR that records the post-ship audit trail.

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** — the Last-session / Action / Goal blocks now reflect session 02 + sub-sessions SHIPPED, not "READY TO SHIP".
2. **Read this Checkpoint 6** (you're here).
3. **Read `git log --first-parent main`** to see the merge history (`67bd882` session 02 + `2f22094` cleanup + `1d31316` list).
4. **Begin session 03** per the standard session-start protocol:
   ```bash
   git switch main && git pull --ff-only origin main \
     && git switch -c session-03/<kebab-slug> \
     && just session-start
   ```
   Author `docs/plans/session-03.md`, run plan-review, then implementation. See `SESSION-STATE.md § Goal` for the candidate scope (TD backlog or new feature work) and the open stub-subcommand drift item as a quick-win first commit.

---

## Checkpoint 7 — session 03 plan-review Round 1 complete (2026-05-28; PAUSED for context refresh)

**Status**: PAUSED. Branch: `session-03/ai-culling-skeleton`. Plan v1 committed at
`319a25d`; Round-1 artifact at `7fd1dea`. No implementation code. CI green on branch
(118 tests, no code changes from main).
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, session 03 plan-review window)

### What landed this window

Session 03 started per the eng-protocol:
- Branch `session-03/ai-culling-skeleton` created off `main` (up-to-date at `6e633e1`).
- `just session-start` → STATUS: ready.
- Scope chosen by user: **AI culling skeleton** end-to-end (NIMA real model + catalog
  v1→v2 migration + cull_scores/dup_groups + cull subcommand rewire + full TD-010
  closure + DN-020 stub-message fix).

`docs/plans/session-03.md` v1 authored and committed (`319a25d`):
- 7 deliverables (D0 pre-flight + D1 photohelper-ai + D2 catalog migration + D3 fixtures
  + D4 cull rewire + D5 TD-010 closure + D6 DN-020 fix + D7 docs).
- Scope rationale: decision-doc 0001 § Amendments binds session 03 to v1→v2 migration
  framework (honored in D2); TD-010 binding trigger fires (touches Catalog; honored in
  D5); DN-020 binding trigger fires (session-03 start; honored in D6).

Plan-review Round 1 ran full 8-agent suite (Cadence A Tier 5) against plan v1 +
9th-agent verifier against all 34 CRITICAL+HIGH findings:
- **43 themes total: 10 CRITICAL + 18 HIGH + 10 MEDIUM + 5 LOW**
- 0 hallucinated findings (9th agent: 26 verified, 8 drifted line-numbers, 0 no)
- Artifact committed at `7fd1dea` (`docs/code-reviews/session-03-plan-round1.md`)

### 10 CRITICAL themes requiring plan v2 remediation

1. **PR1-T1** — D6 targets non-existent files (`commands/{camera,...}.rs` don't exist;
   stubs live in `main.rs:127-130`); `stub()` only has "session 02" for `camera`; 30
   LoC estimate is 5× too high; message points at internal `SESSION-STATE.md`.
2. **PR1-T2** — D5c adds panic site to `heartbeat_loop` then immediately removes it in
   the same deliverable; contradicts TD-005's "production `heartbeat_loop` becomes
   panic-free." No panic site should ever land.
3. **PR1-T3** — `DN-022` cited 4× in plan as if it exists; `discovery-notes.md` ends
   at DN-021. Three separate scopes mapped to one phantom DN.
4. **PR1-T4** — `per ANL-001 § out-of-scope, the original SCUNet plan is in flux`
   fabricated; ANL-001 is LibRaw pre-flight with zero SCUNet content.
5. **PR1-T5** — ort `Session::run()` requires `&mut self`; `Session: Send + Sync` does
   NOT permit `&mut`-receiver calls from multiple rayon workers without a Mutex. The
   plan's "one session shared across worker threads" won't compile.
6. **PR1-T6** — `Scorer` trait referenced in D4 (`&dyn Scorer`) but never defined in
   D1. No method signature, no associated SLUG const, no object-safety analysis.
7. **PR1-T7** — §Stop-gap declarations says "None" but plan body acknowledges ≥3
   stop-gaps (bilinear demosaic, dup_groups ships empty, per-cull-run audit trail
   absent). No TD entries filed for any of the three.
8. **PR1-T8** — D2b end-to-end column reads "manual SQLite REPL inspection" — not a
   test per global testing standards. BLOCKS merge.
9. **PR1-T9** — D0 pre-flight sequencing fires AFTER D1's first scaffolding commit; if
   D0 ABORTs (CVE found), the ort dep + model file is already committed. Must invert:
   D0 → D1a (dep only) → D0 commits ANL-002 → D1d (model file).
10. **PR1-T10** — `LoadedModel`'s SHA-256 check on the ALREADY-CONSTRUCTED
    `ort::Session` cannot verify the session's provenance. Trust boundary inverted.
    Need `VerifiedModelBytes` type-state before `LoadedModel`.

### 18 HIGH themes (abbreviated)

- PR1-T11: `ort = "=2.0.X"` invalid Cargo syntax; 2.0 is still RC (use `=2.0.0-rc.12`).
- PR1-T12: `cull_scores` SELECT walks superseded photos; supersede semantics unspecified.
- PR1-T13: D4 per-photo error dispatch unspecified; all errors collapse to `errored`.
  TD-006 binding trigger fires (cull is `read_raw`'s first consumer).
- PR1-T14: `--strict cull` semantics: 6 cases unresolved ("TBD per plan-review").
- PR1-T15: `apply_pending` ROLLBACK-of-ROLLBACK silent; partial-failure behavior unspecified.
- PR1-T16: D1d hardcodes model filename before D0 has chosen the model.
- PR1-T17: Catalog schema-version gate logic incoherent with migration runner insertion.
- PR1-T18: D4 SELECT returns `id` blob; no spec for how `source_path` is obtained.
- PR1-T19: Migration framework for one migration; decision-doc 0001:129 says this is wrong.
- PR1-T20: Bilinear Rust demosaic planned when LibRaw already exposes `dcraw_process()`.
- PR1-T21: SLO "wall-clock < 30 min" is 60× looser than expected (actual ~30s with rayon).
- PR1-T22: `PRAGMA foreign_keys = ON` absent; v2 FK constraints are decorative.
- PR1-T23: D5b cites `catalog.rs:281` for R2-M8; actual site is `:304`.
- PR1-T24: TD-006/TD-007 binding-trigger status vs. session 03 scope unaddressed.
- PR1-T25: TD-011 3-session bound unacknowledged (session 03 = 1 of 3).
- PR1-T26: NIMA golden vector: no tolerance spec, no generation procedure.
- PR1-T27: `ModelRegistry` trait with one impl; `--model-path` both premature abstractions.
- PR1-T28: D3 sanitize-check on ONNX via `exiftool` technically wrong (ONNX is Protobuf).

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** (canonical re-orientation).
2. **Read this Checkpoint 7** (you're here).
3. **Read `docs/code-reviews/session-03-plan-round1.md`** — the canonical R1 artifact
   with all 43 themes. The §R2 watch-list at the bottom is the mandatory R2 checklist.
4. **Remediate all 10 CRITICAL + 18 HIGH items** in `docs/plans/session-03.md`:
   - PR1-T1: rewrite D6 to target `main.rs:127-130`; fix message to point at README.
   - PR1-T2: remove D5c's add-then-retire sequence; 5c-i = test-helper crate only; no
     panic site ever in `heartbeat_loop`.
   - PR1-T3: file DN-022/023/024 (or consolidated); remove phantom citations.
   - PR1-T4: drop the fabricated `per ANL-001 § out-of-scope` clause.
   - PR1-T5: specify ort concurrency model explicitly (Mutex / per-worker / run_async).
   - PR1-T6: define `Scorer` trait in D1c/D1e, OR make D4 take concrete `&Nima`.
   - PR1-T7: file TD entries for ≥3 stop-gaps; rewrite §Stop-gap declarations.
   - PR1-T8: replace D2b manual-REPL with automated integration test spec.
   - PR1-T9: fix D0 sequencing: D0 → D1a-dep-only → D0-ANL-002-commit → D1d-model-file.
   - PR1-T10: add `VerifiedModelBytes` type-state to D1b LoadedModel spec.
   - PR1-T11 through PR1-T28: address in the same remediation pass.
5. **Commit plan v2** (single conventional commit: `docs(session-03): plan v2 — R1
   remediation (closes NN CRITICAL + NN HIGH)`).
6. **Fire plan-review Round 2** using `/plan-review` or the `eight-agent-review` skill.
7. **Remediate Round 2 findings** → commit plan v3 → fire Round 3 if CRITICAL.
8. **Begin implementation** only after Round 2 (or Round 3) is clean.

### Resume from a fresh context

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-03/ai-culling-skeleton
just session-start
```

Then paste the restart prompt (see handoff block below).

---

## Checkpoint 8 — session 03 plan-review COMPLETE (2026-05-28; PAUSED for context refresh, implementation ready)

**Status**: PAUSED. Branch: `session-03/ai-culling-skeleton` at `5eb2bc1`. Plan-review ran 4 rounds to CLEAN. No implementation code yet. CI GREEN (118 tests).
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, session 03 plan-review rounds 2–4 window)

### What landed this window

This window completed the full session 03 plan-review protocol (Rounds 2–4), beginning where Checkpoint 7 paused (R1 complete, R1 remediation needed).

**11 commits on `session-03/ai-culling-skeleton`**:

- `dc95639` — **plan v2** (R1 remediation: 10 CRITICAL + 18 HIGH + 5 MEDIUM). Major structural changes: D0 resequenced first; no Migration trait (match arm); `dup_groups` deferred; no Scorer trait (concrete `&Nima`); `VerifiedModelBytes` type-state; per-worker ort Session; D5c restructured (no panic in `heartbeat_loop`); `read_raw_rgb` via LibRaw `dcraw_process`; 5 new TDs (TD-012–TD-016); 4 new DNs (DN-022–DN-025).
- `49e218b` — SESSION-STATE update (plan v2 committed, R2 pending)
- `e9a8e96` — **plan-review Round 2 artifact** (3 CRITICAL + 10 HIGH + 9 MEDIUM + 4 LOW). All 10 R1 watch-list items VERIFIED PASS. New CRITICALs: Python onnx CI breakage (T1), `force_heartbeat_panic_in_thread` unimplementable (T2), `CullStats` counter type unspecified (T3).
- `285675e` — **plan v3** (R2 remediation). Closed all 3 CRITICALs + 10 HIGHs + 9 MEDIUMs.
- `5911262` — **plan-review Round 3 artifact** (3 CRITICAL + 4 HIGH + 2 MEDIUM + 1 LOW). R3 watch-list 11/11 PASS. New CRITICALs: `NimaScore: Ord` without `Eq` compile error (T-α), D5e subprocess test structurally impossible (T-β), per-worker Session frequency unspecified (T-γ).
- `a9f7152` — **plan v4** (R3 remediation). Closed 3 CRITICALs + 4 HIGHs + 2 MEDIUMs.
- `da5478e` — plan v4 addendum: `run_cull` signature made D0-conditional on `Session::run` receiver type.
- `b158af2` — plan v4 fixup: removed stale `?` pseudocode duplicate; added `Nima::new` constructor to D1c; fixed `thread_local!` `Result` handling.
- `b239a38` — **plan-review Round 4 artifact — CLEAN** (0 CRITICAL + 0 HIGH; 2 MEDIUM resolved inline). R4 watch-list 7/7 PASS. Plan-review declared complete.
- `5eb2bc1` — SESSION-STATE update (plan-review complete, implementation ready)

### New TDs opened this window

- **TD-012**: LibRaw AHD demosaic stop-gap for NIMA preprocessing.
- **TD-013**: Per-cull-run audit trail absent from `cull_scores`.
- **TD-014**: ort RC pin `=<pin-from-D0>` requires upgrade to stable 2.0.0.
- **TD-015**: `--model-path` power-user override deferred from v0.1.
- **TD-016**: `HeartbeatStop` + `heartbeat_loop` duplicated in `cull.rs` (factor at third subcommand).

### New DNs opened this window

- **DN-022**: LibRaw demosaic algorithm selection for NIMA preprocessing.
- **DN-023**: `cull_scores.photo_id` ON DELETE CASCADE absent from v2 schema.
- **DN-024**: MobileCLIP dup-detection compute deferred to session 04+.
- **DN-025**: NIMA cross-platform score tolerance (apple-silicon vs Linux x86_64).

### Plan v4 key design decisions (canonical; binding for implementation)

1. **D0 is binding**: `Session::run` receiver type (confirmed at D0 §Threading semantics) determines D4 design: `&self` → one `Arc<Nima>` shared; `&mut self` → `thread_local!` per-worker construction.
2. **No `Migration` trait**: `apply_v1_to_v2()` match-arm approach. `SCHEMA_VERSION = 2`.
3. **`cull_scores` only in v2** (no `dup_groups`).
4. **`VerifiedModelBytes` wraps `Arc<[u8]>`**: `from_verified` takes borrow; workers clone cheaply.
5. **`NimaScore: Copy + Clone + PartialEq + Eq + PartialOrd + Ord`**: `Ord` via `f32::total_cmp`.
6. **D5c heartbeat-death test is in-process only** (not subprocess).
7. **`run_cull` signature D0-conditional**: two paths shown in plan; D0 picks one.
8. **`CullStats` has 8 `AtomicU64` fields** (explicitly enumerated including `derive_failed` + `catalog_inconsistency`).
9. **`PhotoId::derive` uses `match` in `for_each` closure** (not `?`).

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** (canonical re-orientation).
2. **Read this Checkpoint 8** (you're here).
3. **Skim `docs/plans/session-03.md` §Deliverables** to understand the implementation order: D6 (first chore) → D0 (pre-flight, binding) → D1a → D1b/c → D1d → D2a/b/c → D3 → D4 → D5 → D7.
4. **Skim `docs/code-reviews/session-03-plan-round4.md`** — the "plan-review declared complete" artifact (summary of all 4 rounds).
5. **Begin implementation**: first commit is `chore(cli): refresh stub-subcommand messages (closes DN-020)` targeting `main.rs:127-130` (the D6 first-chore per plan §D6).
6. **Then D0** (pre-flight): `chore(ai): pre-flight ort + NIMA audit (Deliverable 0)` — verify ort RC CVE-posture, verify NIMA license, run inference on CC0 fixtures, verify `Session::run` receiver type (the binding output that determines D4's signature and concurrency model).
7. **Sub-component reviews** fire after D1a+D1b+D1c land (photohelper-ai) and after D2a+D2b land (catalog migration).

### Resume from a fresh context

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-03/ai-culling-skeleton
just session-start
```

Then paste the restart prompt rendered at the end of this pause turn.

---

## Checkpoint 9 — session 03 implementation complete (2026-05-28; D0 ABORT path)

**Status**: IMPLEMENTATION COMPLETE (D0 ABORT path). Branch `session-03/ai-culling-skeleton` at HEAD. `just ci` GREEN. Session-end review (Round 1 + Round 2) pending.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, session 03 implementation window)

### Summary

Session 03 narrowed from its original scope (full AI culling pipeline) to D5+D6+D7 after D0 pre-flight ABORT. The AI culling pipeline (D1–D4: photohelper-ai crate, catalog v1→v2 migration, cull subcommand) is blocked by DN-026: no NIMA ONNX model with explicit MIT/Apache-2.0/CC-BY-4.0 license was found. See `docs/analysis/ANL-002-ort-nima-preflight.md` for the full pre-flight findings and two resolution paths for session 04+.

### What landed this window (12 commits, all conventional)

| Commit | Subject |
|---|---|
| D6 | `chore(cli): refresh stub-subcommand messages (closes DN-020)` — stub() rewritten to public message; justfile PATH fix |
| D0 | `chore(ai): pre-flight ort + NIMA audit (Deliverable 0) — ABORT` — ANL-002; DN-026 filed |
| D5a+D5b | `fix(catalog): D5a+D5b — poison_for_testing knob + silent-ROLLBACK fix` |
| D5d | `feat(cli)+test: D5d — DN-008 6 rows + fatal exit codes (EX_TEMPFAIL/EX_NOPERM)` |
| D5c | `feat(test-infra): D5c — photohelper-test-helpers crate + HeartbeatDeathTrigger` |
| D5e | `test(cli): D5e — R2-T18 WARN regression tests (file-lock op-tag + wal_checkpoint)` |
| D7 | `docs: D7 + TD-010 partial closure update` |

### Test count growth

118 tests at session start → 133 tests at session end (+15). Key additions:
- D5a: 3 catalog poison-recovery tests
- D5c: 1 HeartbeatDeathTrigger smoke test + 1 doc-test
- D5d: 7 CLI integration tests (hardlink dedup, strict+real-CR3, nested-dirs, broken-symlinks, future-mtime, EX_TEMPFAIL, EX_NOPERM)
- D5e: 2 WARN regression tests (file-lock op-tag, wal_checkpoint)
- D6: 1 negative test (cull --help)

### TD-010 closure status

- 6a (poison_for_testing): **CLOSED**
- 6b (ROLLBACK fix): **CLOSED** (extended_code==1, not ApiMisuse as plan cited)
- 6c (HeartbeatDeathTrigger crate): **CLOSED**
- 6d (DN-008 6 rows): **CLOSED**
- 6e rows 2+3 (wal_checkpoint + file-lock): **CLOSED**
- 6e rows 1+4 (build_global + heartbeat-death in-process): **DEFERRED** — needs in-process run_ingest() invocation, not subprocess. Updated TD-010 with concrete plan (~50 LoC).
- 6f (R2-T19): already closed at session 01 R2.

### D0 ABORT key findings (from ANL-002)

- **ort 2.0.0-rc.12**: CVE-clean (0 RustSec + 0 GitHub + 0 OSV.dev advisories as of 2026-05-28). Wraps ONNX Runtime 1.24. Rust MSRV = 1.88 (matches our toolchain).
- **Session::run receiver**: `&mut self` (confirmed from pykeio/ort source). Binding for D4: per-worker `thread_local!` path is the correct concurrency model.
- **NIMA ONNX model**: No model found with explicit MIT/Apache-2.0/CC-BY-4.0 license. Only candidate (`cromsc/nima-mobilenet-aesthetic` on HuggingFace) has no license file, no model card, no provenance documentation. ABORT condition fires.

### New TDs/DNs this window

- **DN-026**: No NIMA ONNX model with explicit permissive license found — BLOCKER for D1–D4.
- **TD-010**: PARTIALLY CLOSED (see above).

### What is not yet in place

- AI culling pipeline (D1–D4): blocked by DN-026. Resolution paths in ANL-002.
- catalog v1→v2 migration + `cull_scores` table (D2): blocked by D0 ABORT.
- 2 remaining TD-010 in-process WARN tests.
- Session-end 8-agent review (Round 1 + Round 2).

### How to resume

```bash
git switch session-03/ai-culling-skeleton
just session-start
```

Then fire the session-end review (`/eight-agent-review` or `eight-agent-review` skill), remediate, update STATE + HANDOFF, push, PR, merge.

---

## Checkpoint 10 — session 04 paused for context refresh (2026-05-29; D1e pending)

**Status**: PAUSED. Branch: `session-04/ai-culling-pipeline`. `just ci` GREEN (133 tests).
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, session 04 implementation window 1)

### What landed this window

**Plan-review** (before this implementation window):
- Plan v1 committed; R1 ran (6 CRITICAL + 13 HIGH + 10 MEDIUM + 3 LOW) → plan v2
- R2 ran (3 HIGH + 5 MEDIUM + 2 LOW) → plan v3 (CLEAN); implementation unblocked

**Implementation commits** (D0'→D1d):

| Commit | Subject |
|---|---|
| `scripts/verify-nima-d0-prime.sh` | D0': verify NIMA inference on CC0 CR3 fixtures |
| D0' commit | ANL-002 addendum + DN-026 CLOSED (scores [3.7377, 3.9253]; 0.62s/photo; deterministic) |
| D1a | Wire `ort =2.0.0-rc.12` dep + download-binaries + tls-native |
| D1b+D1c | `RgbImage` in photohelper-core; `VerifiedModelBytes`, `NimaScore`, `Nima`, `Error` in photohelper-ai |
| D1d | NIMA ONNX model via Git LFS + `scripts/verify-model-sha256.sh` + `just verify-model-sha256` |

Also: `scripts/convert-nima-to-onnx.sh` committed (pre-session-04 tool that resolved DN-026).
DN-024 (dedup/MobileCLIP) escalated to session 05 per user request.

### Key implementation details (canonical; binding for D1e-D3)

**D0' scores** (Canon R8 CC0 fixtures):
- `CRAW_FULL_FRAME.CR3`: aesthetic_score = **3.7377**
- `RAW_FULL_FRAME.CR3`: aesthetic_score = **3.9253**
- CI band (Linux x86_64): `score ∈ [1.74, 5.74]` (score ± 2.0, clamped to [1, 10])
- Apple Silicon tolerance: ±1e-3 (deterministic; delta=0.00 confirmed)

**ort API notes** (verified empirically during D1c):
- `Session` type: `ort::session::Session` (NOT re-exported at crate root in 2.0.0-rc.12)
- Session construction: `ort::session::Session::builder()?.commit_from_memory(&bytes)?`
- Input tensor: `ort::value::Tensor::<f32>::from_array(([1_usize,224,224,3], boxed_slice))?`
- Session inputs: `ort::inputs!["name" => tensor]` → `Vec<(Cow<str>, SessionInputValue)>` (NOT a Result)
- Session outputs: `outputs.get("output_name")` returns `Option<&DynValue>`
- Output extraction: `value.try_extract_tensor::<f32>()` → `Result<(&Shape, &[f32])>`
- Input/output names: `sess.inputs().first().map_or_else(...)` (`.inputs()` is a method, not a field)
- Borrow issue: extract input/output names to `String` BEFORE `sess.run()` to avoid borrow conflict

**ort feature flags required** (workspace Cargo.toml):
```toml
ort = { version = "=2.0.0-rc.12", default-features = false, features = ["std", "ndarray", "download-binaries", "tls-native"] }
```

**thread_local! pattern in Nima::score**:
- `if guard.is_none() { match builder.commit_from_memory(...) { Ok(s) => *guard = Some(s), Err(e) => return Err(Error::ModelLoad{...}) } }`
- `let sess = guard.as_mut().unwrap(); // #[allow(clippy::unwrap_used, reason="proven Some")]`
- Session stays `None` on construction failure so next photo retries

**RgbImage** is in `photohelper_core::model` (NOT photohelper-ai) — plan PR1-T7 remediation.
Both `photohelper-raw` (producer) and `photohelper-ai` (consumer) import from `photohelper-core`.

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** (canonical re-orientation).
2. **Read this Checkpoint 10** (you're here).
3. **Read `docs/plans/session-04.md § D1e`** for the FFI extension spec:
   - Three new `unsafe extern "C"` bindings: `libraw_dcraw_process`, `libraw_dcraw_make_mem_image`, `libraw_dcraw_clear_mem`
   - Six C-shim accessor functions for `libraw_processed_image_t` fields (in `cpp/photohelper_libraw_shim.c`)
   - `pub fn read_raw_rgb(path: &Path) -> Result<RgbImage, Error>` in `crates/photohelper-raw/src/decode.rs`
   - Integration test: `read_raw_rgb_cc0_fixture` with len check + mean∈(20,240) + std_dev>5
4. **Implement D1e** (the FFI bindings are the blocking work; ~100 LoC in ffi.rs + C shim + decode.rs)
5. **Then D2a** (`apply_v1_to_v2` migration + `cull_scores` table + SCHEMA_VERSION=2 + FK PRAGMA)
6. **Then D2b** (`Catalog::unsuperseded_unscored_rows` + `insert_cull_score` with changes() mechanism)
7. **Sub-component review** (D2b boundary: `docs/code-reviews/session-04-catalog-migration-round{1,2}.md`)
8. **Then D3** (`run_cull` + `cull` subcommand + `CullStats` 10 fields + heartbeat + tests)
9. **Session-end R1+R2** when D3 is complete.

### Resume from a fresh context

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-04/ai-culling-pipeline
just session-start
```

Then paste the restart prompt.

---

## Checkpoint 11 — session 04 SHIPPED (2026-05-29; full AI culling pipeline)

**Status**: SHIPPED. Session-04 GOAL fully met end-to-end.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 04)

### What landed this session

**D0'**: Re-ran NIMA inference verification after DN-026 closure. Scores:
CRAW_FULL_FRAME.CR3 = 3.7377, RAW_FULL_FRAME.CR3 = 3.9253. ANL-002 addendum.
DN-026 CLOSED.

**D1a**: Wired `ort =2.0.0-rc.12` dep with `download-binaries` + `tls-native`.

**D1b+D1c**: `RgbImage` in photohelper-core (breaks ai↔raw circular dep). In
photohelper-ai: `VerifiedModelBytes` (SHA-256 manifest verification), `NimaScore`
(f32 newtype, range [1,10], Ord via total_cmp), `Nima` (thread_local! per-worker
ort::Session). `MODEL_SLUG` + `MODEL_MANIFEST_NAME` constants.

**D1d**: NIMA ONNX model in Git LFS (Apache-2.0 converted from idealo/image-
quality-assessment). `scripts/verify-model-sha256.sh` + `just verify-model-sha256`
CI gate.

**D1e**: `read_raw_rgb(path) → Result<RgbImage, Error>` via LibRaw dcraw_process
pipeline. 3 new FFI bindings (dcraw_process, make_mem_image, clear_mem) + 6 C
shim accessors for libraw_processed_image_t. Integration test verifies both CC0
CR3 fixtures: dim invariant + mean∈(20,240) + std_dev>5.

**D2a**: Catalog schema v2 — `cull_scores` table (photo_id FK, model_slug,
aesthetic_score REAL [1,10], scored_at_unix_seconds). `PRAGMA foreign_keys = ON`.
`apply_v1_to_v2` migration (idempotent DDL). Decision doc 0002 + 0001 amendment.
`SCHEMA_VERSION = 2`. 3 new catalog unit tests.

**D2b**: `CullRow` (PathBuf source_path, private fields + accessors — Theme-A fix
from sub-component R1). `InsertScoreOutcome` (Inserted | AlreadyScored).
`Catalog::unsuperseded_unscored_rows(model_slug)` (SQL NOT IN filter, ORDER BY).
`Catalog::insert_cull_score(photo_id, model_slug, f64, i64)` (INSERT OR IGNORE +
changes(), range guard). Sub-component review R1+R2 complete; D2b has 5 catalog
tests.

**D3**: `crates/photohelper-cli/src/commands/cull.rs` — full `run_cull` pipeline
(exists-check → derive → decode → NIMA → persist), `CullStats` (9 AtomicU64
fields), `CullArgs` (`--strict`), heartbeat (TD-016 stop-gap). `main.rs` wired
with `PHOTOHELPER_MODEL_DIR` env var + `MODEL_MANIFEST_NAME` constant.
3 CLI integration tests: real CC0 fixture end-to-end, strict decode-fail exit code,
idempotency (second run walks 0 rows — SQL filter verified).

**Session-end review R1+R2**: R1 surfaced 1 CRITICAL + 4 HIGH + 9 MEDIUM; all
remediated. R2 was clean (0 findings, all 13 watch-list items closed).

### Test count
133 (baseline) → **143** (+10: 1 raw integration, 3 catalog unit, 3 CLI integration,
3 other catalog from sub-component review).

### TDs closed / filed this session

Closed: DN-026 (NIMA ONNX model license — resolved by converting idealo model to
ONNX under Apache-2.0). TD-016 status updated from "prospective" to "Open
(materialized)".

Filed: TD-012 in-source comment added to read_raw_rgb. TD-013 in-source comment
added to insert_cull_score.

### What is not yet in place

- D3 `--dry-run` (TD-015 deferred)
- Duplicate-detection pipeline (DN-024 → session 05)
- `develop` / `export` / `watermark` subcommands
- Windows build verification + release engineering

### How to resume (session 05)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-05/<kebab-slug>
just session-start
```

---

## Checkpoint 12 — post-session-04 script fixes (2026-05-29; context refresh)

**Status**: main is clean (PR #7 merged). Two uncommitted script improvements
committed to main as a post-ship chore.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, post-session-04 window)

### What landed this window

After session 04 shipped, the user ran the pipeline manually and hit two issues:

1. **`photohelper-cull.sh` missing `PHOTOHELPER_MODEL_DIR`**: The script ran
   `cargo run ... cull` without the env var, so the binary looked for the model
   at `target/debug/models/manifest.toml` (binary-adjacent), not in
   `crates/photohelper-ai/models/`. Fixed by exporting
   `PHOTOHELPER_MODEL_DIR="$ROOT_DIR/crates/photohelper-ai/models"` in the script.

2. **`photohelper-list-catalog.sh` no score column**: The list script only queried
   `photos`, not `cull_scores`. Added `--sort score` support (ORDER BY
   `cs.aesthetic_score DESC`) and a `score` column (LEFT JOIN `cull_scores`
   on `model_slug = 'nima-aesthetic-v1'`; shows `printf('%.4f', …)` or `-` for
   unscored photos). Usage updated in script header and `print_usage()`.

### Manual verification

User ran the full pipeline end-to-end:
- `scripts/photohelper-ingest.sh ~/Pictures/tests` → 370 Canon R8 CR3s ingested
- `scripts/photohelper-cull.sh --catalog ~/Pictures/tests/.photohelper/catalog.db`
  → `walked: 370, scored: 370, decode-failed: 0` (all passed, ~30s wall-clock)

### Next steps (session 05)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-05/<kebab-slug>
just session-start
cat SESSION-STATE.md
```

Candidate scope for session 05: duplicate-detection pipeline (DN-024 MobileCLIP)
or `develop` pipeline start (XMP sidecar I/O). Review open TDs and DNs at
session start to decide.

---

## Checkpoint 13 — session 05 PAUSED for context refresh (2026-05-29; plan v1 committed, plan-review pending)

**Status**: PAUSED. Branch: `session-05/dedup-mobileclip`. `just ci` GREEN (143 tests). No code changes from main.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, session 05 opening window)

### What landed this window

Session 05 started per the eng-protocol:
- Branch `session-05/dedup-mobileclip` created off `main` (up-to-date).
- `just session-start` → STATUS: ready.
- Session-04 Round-2 review confirmed CLEAN (0 findings; all 13 watch-list items closed).
- Scope chosen by user: **DN-024 duplicate-detection pipeline** (MobileCLIP embeddings + dup_clusters).

**Plan v1 committed** (`docs/plans/session-05.md`):

Plan covers 5 deliverables (D0 pre-flight ABORT gate → D1 MobileCLIP in photohelper-ai →
D2 catalog v2→v3 migration → D3 dedup subcommand → D4 TD-016 closure → D5 docs).

Key design decisions locked in plan v1:
1. **D0 ABORT gate first** — MobileCLIP weight license must be explicit (MIT/Apache-2.0/CC-BY-4.0). Search order: `apple/ml-mobileclip` → community ONNX export → `openai/clip-vit-base-patch32` fallback.
2. **thread_local! per-worker ort Session** (same as Nima) — expected since Session::run is &mut self.
3. **Schema v3**: `embeddings` (photo_id, model_slug, dim, quantization, embedding BLOB, PRIMARY KEY (photo_id, model_slug)) + `dup_clusters` (cluster_id, photo_id, model_slug, similarity_threshold, FK to embeddings) + `apply_v2_to_v3` idempotent migration.
4. **Cosine-similarity threshold clustering** via union-find (O(n²) pairwise comparisons; stop-gap S1/TD-017). Warn at n > 5K.
5. **TD-016 fires** — dedup is the 3rd heartbeat consumer (ingest + cull + dedup) → `heartbeat.rs` extraction is mandatory in D4.
6. **TD-010 also fires** — D4 touches `ingest.rs` → remaining 2 sub-items (build_global WARN + heartbeat-death-WARN) close in D4.
7. **3 new stop-gaps declared**: S1 (O(n²) clustering / TD-017), S2 (f32 BLOB / TD-018), S3 (no dedup_runs audit trail / TD-019).
8. **Target**: ≥ 163 tests (143 + 20 minimum).

### Why paused

Plan-review Round 1 was attempted (8-agent suite launched in parallel) but all 4
initial agents hit network errors ("API Error: Unable to connect") simultaneously.
Network connectivity issue on the host. No findings were produced. Pausing to
save state and retry plan-review in a fresh context window.

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** (canonical re-orientation).
2. **Read this Checkpoint 13** (you're here).
3. **Read `docs/plans/session-05.md`** — the full plan v1 (519 lines). Understand
   the D0→D1→D2→D3→D4→D5 sequencing and all design decisions.
4. **Fire `/plan-review` on `docs/plans/session-05.md`**:
   - Run the 8-agent suite in parallel (Round 1).
   - Consolidate by theme, triage by severity.
   - Run 9th-agent verification.
   - Write `docs/code-reviews/session-05-plan-round1.md`.
   - Remediate all CRITICAL + HIGH items in the plan.
   - Commit plan v2.
   - Fire Round 2.
   - Remediate Round 2 findings → plan v3 if CRITICAL.
   - Begin implementation ONLY after Round 2 is clean.
5. **Do NOT begin implementation** until plan-review Round 2 (at minimum) is clean.

### Resume from a fresh context

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-05/dedup-mobileclip
just session-start
```

Then paste the standard restart prompt.

---

## Checkpoint 14 — session 05 PAUSED for context refresh (2026-05-29; D0–D4 complete, D5 pending)

**Status**: PAUSED. Branch: `session-05/dedup-mobileclip`. `just ci` GREEN (183 tests). D0-D4 fully implemented.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code, session 05 implementation window)

### What landed this window (D0–D4 commits)

This was a substantial implementation window. All D0-D4 deliverables from the plan shipped:

**D0 Pre-flight** (commit `...`):
- MobileCLIP (apple/ml-mobileclip) failed license gate — `apple-amlr` is proprietary. ABORT condition for MobileCLIP.
- Fallback: `laion/CLIP-ViT-B-32-laion2B-s34B-b79K` (MIT) selected.
- ONNX export via `scripts/convert-clip-to-onnx.sh` (OpenCLIP MIT → int8 quantized).
- **Model**: `clip_vit_b32_laion2b_int8.onnx` (85.3 MB, single file, commit_from_memory compatible).
- SHA-256: `09361948663aa58d62cdaee26c291e913d6d87c35b199c15115aeb4f6c1bd508`
- Inference: dim=512, norm=1.0, cosine_sim(CRAW,RAW)=0.843 (bilinear resize, TD-020 stop-gap), wall-clock=0.96s/photo.
- TD-020 filed (bicubic center-crop deferred). DN-027 (cross-platform tolerance) filed.
- ANL-003 committed.

**D1a**: `ImageEmbedding(Arc<[f32]>)` — from_raw (is_finite() guard for NaN), cosine_similarity, as_f32_le_bytes, from_f32_le_bytes (EmbeddingCorruptBytes for misaligned bytes). 6 unit tests + `static_assertions::assert_impl_all!(Send, Sync)`.

**D1b**: verify-model-sha256.sh extended to loop over ALL manifest sections (covers NIMA + CLIP). CLIP model in Git LFS.

**D1c**: `MobileClip { bytes: Arc<[u8]> }` — embed(&self, rgb, path) → ImageEmbedding. NCHW preprocessing, CLIP-standard normalization, thread_local! per-worker ort Session. EmbeddingZeroVector guard. tracing::error! on model load failure. 3 integration tests (in photohelper-raw/tests/).

**D1d sub-component review** (R1: 1C+2H+5M+7L; R2: 0C+0H — CLEAN). Key fix: `extract_field` had a `?` early-exit bug (swallowed prefix matches) — discovered by new unit tests.

**D2a**: Schema v3 — `embeddings` + `dup_clusters` tables, `apply_v2_to_v3`, migration chain (0→3, 1→3, 2→3). Decision doc `docs/decisions/0003-catalog-schema-v3.md`. TD-019 filed. 4 catalog tests (idempotency, chain v1→v3, FK enforcement, schema_version_too_new gate).

**D2b**: Catalog API — `EmbeddingRow`, `InsertEmbeddingOutcome`, `unembedded_rows`, `insert_embedding` (with Rust-level dim guard — INSERT OR IGNORE swallows CHECK violations!), `all_embeddings_for_model` (returns dim alongside bytes), `insert_dup_cluster`. 7 catalog tests.

**D2c sub-component review** (R1: 1C+2H+3M+3L; R2: 0C+0H — CLEAN). Key discoveries: INSERT OR IGNORE swallows CHECK violations (dim guard added); FK violations DO propagate despite OR IGNORE (per SQLite docs); TD-017+TD-018 were missing from TECH-DEBT.md (filed).

**D3**: `crates/photohelper-cli/src/commands/dedup.rs`:
- `DedupeArgs` with `parse_similarity_threshold` value_parser
- `DedupeStats` (9 AtomicU64)
- `run_dedup`: Phase 1 (embed via rayon into_par_iter) + Phase 2 (cluster)
- `threshold_cluster`: union-find with path compression + union-by-rank, O(n²)
- TD-017 + TD-019 in-source labels at this commit
- `scripts/photohelper-dedup.sh` + `just dedup`
- 3 integration tests: end-to-end, idempotency (walked:0), strict-mode (file-missing:1)

**D4**: `crates/photohelper-cli/src/heartbeat.rs` extracted:
- `HeartbeatStop`, `heartbeat_interval()`, `run_heartbeat_loop(on_tick: Fn())`
- `spawn_dying_heartbeat` test seam (`#[cfg(test)]`)
- `ingest.rs` + `cull.rs` + `dedup.rs` all import from `heartbeat.rs`
- TD-016 → **CLOSED**
- TD-010 → **CLOSED**: `build_global_already_initialized_warns_but_succeeds` + `spawn_dying_heartbeat_panics_and_join_returns_err` in `ingest.rs::td010_tests`

**Test count**: 143 (baseline) → **183** (+40):
- +6 ImageEmbedding unit tests (D1a)
- +3 CLIP integration tests in photohelper-raw (D1c)
- +7 catalog unit tests (D2a+D2b)
- +3 dedup integration tests (D3)
- +2 TD-010 in-process tests (D4)
- +various catalog remediation tests from D2c review

### TDs filed/closed this window

**Filed:**
- TD-017: O(n²) union-find clustering (D2c review + D3)
- TD-018: f32 BLOB quantization (D2b)
- TD-019: no dedup_runs audit trail (D2a)
- TD-020: bilinear resize stop-gap for CLIP (D0)

**Closed:**
- TD-016: heartbeat duplication → heartbeat.rs extracted (D4)
- TD-010: 2 remaining in-process tests (D4)

### DNs opened this window

- DN-027: MobileCLIP cross-platform embedding tolerance for cosine-similarity clustering
- DN-028: MobileCLIP `apple-amlr` license blocks direct use

### Precise next steps when context restored

1. **Read `SESSION-STATE.md`** (canonical re-orientation).
2. **Read this Checkpoint 14** (you're here).
3. **Run `/session-end`** — this fires the full 8-agent double-review (R1 → remediate → R2) against all session code, updates SESSION-STATE.md + HANDOFF_REPORT.md, runs `just ci`, opens the PR, waits for green CI, and merges.
4. If session-end R1 surfaces CRITICAL findings that require substantial code changes, remediate inline before R2.
5. After merge: start session 06 per standard protocol (`git switch main && git pull --ff-only && git switch -c session-06/<slug> && just session-start`).

### Resume from a fresh context

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-05/dedup-mobileclip
just session-start
```

Then paste the standard restart prompt.

---

## Checkpoint 15 — session 05 SHIPPED (2026-05-29; full dedup pipeline + session-end review)

**Status**: SHIPPED. PR #8 opened; CI pending merge.
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 05, session-end window)

### What landed since Checkpoint 14 (D5 + session-end review)

**D5 (ledger)**: DN-024 closed. TD-017/TD-015/TD-018 stale text fixed. TD-019 label corrected.
SESSION-STATE catalog component updated to sessions 01+04+05.

**Session-end R1** (`docs/code-reviews/session-05-round1.md`): 19 findings; 15 retained
(discard_rate=0.053). 2C+3H+9M+1L. Key findings:
- CRITICAL A: `insert_embedding` range-checked dim but not `dim*4==bytes.len()`; `dedup.rs`
  discarded `_dim`; docstring promised non-existent caller validation.
- CRITICAL B: `threshold_cluster` had zero unit tests (plan promised 5).
- HIGH C: `MobileClip` SESS module-scoped thread_local (cross-contamination risk).
- HIGH D: `cosine_similarity` Err silently swallowed in clustering loop.
- HIGH E: Phase 2 empty/corrupt set: no log, no counter.

**R1 remediation**: `insert_embedding` `dim*4==bytes.len()` guard; `all_embeddings_for_model`
JOINs photos (excludes superseded); 6 threshold_cluster unit tests; `INSTANCE_EXISTS` guard
in `MobileClip`; `if let Ok(sim)` → match+error; `deserialize_failed` counter; `catalog_inconsistency`
split into `already_embedded` + `catalog_insert_failed`; all 9 MEDIUM items closed.

**Session-end R2** (`docs/code-reviews/session-05-round2.md`): 0 findings; 8/8 CLOSED. CLEAN.

**Final test count**: 182 (176 pre-R1 + 6 new threshold_cluster unit tests).
`just ci` GREEN: fmt, lint, tests, audit, unsafe-isolation, sanitize-check, model SHA-256.

### What is not yet in place

TD-012 AHD demosaic, TD-014 ort stable, TD-017 O(n²) clustering, TD-018 quantization,
TD-020 CLIP bicubic, develop/export/watermark subcommands, release engineering.

### How to resume (session 06)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-06/<slug>
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 16 — session 06 SHIPPED (2026-05-30; TD cleanup + develop pipeline + session-end review)

**Status**: SHIPPED. PR #9 opened; CI pending merge.
**Author**: Antigravity session 06, session-end window

### What landed since Checkpoint 15 (D0-D5 + session-end review)

**D0-D2 (TD Cleanup)**: Formally closed 7 Technical Debts:
- TD-001: Pinned all GitHub Actions to commit SHAs.
- TD-004: Configured OSV-Scanner for LibRaw CVE vulnerability monitoring.
- TD-005: Formally closed env-var panic (removed in session 05).
- TD-009: Ported `sanitize-check.sh` stage-2 to use portable macOS/BSD `mktemp`.
- TD-011: Fixed all session-02 post-hoc review findings (+7 tests, Round 1 & Round 2 CLEAN).
- TD-014: Set up `ort` stable cargo monitoring.
- TD-020: Optimized and center-cropped MobileCLIP preprocessing.

**D3-D4 (Develop Pipeline)**:
- Designed and implemented the complete `photohelper-sidecar` crate for robust, atomic XMP sidecar reading/writing with Camera Raw (`crs:`) and photohelper (`ph:`) custom namespaces.
- Developed the `DevelopRow` catalog projection and the `develop` subcommand to process raw files and write corresponding `.xmp` sidecar files natively in Lightroom Classic-compliant locations.

**Session-end R1** (`docs/code-reviews/session-06-round1.md`): 14 findings; 14 retained (discard_rate=0.00). 3C+7H+2M+2L.
- Key findings: Theme A (Lightroom overwrite clobber / rigid namespaces), Theme B (Temp file leak), Theme C (SQL iteration error swallowing), Theme D (macOS `mktemp` template suffix), Theme J (Thread-local Session reuse vulnerability).
- Remediation: Fully resolved all 14 findings, verified inline, and added 2 new tests to ensure total correctness of conflict preservation.

**Session-end R2** (`docs/code-reviews/session-06-round2.md`): 0 findings; all 9/9 watch-list items CLOSED. CLEAN.

**Final test count**: 223 tests.
`just ci` GREEN: fmt, lint, tests, audit, unsafe-isolation, sanitize-check, model SHA-256.

### What is not yet in place

- TD-012 AHD demosaic, TD-017 O(n²) clustering, TD-018 f32 BLOB quantization, export/watermark subcommands, release engineering.
- DN-029: Lightroom custom namespace incompatibility (mapped out for session 07).

### How to resume (session 07)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-07/lightroom-namespace-compatibility
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 17 — session 07 SHIPPED (2026-05-30; Lightroom compatibility + custom namespace resolution)

**Status**: SHIPPED. PR #10 opened; CI pending merge.
**Author**: Antigravity session 07, session-end window

### What landed since Checkpoint 16 (D1-D5 + session-end review)

**Lightroom Classic Custom Namespace Compatibility (DN-029)**:
- Designed and implemented complete Lightroom Classic-supported field mappings for NIMA aesthetic culling scores and duplicate cluster IDs inside `photohelper-sidecar`.
- Star ratings (`xmp:Rating`) represent rating states, where unrated (score 0) is natively omitted to prevent attribute clutter and rejected is represented as `-1`.
- Color labels (`xmp:Label`) map to standard `"Red"` and `"Green"` values with explicit empty string `""` support to clear pre-existing labels on deep merge.
- Flat keywords (`dc:subject`) and hierarchical keywords (`lr:hierarchicalSubject`) are safely merged and written under a single root `"photohelper"` tag list, avoiding keyword catalog pollution.
- Upgraded the XMP sidecar parser with prefix-agnostic namespace parsing, lenient decimal-formatted numeric string conversion, non-finite score handling via out-of-range SQL numerical literal `9e999` evaluation, and safe Temperature (`[2000, 50000]`) and Tint (`[-150, 150]`) slider clamping.
- Resolved write race hazards during Rayon parallel execution by deduping target paths and using thread-unique temporary files.
- Added three comprehensive sidecar parsing and validation unit tests.
- Closed TD-023 (pinning `time` crate dependency strictly to `=0.3.47` in `Cargo.toml`).

**Final test count**: 226 tests.
`just ci` GREEN: fmt, lint, tests, audit, unsafe-isolation, sanitize-check, model SHA-256.

### What is not yet in place

- TD-012 AHD demosaic, TD-017 O(n²) clustering, TD-018 f32 BLOB quantization, export/watermark subcommands, release engineering.

### How to resume (session 08)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-08/export-integration
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 18 — session 08 SHIPPED (2026-05-30; export integration completed and verified)

**Status**: SHIPPED. PR #11 merged to main.
**Author**: Antigravity session 08, session-end window

### What landed since Checkpoint 17

**Export Integration and Crate Implementation**:
- Fully implemented the `photohelper-export` crate for aspect-ratio-aware high-fidelity image resizing (using `tiny-skia` with safe demultiplication), robust watermarking (using `cosmic-text` with custom embedded RobotoMono font loading bypassing system directory scans), and safe high-performance MozJPEG encoding.
- Resolved the cosmic-text default font panic by replacing the corrupt placeholder HTML font with a valid, optimized, Google-Fonts-sourced TrueType RobotoMono binary file.
- Wired up the `export` CLI subcommand to run in parallel using Rayon, complete with a clean unique-suffix filename collision prevention map calculated upfront on the main thread.
- Standardized all CLI options, clippy warnings, and integer cast safety workspace-wide.
- All 236 tests, formatting checks, clippy, and security audit checks are 100% green.

### What is not yet in place

- BUG-001 (Lightroom Classic Metadata Sync Gaps), TD-012 AHD demosaic, TD-017 O(n²) clustering, TD-018 f32 BLOB quantization.

### How to resume (session 09)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-09/lightroom-sync-fixes
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 19 — session 09 SHIPPED (2026-05-31; Lightroom metadata syncing improvements and conflict shield)

**Status**: SHIPPED. PR #12 merged to main.
**Author**: Paulo Henrique Lerbach Rodrigues (Antigravity session 09, session-end window)

### What landed since Checkpoint 18

**BUG-001 Mitigation & Lightroom Classic Metadata Sync Gaps**:
- **Smart CLI Warnings & Shorthand**: Added `--all-lr` shorthand that automatically activates all three metadata fields (`lr_rating`, `lr_label`, `lr_keywords`). Added a highly visible startup warning to `stderr` if `develop` is run with no metadata fields active.
- **High-Performance Granular Conflict Logging**: Shifted individual parallel sidecar skip logging from `tracing::warn!` to `tracing::info!`/`tracing::debug!` to completely avoid parallel terminal lock contention. Added a consolidated warning on `stderr` exactly once at the end of the run if conflicts were preserved.
- **Type-Safe Localized Color Label Customization**: Replaced ad-hoc CSV string configurations with separate `--lr-label-red` and `--lr-label-green` arguments with upfront XML safety, non-empty, and distinct validation in `run_develop` before starting threads or catalogs.
- **mtime-Based Fallback Conflict Shield**: Implemented filesystem `mtime` comparison against `ph:LastProcessedAt` with a 2-second safety margin to shield manual Lightroom edits that might not have updated `xmp:MetadataDate`. Added a graceful database-fallback if filesystem `mtime` retrieval fails.
- **Precision mtime Alignment**: Explicitly updated written sidecars' physical `mtime` to match the internal `ph:LastProcessedAt` timestamp using the `filetime` crate, completely eliminating scheduling delay and OS write skews.
- **XML Parser Resiliency**: Refactored `photohelper-sidecar` XMP reading to handle CData block type-normalization correctly, and carry full sidecar file path context `%path.display()` on all warning logs.
- **Syncing Guide**: Authored a premium user-facing guide at `docs/user-guide/lightroom-sync.md` documenting passive sync, reload steps, case sensitivity, custom label configurations, and conflict overrides. Updated `README.md` to link the guide and align subcommand statuses.

**Final test count**: 248 tests (all 100% passing).
`just ci` is completely green.

### How to resume (session 10)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-10/run-pipeline
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 20 — session 10 SHIPPED (2026-05-31; run pipeline orchestrated)

**Status**: SHIPPED. PR #13 merged to main.
**Author**: Antigravity session 10, session-end window

### What landed since Checkpoint 19

- Implemented the `run` orchestrating pipeline, connecting ingest → cull → develop → export into a single unified CLI subcommand.

### How to resume (session 11)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-11/lightroom-metadata-sync-fixes
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 21 — session 11 SHIPPED (2026-05-31; BUG-002, TOCTOU, XML safety)

**Status**: SHIPPED. PR #14 merged to main.
**Author**: Antigravity session 11, session-end window

### What landed since Checkpoint 20

- **XML Data Safety (Theme B & D)**: Replaced O(N) heap allocation XML filtering with in-place scalar validation (`is_valid_xml_char`) and explicit builder rejection (via `Err(Error::Validation)`).
- **Strict Bounds (Theme E)**: Replaced silent masking `.clamp()` on `nima_score` with explicit validation at the builder boundary.
- **Fail-Open Escalation (Theme F)**: Upgraded filesystem operations to explicitly match and ignore `ErrorKind::NotFound` while propagating and escalating other `Error::Io` issues like `PermissionDenied`, closing a vulnerability where locked sidecars were assumed conflict-free.
- **Panic-Free Architecture (Theme I)**: Replaced `write!(...).expect(...)` calls in `writer.rs` with infallible `.push_str(&format!(...))` appending to comply with project policies.
- **Deduplication Correctness (Theme A)**: Fixed the Unicode path deduplication case-folding bug in `develop.rs` to guarantee uniform casing across Linux and macOS filesystems to prevent write races.

### What is not yet in place

- TD-012 AHD demosaic, TD-017 O(n²) clustering, TD-018 f32 BLOB quantization.

### How to resume (session 12)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-12/to-be-determined
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint 22 — session 12 SHIPPED (2026-05-31; export enhancements: ISP and watermarks)

**Status**: SHIPPED. PR #15 merged to main.
**Author**: Antigravity session 12, session-end window

### What landed since Checkpoint 21

- **Image Signal Processor (ISP)**: Implemented `ToneMappingLut` in `photohelper-export` for O(1) tone mapping of 16-bit linear RAW samples to 8-bit sRGB. Converts linear float with exposure compensation, applies ACES-like filmic S-curve, and finalizes with the sRGB OETF.
- **Image Watermarking**: Replaced empty badge stubs with `tiny-skia` based PNG decoding and compositing. Supports percentage-based scaling (relative to the image's long edge) and robust boundary checks with explicit fail-safe error propagation (`WatermarkOmitted`).
- **Tech-Debt Remediation**:
  - Pre-load badges once per run using `PreloadedBadge::load` and `Arc<tiny_skia::Pixmap>` to avoid $O(N)$ repeated disk I/O and PNG decoding bottlenecks per image.
  - Refactored `run_export` collision resolution using a `HashMap<PathBuf, usize>` for the target file stems, optimizing it from an $O(N^2)$ prefix scan to amortized $O(1)$.
  - Decoupled `export_photo` from the catalog persistence objects (`DevelopRow`), accepting abstract filesystem paths instead.
  - Replaced masking logic where failure outcomes on `--strict` were incorrectly shadowed.

### What is not yet in place

- TD-012 AHD demosaic, TD-017 O(n²) clustering, TD-018 f32 BLOB quantization.

### How to resume (session 13)

```bash
git switch main && git pull --ff-only origin main
git switch -c session-13/to-be-determined
just session-start
cat SESSION-STATE.md
```

---

## Checkpoint — session 15 PAUSED for context refresh (2026-06-02; plan-review COMPLETE, implementation not yet started)

**Status**: PAUSED. Branch: `session-15/watermark-and-rename`. `just ci` GREEN (248 tests). Plan-review complete (plan v4 committed; 3 review rounds ran to convergence). No implementation code written yet.

**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 15, context-refresh window)

### What landed this window

7 commits on `session-15/watermark-and-rename`:

| Commit | Subject |
|---|---|
| `82b2bd79` | `docs(session-15): session contract + state pointer (session-start)` — plan v1 + SESSION-STATE pointer corrected. |
| `59bf5ff5` | `docs(session-15): plan-review Round 1 artifact (7C+10H+3M+2L by theme)` |
| `d749f289` | `docs(session-15): plan v2 — Round 1 remediation (closes 7C+10H+3M+2L)` |
| `dbda6004` | `docs(session-15): plan-review Round 2 artifact (2C+7H+6M+3L by theme)` |
| `a8704da3` | `docs(session-15): plan v3 — Round 2 remediation (closes 2C+7H+6M+3L)` |
| `717c7741` | `docs(session-15): plan-review Round 3 artifact (0C+2H+4M+5L, CONVERGED)` |
| `da818c0d` | `docs(session-15): plan v4 — Round 3 remediation; plan-review COMPLETE` |

Plus (in this pause window):
- TD-024 filed (`paste` RUSTSEC-2024-0436 — unmaintained dev-dep advisory surfaced by fresh audit DB fetch; ignored in `.cargo/audit.toml`)
- Ledger updates (SESSION-STATE, HANDOFF, TECH-DEBT)

### Plan-review summary (what was decided and why)

The plan-review ran 3 rounds (R1 → R2 → R3) to convergence. Full 8-agent suite + 9th-agent verification each round; **cumulative hallucination/discard rate: 0.00** across ~50 code citations.

**Severity trajectory**: 7C → 2C → **0C** (converged R3).

**The single highest-value catch (R1):** the plan repeatedly described net-new work as "reuse." `export_photo` (`crates/photohelper-export/src/lib.rs:178-347`) is a private 170-line monolith — so the plan now extracts shared `pub` primitives **first** (D1.0), then builds the two new subcommands on top.

**Round-2 CRITICALs (both closed by Round 3):**
- **RT-A**: `canonicalize_within` cannot validate a *non-existent* destination (it wraps `std::fs::canonicalize` which errors on missing paths — `model.rs:264`). v4 uses **lexical containment** on the already-canonicalized `--output` directory instead.
- **RT-B**: sanitized-stem + NAME_MAX truncation could silently clobber two distinct sources to one output. v4 pins the exact pipeline order (sanitize → compose → cap-STEM → `resolve_collisions` keyed on final bytes) and makes the catalog `ORDER BY` total (add `, p.id`). Note: `PhotoId` has **no `Ord`** and `ingested_at` is **not projected** onto `DevelopRow`, so the Rust-sort option is non-compilable — the SQL `, p.id` is the only viable branch.

**Four user decisions baked into the plan:**
- **Q-A**: RAW via `ProcessOptions::Srgb8` (plain sRGB, not filmic ISP) → uniform look with raster.
- **Q-B**: mark-doesn't-fit = **error** (no JPEG written, `EX_PARTIAL_FAIL` even without `--strict`).
- **Q-C**: non-CR3 RAW gated behind `--allow-untested-raw` + post-decode dimension+channel sanity guard.
- **Q-D**: marks are **PNG only** (fatal up-front on non-PNG).

**Key architectural pin from plan-review:**
`render_to_jpeg(rgb: &[u8], w, h, opts)` takes a **borrow** (not an owned `Vec`) — call as `render_to_jpeg(img.pixels(), img.width().get(), img.height().get(), &opts)` — so there is no per-image ~24MB copy at the `RgbImage → render_to_jpeg` seam.

### What is not yet in place

- **D0**: remove untracked scratch files (`crates/photohelper-sidecar/test_quick_xml.rs`, `crates/photohelper-sidecar/test_quick_xml/`, `diff.txt`).
- **D1.0**: extract `resize_rgb`/`render_to_jpeg`/`pixmap_to_rgb` from the monolith + make `compress_jpeg`/`draw_image_watermark`/`calculate_watermark_position` `pub`; re-point `export_photo` at the shared functions; move `TempFileGuard` + `resolve_collisions` to `cli/util.rs`. Migrate `test_watermark_position_calculation` to the 2-axis signature. Re-point export, green its integration tests.
- **D1**: raster loader (`image` dep on `photohelper-export`; `SourceKind` dispatch; EXIF orientation; `RgbImage::new` channel guard); `MarkPlacement`/`GeometryError`/`MarkSlot` geometry module; shadow generator; composite parametrization (`BadgeSizeBasis { LongEdge(Scale) | Height(f32) }`, per-axis margin).
- **D2**: `watermark` subcommand wiring, tests (incl. `mark_doesnt_fit` integration test asserting exit 2/1 — R3-B; written:N positive row — R3-F).
- **D3**: `rename` subcommand (the shared `RenamedFilename` builder, `(ingested_at, id)` ORDER BY via SQL `, p.id`).
- **D4**: ledger updates, DN reconciliation (duplicate DN-029@241/329, DN-033@284/305 → renumber to DN-038/039; new untested-RAW DN-040), decision note, scripts, README quickstart. File NEF/ARW fixtures TD at TD-024 (BUT NOTE: TD-024 was just claimed for the `paste` advisory above — the NEF/ARW TD is now **TD-027** per the ledger non-contiguity scan; confirm against TECH-DEBT.md at filing).

**Important correction on TD numbering**: At plan-review time, we computed TD-024 was free and earmarked it for the NEF/ARW fixtures TD. But during this pause window, TD-024 was filed for the `paste` audit advisory. The NEF/ARW fixtures TD should now be filed as **TD-027** (next free after TD-026; TD-027..TD-039 were confirmed free; TD-040 is taken). Confirm against TECH-DEBT.md before filing.

### Open session-14 sidecar debt (NOT in scope for session 15)

The session-14 session-end review left 15 verified findings open (2 CRITICAL + 8 HIGH + 3 MEDIUM + 2 LOW) in `photohelper-sidecar` (`conflict.rs`, `writer.rs`) with no Round-4 CLEAN artifact. Session 15 deliberately avoids this code (D-Q7: catalog metrics + verbatim `.xmp` copy; `rename` never calls `read_xmp`/`write_xmp`). A future sidecar-focused remediation session is needed.

### Restart prompt

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-15/watermark-and-rename
just session-start
```

Then read `SESSION-STATE.md` (top), this checkpoint, and `docs/plans/session-15.md` (v4, the final plan). Begin with **D0** (the untracked scratch files are still present — `rg`-confirm they're unreferenced, then `rm`), then **D1.0** (extraction + export re-point + green CI), then D1 → D2 → D3 → D4.

---

## Checkpoint — session 15 PAUSED for context refresh (2026-06-02, post-ship discoveries)

**Status**: PAUSED. Branch: `session-15/watermark-and-rename`. `just ci` GREEN. All implementation work complete; all implementation reviews (R1 + R2) clean. Release CI for v0.1.0 green (2 archives: macOS arm64 + Linux x86_64). **Next action: run `/session-end` to ship the PR to main.**

### What is done (complete list)

**Core session-15 deliverables (from previous checkpoint):**
- D0–D4 all shipped and committed
- Implementation review R1 (11 HIGH/MEDIUM themes) and R2 (7 HIGH/MEDIUM themes) — all closed
- `just ci` GREEN throughout

**Post-ship discoveries resolved in this window:**

| Fix | Commit | Detail |
|---|---|---|
| Mark quality — Lanczos3 | `fix(export): Lanczos3 for badge...` | Replaced tiny-skia Pattern (interpolation) with `image::imageops::Lanczos3` for badge downscaling. Root cause: Marca-1.png is 1100×1540px and Marca-2.png is 8120×1920px — at 1080px output these scale 15-20× and bicubic aliased badly. Lanczos3 is anti-aliased area-averaging. |
| Mark margins — equal sides | (earlier R2 fix) | `MarkPlacement::fit` now takes `margin_x_frac` + `margin_y_frac` separately; `fit_equal_margin` added for uniform pixel margin from short edge. Fixes: mark1 had 88px from right but 50px from top on 1920×1080. |
| Readability warning | (same R2 fix) | `MARK_MIN_READABLE_PX = 80`; warns on stderr when mark_h < 80px. |
| `photohelper-produce.sh` raster fix | `scripts/photohelper-produce.sh` | When source has BOTH CR3 and JPEG/PNG files, script now watermarks raster files directly (in a temp dir) alongside the catalog-exported RAWs. Addresses the case where user adds JPEG/PNG crops alongside CR3 originals. |
| Release CI v0.1.0 | `.github/workflows/release.yml` | After extensive debugging: ORT uses Apple CoreML on macOS arm64 (no bundling needed) and links statically on Linux (libonnxruntime.a). Archives: binary + models only. 79-81MB each. Windows deferred (TD-029). |
| Mark quality test | `just ci GREEN` | Additional tests: Lanczos3 quality is verified by visual output (not unit-testable without golden images). |

**Key architectural findings:**

1. **ORT on macOS arm64 = Apple CoreML** — ort-sys 2.0.0-rc.12 uses CoreML.framework (system) as the inference backend on Apple Silicon. No `libonnxruntime.dylib` to bundle. Binary is self-contained.

2. **ORT on Linux = static link** — ORT 1.24.2 for Linux is a static archive at `~/.cache/ort.pyke.io/.../libonnxruntime.a`. Binary is self-contained.

3. **Virtual copy gap discovered** — User added `_MG_9703-1.cr3` through `_MG_9703-8.cr3` as identical-bytes copies (same mtime!) of `_MG_9703.CR3` with different Lightroom crops in XMP (`crs:CropTop/Left/Bottom/Right`). The catalog deduplicates them all to ONE PhotoId (same hash). Export doesn't apply XMP crops. This is a scope for **session 16** (virtual copy + XMP crop support). See next steps.

### XMP crop data discovered (session-16 scope)

`_MG_9703-1.xmp` through `-8.xmp` each contain different `crs:CropTop/Left/Bottom/Right` values defining 8 different aspect ratio crops of the same RAW. Currently only 1 of the 9 is in the catalog (all share same PhotoId). Session 16 would need:
1. Sidecar: read crop rect from `crs:HasCrop=True` + Top/Left/Bottom/Right
2. Export: apply crop to decoded pixel buffer before resize
3. Catalog: support virtual copies (same-content files with different paths → separate entries)

### Restart prompt (fresh context)

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-15/watermark-and-rename
just session-start
```

Then read `SESSION-STATE.md` and this checkpoint. **Next action**: run `/session-end` to fire the final review gate and open the PR to main.

---

## Checkpoint — session 15 PAUSED for context refresh (2026-06-02, second window)

**Status**: PAUSED. Branch: `session-15/watermark-and-rename`. `just ci` GREEN. All post-ship bugfixes committed. **Next action: run `/session-end` to ship the PR to main.**

### What landed since previous checkpoint

| Fix/Feature | Commit | Detail |
|---|---|---|
| JXL unsupported-format warning | `fix(watermark)` | `watermark` now logs actionable `tracing::warn!` when file extension is unsupported (e.g. `.jxl`), instructing user to export as JPEG first. |
| `produce.sh` JXL/HEIC detection | `fix(watermark)` | Script now detects JXL/HEIC/TIFF/WebP at startup and warns with filenames before pipeline runs. |
| `produce.sh` partial-failure tolerance | `fix(produce)` | Raster watermark step no longer kills the script on exit code 2 (mark-doesnt-fit). Shows warning and continues. Fixed by capturing `WM_EXIT` and checking for EX_PARTIAL_FAIL. |
| 54% performance optimization | `perf(export)` | Added `--mark1-png`/`--mark2-png`/`--with-shadow` to the `export` subcommand. Marks are now composited INSIDE the filmic-ISP encode step — eliminating the separate `watermark` pass (no second JPEG decode/encode cycle). `produce.sh` updated to use single-pass. Benchmark: 6:10 → 2:50 for 370 photos at 4000px. |

### Architecture added: single-pass export+watermark

`ExportOptions` gained two new fields:
- `render_marks: Vec<MarkSpec>` — height-based corner marks applied via `render_to_jpeg` after filmic ISP
- `render_shadow: Option<ShadowSpec>` — shadow gradient alongside the marks

`export_photo` combines legacy badge marks (LongEdge-based) with new height-based marks in `RenderOptions` before the final `render_to_jpeg` call. This means the filmic tone-mapped pixel data gets marks applied WITHOUT a second JPEG encode/decode cycle.

### Bug found: mark-doesnt-fit on narrow portrait images

`_MG_9703-6.jpg` (3000×5333px) at 4000px output scales to 2300×4000. Marca-2 (8120×1920 = 4.23:1 wide) sized at 13% height = 520px tall → 2244px wide. With 184px margin: 2244+184 = 2428 > 2300 → MarkDoesNotFit. This is correct behavior (can't fit). Script now shows warning and continues.

### Bug found: JXL files silently skipped

User exported `_MG_9703-1.jxl` through `-8.jxl`. JXL (JPEG XL) is not supported by the `image` crate in our config. Files returned `None` from `SourceKind::classify` → silently `skipped_unsupported`. Now logs a clear warning with the extension and "Convert to JPEG first" message.

### Open: XMP virtual copies (session-16 scope)

`_MG_9703-1.cr3` through `-8.cr3` are identical bytes (same PhotoId → catalog deduplicates to 1 entry). Each XMP has different `crs:CropTop/Left/Bottom/Right` values defining 8 different aspect ratio crops. The catalog and export pipeline don't support virtual copies. This is a session-16 feature (DN-042).

### Restart prompt

```bash
cd /Users/ph/area-de-trabalho/pessoal/photohelper
git switch session-15/watermark-and-rename
just session-start
```

Then read `SESSION-STATE.md` and this checkpoint. **Next action: run `/session-end`** to fire the final review gate and open the PR to main.
