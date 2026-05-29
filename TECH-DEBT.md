# photohelper — Tech-Debt Ledger

> Known shortcuts taken for velocity, each with a remediation plan and a
> **binding trigger**. This ledger is the canonical view of "where the codebase
> trades off quality vs. velocity right now."
>
> Policy: see `CLAUDE.md § No Acceptable Trade-offs Policy`. A stop-gap without
> a TD entry here is a process violation; a deferral without a plan is a
> CRITICAL finding on its own (`docs/quality-assurance.md § Findings triage`).

## Entry format

Each TD has a stable ID (`TD-NNN`) and these fields:

```markdown
### TD-NNN — <descriptive title>

- **Status**: Open | Closed (YYYY-MM-DD, session N; reason)
- **Opened**: YYYY-MM-DD (session N)
- **Stop-gap location**: <file:line> @ <commit-sha>
- **Fundamental fix**: <concrete implementation outline — not "investigate">
- **Binding trigger**: <rev-list-anchored | by YYYY-MM-DD | event-driven>
- **Scope estimate**: <~LoC> / <risk: low|med|high>
- **Consequence of inaction**: <if unaddressed, X happens>
- **Related**: <links to code-review artifacts, discovery-notes, ADRs>
```

---

## Open

### TD-001 — GitHub Actions action versions use `@vN` floating tags, not pinned SHAs

- **Status**: Open
- **Opened**: 2026-05-27 (session 0)
- **Stop-gap location**: `.github/workflows/ci.yml` (all `uses:` lines tagged `<<pin to SHA>>`) @ bootstrap commit
- **Fundamental fix**: replace every `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2` with the corresponding commit SHA from the action's repo; commit a `docs/decisions/0001-action-version-pinning.md` recording the SHAs chosen and the upgrade cadence. Add a periodic refresh task (Dependabot or scheduled session).
- **Binding trigger**: before the first PR from an external contributor merges, OR before the first GitHub Release tag is cut — whichever comes first.
- **Scope estimate**: ~20 LoC across `.github/workflows/ci.yml` + one new decision doc / low risk
- **Consequence of inaction**: a compromised upstream action could exfiltrate secrets or inject code into the build; the `<<pin to SHA>>` comments are visible reminders but not enforced.
- **Related**: `docs/discovery-notes.md` (none yet — this is a self-contained debt)

---

### TD-002 — `rusqlite` pinned at 0.34 instead of plan-v5 target 0.40 (CVE exposure, partial closure)

- **Status**: Partial (2026-05-28, session 2). Bumped from 0.32 → 0.34, the latest version compatible with our MSRV 1.88. **NOT** at the original 0.40 target because rusqlite ≥ 0.36 pulls in `libsqlite3-sys ≥ 0.38` which uses the `cfg_select!` macro requiring MSRV 1.92. Closing the full TD requires bumping MSRV first.
- **Opened**: 2026-05-28 (session 1)
- **Stop-gap location**: `Cargo.toml` `[workspace.dependencies]` `rusqlite = { version = "0.34", features = ["bundled"] }` @ commit (session 02 Deliverable 5).
- **Fundamental fix (remaining)**: bump MSRV to ≥ 1.92 (file an ADR per the 1.85 → 1.88 precedent), then bump `rusqlite` to latest (currently 0.40). Touchups expected on the catalog crate's call sites if `libsqlite3-sys ≥ 0.38` introduces API breaks; verify with `cargo update -p rusqlite && cargo test`.
- **Binding trigger (revised)**: next MSRV bump OR a SQLite CVE that 0.34's bundled amalgamation does not include the fix for OR 2026-08-01 (calendar-anchored, unchanged from original). The MSRV-bump dependency is the most likely path; rusqlite + bundled-sqlite is unusually conservative about MSRV.
- **Scope estimate**: ~30 LoC for the MSRV bump (ADR + rust-toolchain.toml + workspace.package.rust-version) + ~5 LoC for the rusqlite bump itself / low risk.
- **Consequence of inaction**: the bundled SQLite version inside rusqlite 0.34 (mid-2025) is significantly fresher than 0.32 (early-2024), so the CVE-exposure risk is now substantially smaller than when TD-002 was filed. Sitting on 0.34 long-term still violates the bundled-sqlite freshness expectation but the next-CVE risk window is months, not weeks.
- **Related**: `docs/discovery-notes.md` DN-007; `docs/code-reviews/session-01-round1.md § T5`.

---

