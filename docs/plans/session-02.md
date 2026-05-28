# Session 02 — `libraw-cr3-decode`

> **Branch**: `session-02/libraw-cr3-decode`
> **Started**: 2026-05-28
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v1 (this revision; pre plan-review)

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the LibRaw FFI integration that turns `photohelper-raw` from a one-line
stub into a working RAW pipeline for Canon R8 CR3. Two complementary
deliverables under the same FFI surface:

1. **`photohelper-raw::exif::read_cr3(path) -> Result<RawExif>`** — extract
   `Make`, `Model`, `Orientation`, `CaptureTime`, `Width`, `Height` from a
   Canon R8 CR3 ISO-BMFF container. This is the **DN-011 critical-path
   remediation**: kamadak-exif fails on 371/371 real Canon R8 CR3s, so
   LibRaw EXIF is the only path to a usable `--strict` mode and a
   non-degraded catalog row for CR3 ingest.
2. **`photohelper-raw::decode::read_raw(path) -> Result<RawImage>`** — decode
   the Bayer-pattern sensor data into a `RawImage { pixels: Vec<u16>,
   width, height, black_level, white_level, cfa_pattern, ... }` ready to
   feed session 04's develop pipeline. RAW pixel decode is the
   originally-planned-for-session-02 deliverable; the EXIF surface above
   was elevated from "nice to have" to "critical path" by DN-011 between
   plan and start.

`ingest_one` rewires to use LibRaw EXIF as the primary path for CR3
(kamadak-exif stays as the path for non-CR3 EXIF in later sessions — JPEG
sidecars on export, etc.). Once wired, integration test row 32 flips its
assertions from `is_none()` to `Some("canon-r8")` and the strict-mode test
on real CR3 fixtures exits 0 — closing the DN-006 binding trigger.

### Scope rationale: why bundle EXIF + decode + rusqlite bump in one session

LibRaw is a single C library. Wiring its FFI surface for **only** EXIF read
and then re-wiring it for decode in a later session would mean doing the
FFI safety review, the LGPL static-link plumbing, and the build-system
configuration twice. The EXIF + decode pairing keeps the FFI surface
defined once and reviewed once. The rusqlite bump is bundled because the
binding trigger (TD-002) requires the bump *before* the next schema-touching
session, and session 02 will modify `Catalog::upsert` to populate the EXIF
columns that v1 stubbed as NULL for CR3 — so we're touching catalog code
either way. Bundling minimizes churn.

### Deliverables (when the PR merges, the following will exist)

1. **`photohelper-raw` real implementation**
   - `photohelper-raw::ffi` module (the **only** module containing `unsafe`
     blocks) carrying LibRaw bindings — either via the `libraw-rs` /
     `libraw-sys` ecosystem (TBD per Discovery item DI-1) or a minimal
     hand-rolled FFI shim against LibRaw 0.21+ headers, whichever survives
     plan-review's static-link / LGPL audit.
   - `photohelper-raw::exif::read_cr3(path) -> Result<RawExif, Error>`
     populating `RawExif { make, model, orientation, capture_time_utc,
     width, height }`. Returns `Error::RawExifMissing { path }` if LibRaw
     can open the file but the EXIF fields are absent (corrupt CR3),
     `Error::RawOpenFailed { path, cause }` if LibRaw cannot open it.
   - `photohelper-raw::decode::read_raw(path) -> Result<RawImage, Error>`
     producing `RawImage { pixels: Vec<u16>, width, height, black_level,
     white_level, cfa_pattern, white_balance_multipliers, color_matrix }`
     — the inputs session 04's develop pipeline will consume. Detail of
     intermediate structs to be locked at plan-review.
   - `Error` enum follows the workspace convention (thiserror, no `#[from]`
     across public boundaries, `#[non_exhaustive]`).

