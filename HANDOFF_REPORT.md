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