### TD-003 — Heartbeat thread is not `.join()`-ed at end of `run_ingest`; leaks past summary

- **Status**: Closed (2026-05-28, session 2; replaced `AtomicBool` + `thread::sleep` with `HeartbeatStop` (`Mutex<bool>` + `Condvar`) and a `thread::Builder::new().name("ph-heartbeat").spawn(...)` named handle; `run_ingest` now calls `stop.signal()` then `heartbeat_handle.join()` so all `[heartbeat]` lines flush BEFORE the summary; also restructured `heartbeat_loop` to tick-first-wait-after so a fast ingest still emits at least one liveness signal even when `stop` fires inside the OS thread-startup latency window — closes the DN-019 empirical-trigger class.)
- **Opened**: 2026-05-28 (session 1, R2-T17)
- **Stop-gap location**: `crates/photohelper-cli/src/commands/ingest.rs:181-192` @ commit `0f28627` (R1.T2 remediation kept the leak deliberately to avoid join-latency on summary printing). R2 retained the trade-off; this TD captured the obligation.
- **Fundamental fix (as shipped in session 2)**: `HeartbeatStop` pairs `Mutex<bool>` with `Condvar` so `signal()` cuts a `wait_timeout` short on the spot — join returns near-instantly, not after one `granularity` cycle. The heartbeat thread carries the name `"ph-heartbeat"` so debuggers/profilers can spot it. The loop is now tick-first-wait-after (DN-019 lesson: a wait-first loop races thread-startup against `stop.signal()` and can return without ever printing).
- **Binding trigger (no longer applies; closed)**: any session that touches `run_ingest`'s post-walk teardown OR by 2026-08-01 OR a test-flake from stderr-ordering instability. The empirical trigger fired on 2026-05-28 per DN-019; session 02 closed it ahead of LibRaw work so Acceptance criterion 1 (`just ci` green) is satisfiable.
- **Scope estimate**: ~50 LoC delivered (originally estimated ~15 LoC; the `HeartbeatStop` newtype + struct doc-comments + DN-019 race remediation expanded the budget). / low risk delivered.
- **Consequence of inaction (historical)**: (1) up to one granularity-cycle (≤100ms) of zombie heartbeat output after `summary_line` prints; (2) integration tests asserting strict stderr ordering can flake under CI load; (3) in-process test runs accumulating one leaked detached thread per `run_ingest` call until process exit. All three closed.
- **Related**: `docs/code-reviews/session-01-round2.md § R2-T17`; `docs/code-reviews/session-01-round1.md § T2 sub-(c)`; `docs/discovery-notes.md § DN-019` (empirical trigger).

---

### TD-004 — LibRaw C-library CVE monitoring is manual; `cargo audit` does NOT cover it

- **Status**: Open
- **Opened**: 2026-05-28 (session 2, PR1-T10 from `docs/code-reviews/session-02-plan-round1.md`)
- **Stop-gap location**: `crates/photohelper-raw/build.rs` + `docs/decisions/0002-libraw-lgpl-static-link-mechanics.md` @ session 02's first FFI-landing commit (commit SHA pending). The stop-gap is the absence of any CVE-DB scanner that covers LibRaw — `cargo audit` consults RustSec, which only catalogs Rust crates; LibRaw is C++ and its CVEs (multiple buffer-overflow / out-of-bounds-read CVEs since 2020 per `cve.mitre.org`) are invisible to our gate.
- **Fundamental fix**: wire an automated CVE-DB scanner that covers C-library dependencies. Candidates: (a) `osv-scanner` from Google's OSV.dev (covers the LibRaw CVE feed in the Bitnami / OSS-Fuzz / NIST NVD imports); (b) GitHub Dependabot for the vendored LibRaw tarball (limited — needs a manifest); (c) Trivy or Grype against the built binary's link-graph; (d) manual subscription to LibRaw's GitHub Security Advisories + LibRaw release announcements, with a calendar reminder per release. Path (a) `osv-scanner` is the lowest-friction: a single CLI invocation `osv-scanner --config .osv-scanner.toml .` integrated into `just ci` after `cargo audit`. The config pins the vendored LibRaw version (sourced from `build.rs`).
- **Binding trigger**: first session touching `crates/photohelper-raw` after 2026-08-01 OR any LibRaw CVE disclosure (a real CVE forces immediate action) OR before the first GitHub Release tag is cut (whichever first). Bundling with the release-engineering session is natural: the release workflow also owns Authenticode / codesign / Homebrew tap — CVE scanning fits the same surface.
- **Scope estimate**: ~10 LoC (osv-scanner CLI in `just ci` + `.osv-scanner.toml` config) + maybe a `Cargo.toml` mention of the vendored LibRaw version; or ~5 LoC + a calendar/manual subscription if path (d) is chosen. Low risk; medium consequence if neglected.
- **Consequence of inaction**: LibRaw CVE disclosed in the wild; photohelper binaries ship the vulnerable version; users compromised when LibRaw parses malicious CR3 (e.g. RUSTSEC-2026-XXXX-style stack-exhaustion DoS). Session 02's `Acceptance criteria 4` claim of "`cargo audit --deny warnings` clean on the bumped `rusqlite` + the new LibRaw build inputs" is misleading as written; this TD captures the gap explicitly.
- **Related**: `docs/code-reviews/session-02-plan-round1.md § PR1-T10`; `docs/discovery-notes.md § DN-001` (LGPL §6(a) vendored-tarball commitment which IS the CVE-distribution surface).