2. **LibRaw build-system + LGPL compliance scaffolding**
   - `build.rs` for `photohelper-raw` that statically links the LibRaw
     LGPL build via `cc`/`cmake`/`pkg-config` (TBD per Discovery item
     DI-2). Vendored source path under `crates/photohelper-raw/vendor/`
     OR `Cargo.toml` `[build-dependencies]` pointing at a pinned LibRaw
     tarball — whichever survives plan-review.
   - `docs/decisions/0002-libraw-lgpl-static-link-mechanics.md` — closes
     DN-001 by recording the §6(b) artifact shape (e.g. a per-release
     `vendor/libraw-X.Y.Z.tar.gz` shipping alongside the binary in GitHub
     Releases). The decision doc enumerates the build steps a relinker
     needs and the artifact name + path pattern.

3. **Real CR3 fixtures via `git-lfs`**
   - `git-lfs` initialized in the repo (`.gitattributes` + `.lfsconfig`),
     fixture file(s) at `tests/fixtures/cr3/` with a 1-CR3-per-camera
     starter pack. Canon R8 first; future cameras append.
   - **License audit recorded**: every fixture is CC0 or equivalent
     unencumbered; sources cited in `tests/fixtures/cr3/README.md`.

4. **`photohelper-cli::commands::ingest` rewired for LibRaw EXIF**
   - `parse_exif()` (currently `kamadak-exif`-only) becomes
     `parse_exif_for(path, extension)`:
       - `*.cr3` → `photohelper-raw::exif::read_cr3(path)` → `ExifMetadata`
       - other extensions (JPEG fallback for future sidecar work) →
         `kamadak-exif` as today
   - The R2-T5 "EXIF parsed but yielded zero fields" WARN gate stays valid
     because LibRaw now genuinely populates the fields for CR3.
   - The lying-WARN-on-CR3 production behavior (370/371 files) from
     DN-011 is fixed by construction.
   - `--strict` mode exit code on a CR3-only directory: 0 (was: non-zero
     post-R2-T12; fail-open pre-R2-T12).

5. **`photohelper-catalog` minor schema + rusqlite bump**
   - **TD-002 close**: `rusqlite 0.32` → `rusqlite 0.40` (or latest at
     remediation time). API-compatible migration; close TD-002 with the
     "Closed" disposition. Update Cargo.toml workspace deps + Cargo.lock.
   - **Schema columns populated, not added**: v1 schema already has
     `make`, `model`, `capture_time_unix_seconds`, `width`, `height`,
     `exif_orientation` columns — currently NULL for CR3 (per DN-006
     fallback). After session 02, those columns contain real values for
     CR3 rows. **No `PRAGMA user_version` bump** because the schema shape
     doesn't change; we just stop writing NULL.
   - Update `docs/decisions/0001-catalog-schema-v1.md § Status` to
     acknowledge LibRaw landed (audit-trail clarity; the schema itself
     stays v1).