---

### TD-005 — Heartbeat env-var-triggered panic site is a test-affordance in a production-path function

- **Status**: Open
- **Opened**: 2026-05-28 (session 2, R3-T3)
- **Stop-gap location**: `crates/photohelper-cli/src/commands/ingest.rs::heartbeat_loop` @ commit (session 02 implementation). Panic site is `#[allow(clippy::panic, reason = "...")]`-annotated AND `cfg!(debug_assertions)`-gated; release builds compile out the env-var read entirely so the panic surface is unreachable in production. Test-only.
- **Fundamental fix**: factor the heartbeat-death-WARN regression test (R2-T18 path 4) into a dev-deps-only utility crate `photohelper-test-helpers` that exposes a `pub fn force_heartbeat_panic_in_thread(handle: &JoinHandle<()>)` helper. The production `heartbeat_loop` becomes panic-free; the test-helper crate is `[dev-dependencies]`-only in `photohelper-cli/Cargo.toml`. Removes the `#[allow(clippy::panic)]` site.
- **Binding trigger**: next session that touches `crates/photohelper-cli/src/commands/ingest.rs::heartbeat_loop` for any reason OR before the first GitHub Release tag is cut (whichever first). Session 04+ export pipeline is the likely first toucher (per TD-003's binding trigger for the related `.join()` cleanup).
- **Scope estimate**: ~20 LoC (new `photohelper-test-helpers` crate with one helper fn + Cargo.toml + dev-dep declaration in `photohelper-cli/Cargo.toml`) + delete the env-var path in `heartbeat_loop` / **low risk**.
- **Consequence of inaction**: production-path `panic!()` site lints clean only because of the `#[allow(clippy::panic)]` annotation. The `cfg!(debug_assertions)` gate makes it unreachable in release, BUT debug-build users who export `PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING=1` get a one-tick-and-die heartbeat. Acceptable for v0.1 (test affordance; documented in DN-015) but creates a "test code in production code" precedent that the project should retire.
- **Related**: `docs/code-reviews/session-02-plan-round3.md § R3-T3`; `docs/discovery-notes.md § DN-015` (heartbeat panic_for_testing vs TD-003 distinction).

---

### TD-006 — `RawDecodeCause` + `RawImageDimensionMismatch` / `RawInvalidLevels` / `RawPath` variants have no dispatch-site routing in `ingest_one`; silently fold into `errored`

- **Status**: Open
- **Opened**: 2026-05-28 (session 2, R3-T6)
- **Stop-gap location**: `crates/photohelper-cli/src/commands/ingest.rs::ingest_one` @ commit (session 02 implementation). Dispatch site matches `RawExifCause` variants only; other `photohelper_raw::Error` variants fall through to `errored` counter via `_` wildcard arm.
- **Fundamental fix**: split `photohelper-raw::Error` into two enums: `RawExifError` (returned by `read_cr3`; carries only `RawExifCause` + `RawPath`) and `RawDecodeError` (returned by `read_raw`; carries `RawDecodeCause` + `RawImageDimensionMismatch` + `RawInvalidLevels`). Type-level guarantee that `parse_cr3_exif` cannot return decode-class errors. OR: extend `ingest_one`'s dispatch table to cover every cross-call combination explicitly per the per-counter semantics table extension (R3-T6 remediation option 2). Add an `IngestStats` counter per cause class so operators can discriminate `WhiteBalanceUnloaded` from `LibRawCallFailed`.
- **Binding trigger**: session 04+ when `decode::read_raw` gets a consumer (the develop pipeline); the dispatch-site routing question can't be answered until the consumer exists. UNTIL THEN, session 02 implementation routes all non-`RawExifCause` errors to `errored` with a per-variant WARN tag for log-grep triage. NEW counters NOT added in session 02; v0.2 plan must address.
- **Scope estimate**: ~40 LoC (enum split + per-cause IngestStats counters + dispatch routing rows in §4d) / medium risk (touches per-counter semantics table + `--strict` predicate + WARN routing).
- **Consequence of inaction**: operators reading `errored: 1` log entries cannot tell if the failure was "corrupt CR3" vs "WhiteBalance unloaded" vs "color matrix unloaded" vs "path validation failed." R2-T6 invariants the plan invested in (`WhiteBalanceUnloaded`, `ColorMatrixUnloaded`) lose their discrimination signal at the dispatch boundary.
- **Related**: `docs/code-reviews/session-02-plan-round3.md § R3-T6`; `docs/plans/session-02.md § Deliverable 4d` (strict-mode predicate); `docs/plans/session-02.md § Deliverable 4c` (per-counter semantics table).

---

### TD-007 — Constructor-time error variants use `PathBuf::new()` (empty path); Display renders useless error message at operator

- **Status**: Open
- **Opened**: 2026-05-28 (session 2, R3-T10)
- **Stop-gap location**: `crates/photohelper-raw/src/decode.rs::{BayerPlane,SensorLevels,WhiteBalance,CamRgbToXyzD65Matrix}::new` @ commit (session 02 implementation). Each constructor populates the `path` field of `Error::RawDecodeFailed` / `Error::RawInvalidLevels` / `Error::RawImageDimensionMismatch` as `PathBuf::new()` (empty path) because the constructor's call site doesn't have the path.
- **Fundamental fix**: change constructor signatures to take `path: &Path` as the first argument: `pub(crate) fn new(path: &Path, data: Vec<u16>, width: NonZeroU32, height: NonZeroU32) -> Result<Self, Error>`. Caller (`read_raw(path)` at the FFI boundary) passes the path explicitly. OR (less invasive): add a `pub fn with_path(mut self, path: PathBuf) -> Self` enricher on `photohelper-raw::Error` that the FFI boundary calls before propagating; the constructors keep their empty-path default; the boundary patches before return.
- **Binding trigger**: session 02 implementation MUST address before merge — operator-facing log lines reading "RAW image decode failed at : white balance unloaded" (empty path) are unactionable; session-end review will catch and require fix. If somehow merged with the gap, the next session touching `photohelper-raw/src/decode.rs` (likely session 04 develop pipeline) MUST close before adding new constructors. OR by 2026-08-01.
- **Scope estimate**: ~15 LoC across 4 constructor sites + the `read_raw` / `read_cr3` boundary patch / low risk.
- **Consequence of inaction**: every decode-side error message reads `"... at : ..."` with no file path; operators cannot triage the failing fixture. Sentry events collide on identical path-less messages, defeating per-file frequency analysis. Future contributors copy-paste the `PathBuf::new()` pattern into new constructors, propagating the silent-path-loss class.
- **Related**: `docs/code-reviews/session-02-plan-round3.md § R3-T10`; `docs/plans/session-02.md § Deliverable 1c` (constructor sites).

---

### TD-008 — `photohelper-raw::decode` constructors carry `#[allow(dead_code)]` until the FFI body commit consumes them

- **Status**: Closed (2026-05-28, session 2; Deliverable 1a-decode body commit removed every `#[allow(dead_code, reason = "TD-008")]` attribute from the five named decode constructors AND from the FFI extern block AND from `RawPath::as_path`. `ffi::parse_libraw_image` now calls each constructor as a non-test caller; `read_raw` is the public entry point. No transient suppression remains.)
- **Opened**: 2026-05-28 (session 2, Deliverable 1c types-only slice)
- **Stop-gap location**: five `pub(crate)` constructors in `crates/photohelper-raw/src/decode.rs` carry `#[allow(dead_code)]`:
  - `BayerPlane::new`
  - `SensorBitDepth::new`
  - `SensorLevels::new`
  - `WhiteBalance::from_libraw_cam_mul`
  - `CamRgbToXyzD65Matrix::from_libraw_rgb_cam`

  All five are exercised end-to-end by the `#[cfg(test)] mod tests` block in the same file (both happy and sad paths per the R2-T6 invariant suite). But the `dead_code` lint runs against the `--lib` target, where `cfg(test)` is stripped — so without a non-test caller, the lint fires.
- **Fundamental fix**: the next commit on `session-02/libraw-cr3-decode` is the Deliverable 1a body. That commit adds `ffi::parse_libraw_decode` (or equivalently-named) which calls each of the five constructors as part of `read_raw`'s pipeline. Removing every `#[allow(dead_code)]` on those constructors is part of that commit's diff; the lint then passes naturally.
- **Binding trigger**: the next commit on `session-02/libraw-cr3-decode` MUST remove every `#[allow(dead_code, reason = "TD-008")]` attribute on the five named constructors. If the next commit lands without removing them, this TD escalates to a CRITICAL finding at session-end review.
- **Scope estimate**: ~10 LoC (delete 5 attribute lines + their TD-008 comments) / zero risk.
- **Consequence of inaction**: trivial in isolation — the allows are inert suppressions with no behavioral effect. But violates the policy spirit (transient suppressions that linger become permanent ones; the No-Acceptable-Trade-offs Policy exists to prevent that drift). The strict one-commit binding trigger is the discipline that keeps this TD's lifetime bounded.
- **Related**: `docs/plans/session-02.md § Deliverable 1c` (constructor signatures + R2-T6 invariants); `docs/plans/session-02.md § Deliverable 1a` (FFI body, the consumer).

---

### TD-009 — `scripts/sanitize-check.sh` ships stage-1 only; embedded-preview JPEG re-check (R3-T8 stage 2) deferred

- **Status**: Open
- **Opened**: 2026-05-28 (session 2, Deliverable 3 fixture commit)
- **Stop-gap location**: `scripts/sanitize-check.sh` — does NOT yet
  perform the R3-T8 stage-2 check: extract the embedded preview JPEG
  via `exiftool -b -PreviewImage <fixture>`, then re-run the allow-list
  check against the extracted JPEG. ExifTool's `-G -a` output (stage 1)
  does NOT descend into IFD0:Preview embedded JPEGs in CR3 (the `-ee`
  flag covers EPS/PDF/MPF streams, not CR3 embedded previews).
- **Fundamental fix**: extend `scripts/sanitize-check.sh` per plan
  §Deliverable 3 § Sanitize check § R3-T8: after the top-level
  allow-list check, for each fixture run
  `exiftool -b -PreviewImage "$fixture" > /tmp/preview.jpg
  2>/dev/null || true`. If `/tmp/preview.jpg` is non-empty, run
  `exiftool -G -a /tmp/preview.jpg` and assert the same allow-list.
  Without stage 2, a fixture carrying a GPS-tagged preview JPEG inside
  a clean CR3 would ship unsanitized despite the CR3 itself being clean.
- **Binding trigger**: next session that touches the CR3 fixture set
  (e.g. adding a new fixture, refreshing the existing ones, or session
  04+ when XMP develop work lands and needs additional preview-image
  invariants) OR before the first GitHub Release tag is cut. If a
  GPS-tagged preview slips into a fixture before then, the violation is
  silent.
- **Scope estimate**: ~20 LoC in `sanitize-check.sh` / low risk.
- **Consequence of inaction**: a contributor adding a non-sanitized
  fixture with a GPS-bearing preview JPEG inside a stripped CR3 ships
  GPS data despite passing the current sanitize gate. Today's two
  fixtures from raw.pixls.us are CC0-clean (verified manually), so
  the immediate exposure is zero, but the gap remains.
- **Related**: `docs/plans/session-02.md § Deliverable 3 §
  Sanitization gate § R3-T8`; `docs/code-reviews/session-02-plan-round3.md
  § R3-T8`.

---

### TD-010 — PARTIALLY CLOSED. 2 sub-items remain (build_global + heartbeat-death-WARN in-process tests)

- **Status**: PARTIALLY CLOSED (6a, 6b, 6c, 6d, 6e-rows-2+3, 6f all done in session 03). Remaining: 6e rows 1 + 4.
- **Opened**: 2026-05-28 (session 2, Deliverable 6 deferral)
- **Partial closure (session 03)**: The following sub-items landed in session 03 D5a–D5e commits:
  1. **6a `poison_for_testing` knob** — CLOSED. Commit D5a: `Catalog::poison_for_testing(&Arc<Self>)` + 3 poison tests.
  2. **6b R2-M8 silent-ROLLBACK fix** — CLOSED. Commit D5b: explicit match on `extended_code == 1` (SQLITE_ERROR), propagates unexpected rollback failures. Note: plan cited `ApiMisuse` (rc=21); empirical test shows SQLite returns SQLITE_ERROR (rc=1).
  3. **6c HeartbeatDeathTrigger** — CLOSED. Commit D5c: `crates/photohelper-test-helpers` crate + `HeartbeatDeathTrigger` struct + D5c-ii smoke test in catalog + D5c-E2E `just test-helpers-dev-only` check. The env-var approach (T3-T7) was replaced per plan v4 with this in-process helper.
  4. **6d DN-008 6 rows** — CLOSED. Commit D5d: rows 17 (hardlink), 39 (strict+real-CR3), 42a (nested-dirs), 42b (broken-symlinks), 43 (mtime_anomalous), 49a (EX_TEMPFAIL), 49b (EX_NOPERM). Row 6 (assert_send_sync!) covered by existing static_assertions in catalog tests.
  5. **6e rows 2+3 (wal_checkpoint + file-lock op-tag)** — CLOSED. Commit D5e.
  6. **6f R2-T19** — already closed at session 01 R2.
- **Remaining stop-gap location**: `ingest.rs` — `rayon::build_global` WARN (row 1) and heartbeat-death-WARN (row 4) have no regression tests.
- **Remaining fundamental fix** (2 tests):
  - **6e row 1** (`build_global already initialized`): requires calling `run_ingest` directly from a test binary (not subprocess), so the rayon global pool persists across the two calls. Pattern: add a `#[cfg(test)]` test module in `ingest.rs` that calls `run_ingest(...)` twice. OR: refactor ingest.rs to expose a `build_rayon_pool()` function that tests can call twice. ~20 LoC.
  - **6e row 4** (heartbeat-death-WARN in-process): requires `run_ingest` to be callable in a test context where the heartbeat thread can be made to die before the walk finishes. Approach: extend `HeartbeatStop` with a `kill_for_test()` method (non-signal panic trigger), OR expose a test seam in `run_ingest` that replaces the heartbeat thread factory. ~30–50 LoC.
- **Binding trigger**: next session that touches `commands/ingest.rs` for any reason. Both tests are small (<50 LoC combined) and low-risk once the test seam is identified.
- **Consequence of inaction**: `build_global` WARN and heartbeat-death WARN have no automated regression coverage. The heartbeat-death WARN is the more operator-critical gap (operators rely on `[heartbeat]` liveness; a silent death would go undetected). Still acceptable for v0.1.
- **Related**: `docs/plans/session-03.md § D5c-ii / D5e`; `docs/code-reviews/session-02-plan-round{2,3}.md § R2-T18`.

---

### TD-011 — Session-02 session-end 8-agent multi-agent review deferred to a focused follow-up session

- **Status**: Open
- **Opened**: 2026-05-28 (session 2, session-end ship)
- **Stop-gap location**: gap, not a commit — the plan §Quality gates calls for the full 8-agent suite to fire at session-end (general-purpose / arch / reviewer / type-design / silent-failure-hunter / comment-analyzer / pr-test-analyzer / simplifier), with a 9th-agent verifier and two-round R1+R2 remediation. This session skipped that protocol because the implementation work consumed the available context budget across one workday.
- **Fundamental fix**: a focused review session that:
  1. Pulls the merged session-02 PR diff (or the equivalent `git log --first-parent` range on `main`).
  2. Fires the 8-agent suite via `/eight-agent-review session` against that diff.
  3. Consolidates findings by theme, triages by severity, runs the 9th-verifier agent.
  4. Lands remediation commits on a NEW branch (`session-NN/post-02-review`) if any CRITICAL items surface.
  5. Files the review artifact at `docs/code-reviews/session-02-round{1,2}.md` and updates this TD to closed.
- **Binding trigger**: before the first GitHub Release tag is cut OR within the next 3 sessions OR if any merged commit on `main` triggers a downstream regression that an earlier multi-agent review would plausibly have caught (whichever first). The review is most valuable BEFORE additional sessions accumulate diff that complicates the audit.
- **Scope estimate**: ~1 focused session (the full 8-agent suite + R2) / low-to-medium risk depending on what surfaces. The most likely findings categories: silent-failure patterns in the FFI error paths; type-design feedback on the `RawDecodeCause` cross-class dispatch (already filed as TD-006); test-coverage gaps for D6 already covered by TD-010.
- **Consequence of inaction**: the safety-net the 8-agent review provides at session boundaries is absent for this PR. Any subtle design issue or silent-failure pattern in the substantial new LibRaw FFI surface (~700 LoC of unsafe-adjacent code) lands without the multi-perspective check the plan-review rounds invested in. The local CI (fmt + clippy + tests + audit + unsafe-isolation + sanitize-check) catches the gross issues; the agent review catches the subtle ones.
- **Related**: `docs/plans/session-02.md § Quality gates`; `docs/quality-assurance.md § Double-review protocol`; this session's plan-review R1/R2/R3 artifacts (which DID fire — only the SESSION-END double-review is deferred).

---

### TD-012 — LibRaw AHD demosaic algorithm stop-gap for NIMA preprocessing

- **Status**: Open
- **Opened**: 2026-05-28 (session 3, D1c plan-review R1 remediation — PR1-T20)
- **Stop-gap location**: `crates/photohelper-raw/src/decode.rs::read_raw_rgb` + `crates/photohelper-ai/src/nima.rs` preprocessing call @ session 03 D1c commit. In-source: `// TD-012: AHD demosaic stop-gap`.
- **Fundamental fix**: expose `imgdata.params.user_qual` in the LibRaw FFI shim; add a `DemosaicAlgorithm` enum to `photohelper-raw`; extend `read_raw_rgb(path, alg)` to accept the algorithm selector. The develop pipeline (session 04+) requires explicit algorithm choice (AMaZE or AAHD for quality rendering); NIMA may benefit from a specific algorithm to more closely match camera-native output.
- **Binding trigger**: session 04+'s develop pipeline OR user-reported NIMA score bias traceable to demosaic quality. Cross-reference DN-022 + DN-023.
- **Scope estimate**: ~30 LoC (FFI binding + enum + API extension + tests) / low risk.
- **Consequence of inaction**: v0.1 NIMA scores are computed from AHD-demosaiced images; if the NIMA model's training distribution assumed a different demosaic algorithm, scores may be systematically shifted. For v0.1 this is acceptable (no baseline comparison exists); for v0.2+ quality benchmarking it becomes measurable.
- **Related**: `docs/discovery-notes.md § DN-022` (demosaic algorithm selection); `docs/plans/session-03.md § D1c`. (Note: DN-023 is unrelated to this TD — it covers ON DELETE CASCADE absence, not demosaic.)

---

### TD-013 — Per-cull-run audit trail absent from `cull_scores`

- **Status**: Open
- **Opened**: 2026-05-28 (session 3, stop-gap declaration — PR1-T7)
- **Stop-gap location**: `crates/photohelper-catalog/src/catalog.rs::insert_cull_score` + `cull_scores` schema @ session 03 D2b commit. In-source: `// TD-013: per-cull-run audit trail absent`.
- **Fundamental fix**: add a `cull_run_id INTEGER` column to `cull_scores` referencing a new `cull_runs` table (`id`, `scorer`, `ort_version`, `model_sha256`, `started_at`, `finished_at`, `config_json`). Each `photohelper cull` invocation creates one `cull_runs` row; each `cull_scores` row references its run. This lets users see "I ran cull 3 times; what changed between run 1 and run 3?"
- **Binding trigger**: first user report of "I ran cull twice but can't see what changed" OR before v0.3 (when cull is expected to be a recurring workflow, not a one-shot). Requires a v2→v3 catalog migration.
- **Scope estimate**: ~80 LoC (`cull_runs` table + `cull_run_id` FK + `run_cull` transaction wrapping + query path extension + tests) / medium risk (touches `Catalog` API + per-insert hot path).
- **Consequence of inaction**: users running cull repeatedly cannot compare run outcomes or trace which model version produced which score. v0.1 single-run assumption is codified in the schema; changing it later requires a migration.
- **Related**: `docs/plans/session-03.md § §Stop-gap declarations`; `docs/decisions/0002-catalog-schema-v2.md`.

---

### TD-014 — ort RC pin requires upgrade to stable 2.0.0

- **Status**: Open
- **Opened**: 2026-05-28 (session 3, D1a plan-review R1 remediation — PR1-T11)
- **Stop-gap location**: `Cargo.toml` `[workspace.dependencies]` `ort = { version = "=<pin-from-D0>", ... }` (exact RC pin from ANL-002) @ session 03 D1a commit. In-source: `// TD-014: ort RC pin; upgrade to stable 2.0.0 when released`.
- **Fundamental fix**: when ort 2.0.0 stable is published on crates.io, update `Cargo.toml` to `ort = { version = "=2.0.0" }`, run `cargo test --all-features --workspace`, and resolve any API breaks between the RC and stable. Test the golden-vector fixture to confirm inference determinism is preserved across the version bump.
- **Binding trigger**: ort 2.0.0 stable tag exists on crates.io OR before the first GitHub Release tag is cut (whichever first). Monitor: `cargo update -p ort --dry-run` in `just ci` or a separate periodic audit session.
- **Scope estimate**: ~5 LoC (version pin bump + maybe minor API fixups) / low-to-medium risk depending on ort stable's API delta from the RC.
- **Consequence of inaction**: shipping a v0.1 release binary linked against an ort RC is acceptable for early adopters but not for a stable release; RC APIs may change or RC builds may have known bugs that the stable release fixes.
- **Related**: `docs/plans/session-03.md § D1a`; `docs/analysis/ANL-002-ort-nima-preflight.md § ort version`.

---

### TD-015 — `--model-path` power-user override dropped from v0.1

- **Status**: Open (prospective — `cull.rs` not yet created; D4 deferred due to D0 ABORT + DN-026)
- **Opened**: 2026-05-28 (session 3, D1b plan-review R1 remediation — PR1-T27)
- **Stop-gap location**: Prospective — `crates/photohelper-cli/src/commands/cull.rs` will be the stop-gap location when D4 lands. The file does not exist yet (session 03 D0 ABORTed before D4 was implemented). This TD becomes actionable when DN-026 is resolved and the AI culling pipeline (D1–D4) is implemented in a future session.
- **Fundamental fix**: add `--model-path <path>` + `--model-sha256 <hex>` CLI flags to `cull`. `VerifiedModelBytes::from_path_with_sha256(path, expected_sha256)` constructor validates user-supplied models. Both flags must be provided together (model without SHA = unverified; reject). Update `ModelRegistry::load_from_path_with_sha256`.
- **Binding trigger**: first user request to supply a custom NIMA model (e.g. a fine-tuned model or a different aesthetic scorer) OR before v0.2 if power-user workflows are anticipated.
- **Scope estimate**: ~50 LoC (new constructor + CLI flag pair + validation + tests) / low risk (the verification architecture already handles this via `VerifiedModelBytes`).
- **Consequence of inaction**: users cannot supply custom ONNX models for `photohelper cull` in v0.1. Acceptable for the first release (bundled model only); becomes a UX limitation for power users in v0.2.
- **Related**: `docs/plans/session-03.md § D1b`; `docs/code-reviews/session-03-plan-round1.md § PR1-T27`.

---

### TD-016 — `HeartbeatStop` + `heartbeat_loop` duplicated in `cull.rs`

- **Status**: Open (prospective — `cull.rs` not yet created; D4 deferred due to D0 ABORT + DN-026)
- **Opened**: 2026-05-28 (session 3, D4 plan-review R1 remediation — PR1-T33)
- **Stop-gap location**: Prospective — `crates/photohelper-cli/src/commands/cull.rs` will carry the duplicate when D4 lands. The file does not exist yet (session 03 D0 ABORTed before D4 was implemented). This TD becomes actionable when the AI culling pipeline is implemented in a future session.
- **Fundamental fix**: extract `HeartbeatStop`, `HeartbeatHandle`, and `heartbeat_loop` into a `crates/photohelper-cli/src/heartbeat.rs` module. Both `ingest.rs` and `cull.rs` import from that module. The module is `pub(crate)`. If the `develop` or `export` subcommand (session 04–05) also needs a heartbeat, that is the trigger for the refactor.
- **Binding trigger**: session that adds a heartbeat to the `develop`, `export`, or `run` subcommand. Three consumers is the threshold for extracting the abstraction (CLAUDE.md "Three similar lines is better than a premature abstraction").
- **Scope estimate**: ~30 LoC (new module + two import updates) / zero risk.
- **Consequence of inaction**: two copies of the heartbeat scaffold drift independently. A bug fix to `ingest.rs::HeartbeatStop` must be manually applied to `cull.rs::HeartbeatStop` also. Acceptable for two consumers; must not extend to three.
- **Related**: `docs/plans/session-03.md § D4`; `docs/code-reviews/session-03-plan-round1.md § PR1-T33`.

---

## Closed

- **TD-003** (heartbeat join) — closed 2026-05-28 in session 2 (see entry above for the remediation).
- **TD-008** (decode constructor dead_code) — closed 2026-05-28 in session 2 (see entry above for the remediation).