6. **Test infrastructure (DN-008 subset)**
   - `Catalog::poison_for_testing` knob (closes DN-008 row "poison test
     knob"). Used by the new test for the `BEGIN IMMEDIATE` + `ROLLBACK`
     poison-recovery path.
   - **Row coverage commitment** (per DN-008's session-02 binding trigger):
     this session lands tests for rows **{6, 32-equiv (real CR3), 39, 42,
     43, 49}** (LibRaw-enabled subset). Defers rows **{12, 13, 14, 17, 18,
     19, 34}** to session 03+ with explicit DN-008 cross-reference (those
     rows depend on cull pipeline / dup-group catalog tables / multi-camera
     fixtures none of which session 02 ships). The plan-review will
     finalize the exact row split.
   - **R2-T18 closure**: add regression tests for the 4 R1.T10 WARN paths
     (`build_global` already-initialized, `wal_checkpoint` recovered,
     heartbeat death, `file-lock` op-tag). Folded into the row-coverage
     bundle above.
   - **R2-T19 closure**: replace the non-discriminating 128KB PhotoId
     test with a 96KB test where buggy vs. fixed paths produce different
     digests.

7. **DN-012 polish items folded in where naturally touched**
   - `KnownCamera` Display impl (touched when LibRaw populates camera
     fields → `camera_slug` rendering benefits).
   - `UpsertOutcome` `#[non_exhaustive]` for uniformity (touched when
     `Catalog::upsert` rewires for the populated EXIF columns).
   - Other DN-012 items (workspace clippy comments, Windows
     case-sensitivity) deferred if not naturally touched.

### Out of scope (explicit deferrals)

| Item | Owner | Tracking |
|------|-------|----------|
| AI culling (`cull` subcommand real impl) | session 03 | unchanged |
| AI denoise (SCUNet / `develop` subcommand) | session 04+ | unchanged |
| XMP sidecar I/O (`crs:` / `ph:` namespaces) | session 04+ | unchanged |
| JPEG export + watermarks (`export` subcommand) | session 05 | unchanged |
| `cull-score` + `dup-group` catalog tables + migration framework v1 → v2 | session 03 (when `cull` is wired) | DN-005 |
| Release-engineering wiring (musl static, codesign, Authenticode, winget, Homebrew tap, GitHub Release workflow) | dedicated release session | DN-001 (decision-only this session; build wiring later) |
| `scripts/verify-review-artifact.sh` (bash port of fox's mjs enforcer) | future session | DN-009 |
| GitHub Actions SHA pinning | before first external PR / first release tag | TD-001 |
| Heartbeat-thread `.join()` cleanup | session that touches `run_ingest` teardown (likely session 04 export pipeline) | TD-003 |
| DN-008 rows {12, 13, 14, 17, 18, 19, 34} | session 03+ with cull pipeline | DN-008 (cross-ref update) |
| DN-012 polish items not naturally touched in this session | next session that touches the relevant file | DN-012 (cross-ref update) |
| Windows build + cross-compile audit for LibRaw | v0.2 (target Linux + macOS first) | NEW (DI-3 below if it lands) |
| Other RAW formats (CR2, NEF, ARW, RAF) | when a non-Canon camera profile is added | NEW (DI-4 below if it lands) |

### Test plan

| Deliverable | Unit | Integration |
|-------------|------|-------------|
| `photohelper-raw::ffi` safety | NULL path → `RawOpenFailed`; truncated CR3 → `RawOpenFailed`; non-CR3 bytes (e.g. PNG) → `RawOpenFailed`; valid CR3 → `Ok`. Each test exercises the FFI boundary without segfault. | n/a (real CR3 covered below) |
| `photohelper-raw::exif::read_cr3` | n/a (FFI call; integration only) | Real CR3 fixture: `make == "Canon"`, `model == "Canon EOS R8"` (or whatever LibRaw reports), `orientation in 1..=8`, `capture_time_utc.is_some()`, `width > 0`, `height > 0`. |
| `photohelper-raw::decode::read_raw` | n/a (FFI call; integration only) | Real CR3 fixture: pixel count == `width * height`; `black_level < white_level`; `cfa_pattern.len() == 4`; pixels not all zero / not all `u16::MAX`. |
| `ingest` rewire for CR3 | Mock `parse_exif_for` per-extension dispatch (pure-Rust unit). | End-to-end: `photohelper ingest <real-cr3-dir>` → `walked: N, no-exif: 0, ingested: N, …`; `make`/`model`/`camera_slug`/`capture_time_unix_seconds` populated in catalog; `--strict` exits 0. |
| TD-002 rusqlite bump | n/a (dependency bump) | Existing test suite stays green; `cargo audit` clean on the new bundled SQLite. |
| `Catalog::poison_for_testing` | Poisoning the mutex → `Catalog::upsert` returns `Error::CatalogPoisoned`; next call recovers cleanly. | n/a |
| R2-T18 WARN regression tests | n/a (use `tracing-test` or stderr-capture). | Run `ingest` twice in-process → `build_global already initialized` WARN fires; kill+reopen catalog → `wal_checkpoint recovered N frames` WARN fires; parent-dir read-only → `lock-file-create` op-tag in WARN; (heartbeat death tested via the deferred `panic_for_testing` knob — if added). |
| R2-T19 PhotoId test replacement | Replace 128KB all-0xAA test with 96KB test where bytes `[60KB..68KB)` differ from surrounding bytes so the buggy double-hash and the fixed single-hash produce different digests. | n/a |
| `git-lfs` fixture wiring | n/a | `cargo test --workspace` passes locally with a fresh `git lfs fetch`; CI configures `git lfs install` before checkout. |

### Checkpoints fired this session

| Checkpoint | When | Agents | Artifact |
|------------|------|--------|----------|
| Plan-review | After this top block lands (now) | Full 8 (Tier 5) | `docs/code-reviews/session-02-plan-round{1,2}.md` |
| Sub-component review — `photohelper-raw::ffi` | When `ffi` module first exposes a non-scaffold public API | 3–5 (Tier 4) | `docs/code-reviews/session-02-photohelper-raw-ffi-round{1,2}.md` |
| Sub-component review — LibRaw build-system / LGPL | When `build.rs` + decision doc 0002 land | 3–5 (Tier 4) | `docs/code-reviews/session-02-libraw-build-round{1,2}.md` |
| Session end | After all code complete | Full 8 (Tier 5) | `docs/code-reviews/session-02-round{1,2}.md` |

Plus: the SESSION-STATE.md drift from session 01 (still says "R2 REMEDIATION
APPLIED — ready for `just ci`" despite the PR having merged) gets cleaned up
*before* session 02 plan-review fires. Trivial; cosmetic; recorded here so
plan-review notices the housekeeping commit.

### Discovery items expected up-front

The following are flagged now so plan-review can adjudicate before code:

- **DI-1: Existing LibRaw Rust wrapper vs. hand-rolled FFI shim.** The
  Rust ecosystem has `libraw-rs` and `libraw-sys` (maintenance status TBD
  as of 2026-05-28; CVE history TBD). Plan-review must pick: (a) adopt the
  most-maintained existing crate as a workspace dep and write only the
  thin domain-layer around it, or (b) hand-roll a minimal FFI shim
  binding only the LibRaw calls we use. Trade-offs: (a) less code we own
  + faster delivery + dep on third-party maintenance pace; (b) full
  control + smaller attack surface + more upfront work. If (b), the
  hand-rolled shim lives in `photohelper-raw::ffi` and is the ONLY
  workspace `unsafe` site.

- **DI-2: LibRaw static-link mechanics (vendored source vs. system
  install).** Two viable shapes: (a) vendor a pinned LibRaw source tarball
  under `crates/photohelper-raw/vendor/libraw-X.Y.Z/` and build it from
  `build.rs` via `cc` or `cmake` (reproducible, but bloats the repo by
  ~10 MB), (b) require system-installed LibRaw via `pkg-config` (slim
  repo, harder distribution story for end-users on Windows / minimal
  Linux). The LGPL §6(b) artifact mechanic (decision doc 0002) hangs off
  this choice.

- **DI-3 (may surface): Windows LibRaw build path.** If LibRaw doesn't
  cleanly cross-compile to Windows from the `x86_64-pc-windows-msvc`
  target with the chosen build mechanism, Windows ships in v0.2 as
  originally scoped. If it does, opportunistically add Windows to
  session-02 CI matrix. Recorded as DN-013 if it materially changes
  scope.

- **DI-4 (may surface): kamadak-exif retention vs. removal.** Once LibRaw
  EXIF handles CR3, the only remaining kamadak-exif call site is the
  non-CR3 dispatch in `parse_exif_for`. If session 02 doesn't actually
  exercise that path (we ingest CR3-only), kamadak-exif becomes a
  dependency for code that never runs in v0.1. Plan-review decides
  whether to keep it (for the eventual JPEG/sidecar work in session 04+)
  or drop it (eliminate dead-on-arrival code + the EXIF-attack surface).
  If we drop it, also drop the test that exercises the JPEG path.

### Acceptance criteria (Definition of Done)

A session-02 merge candidate must satisfy all of:

1. `just ci` green locally and on GitHub Actions.
2. `photohelper ingest /Users/ph/Pictures/tests --strict` (the user's real
   371-CR3 fixture set) exits 0, with `walked: 371, no-exif: 0,
   ingested: 371, already-catalogued: 0, skipped (non-RAW): 1`. The 371
   catalog rows have non-NULL `make`, `model`, `capture_time_unix_seconds`,
   `width`, `height`, `camera_slug`.
3. `photohelper-raw::ffi` is the only crate with `unsafe` blocks; every
   `unsafe` block has a `// SAFETY:` comment naming the LibRaw invariant
   it relies on.
4. `cargo audit --deny warnings` clean on the bumped `rusqlite` + the
   new LibRaw build inputs.
5. `docs/decisions/0002-libraw-lgpl-static-link-mechanics.md` exists and
   names the artifact shape that ships alongside each release binary.
6. R2 review round 2 surfaces zero CRITICAL items (per
   `docs/quality-assurance.md § Double-review protocol`); MEDIUM and LOW
   findings ship with TD/DN entries per the No-Acceptable-Trade-offs
   policy.

### Risk register

| Risk | Likelihood | Mitigation |
|------|-----------:|------------|
| LibRaw fails to extract CR3 EXIF for our specific R8 firmware revision | low | Pre-flight check on the user's 371-CR3 set BEFORE writing the wire-up; if it fails, escalate scope (raise plan-review). |
| `libraw-rs` / `libraw-sys` is unmaintained or has open CVEs | medium | Plan-review's DI-1 decision; fall back to hand-rolled shim if (a) is untenable. |
| Static-linking LibRaw on macOS arm64 hits a compiler-flag landmine | medium | DI-2; if vendored-source path is non-trivial, defer Windows to v0.2 explicitly and ship Linux+macOS this session. |
| `rusqlite 0.40` API-breaks our `Connection`/`Transaction` usage | low | Spike the bump before committing to the schema-column-populate work; fall back to a `rusqlite 0.3X` intermediate if 0.40 demands a multi-file rewrite. |
| `git-lfs` adds friction for contributors not using it | low | Document `git lfs install` in README; CI prerequisite; fixtures are small (~30 MB total). |
| Session scope creep beyond a single session boundary | medium | This top block is the contract; any expansion needs a plan-revision commit + re-fire of plan-review. |

### Cross-references

- DN-001 (LibRaw LGPL §6(b)) → owned this session (decision doc 0002).
- DN-005 (catalog schema) → partially advanced (v1 columns populated for
  CR3, no schema shape change).
- DN-006 (kamadak-exif CR3 failure) → **closed by construction** when
  `parse_exif_for(*.cr3, ...)` dispatches to LibRaw.
- DN-007 (rusqlite stale) → **closed** by TD-002 close.
- DN-008 (test infrastructure) → partially advanced (poison knob + subset
  of rows + R2-T18 WARN coverage). Remaining rows deferred to session 03+
  with cross-ref update.
- DN-011 (DN-006 production trace) → **closed by construction** alongside
  DN-006.
- DN-012 (T15 polish items) → partially advanced where naturally touched;
  remainder rolls forward.
- TD-001 (Actions SHA pinning) → unchanged this session.
- TD-002 (rusqlite stale) → **closed** this session.
- TD-003 (heartbeat join) → unchanged this session (binding trigger not
  fired — we're not touching `run_ingest`'s post-walk teardown).
- R2-T18 (WARN regression tests) → closed via row-coverage bundle.
- R2-T19 (128KB PhotoId test) → closed via test replacement.
- R2-T22 / R2-T23 (R1 count drifts) → unchanged (cosmetic, accepted as-is
  in R2 disposition).

### Plan revisions log

- **v1 (2026-05-28)**: initial. Bundles LibRaw EXIF + decode +
  rusqlite bump + minor DN-008 row coverage. Two FFI mechanics flagged as
  DI-1/DI-2 for plan-review.
- *(future revisions per plan-review rounds)*

---

## Detailed implementation (populated AFTER plan-review Round 2 remediation)

_(intentionally empty until plan-review v1→v2 lands; the top block above is
the only thing under review at plan-review Round 1.)_
