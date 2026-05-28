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

### TD-002 — `rusqlite` pinned at 0.32 instead of plan-v5 target 0.40 (CVE exposure)

- **Status**: Open
- **Opened**: 2026-05-28 (session 1)
- **Stop-gap location**: `Cargo.toml` `[workspace.dependencies]` `rusqlite = { version = "0.32", features = ["bundled"] }` @ commit `310f753` (initial implementation)
- **Fundamental fix**: bump to `rusqlite = "0.40"` (or whatever the latest version is at remediation time); run `cargo update -p rusqlite`; verify `just ci` stays green (rusqlite 0.40 is API-compatible for `Connection::open` / `execute` / `query_row` / `Transaction` / `params!` — the operations photohelper uses); confirm `cargo audit` does not flag the newer bundled SQLite version.
- **Binding trigger**: bump by **2026-08-01** OR before session 02 introduces new catalog schema columns (whichever first). Session 02 will modify `Catalog::upsert` paths anyway — bundling the dep bump into that change minimizes churn.
- **Scope estimate**: ~5 LoC (Cargo.toml + Cargo.lock auto-update + possibly a few rusqlite API-rename touchups if 0.32→0.40 deprecates anything we use) / low risk
- **Consequence of inaction**: any SQLite CVE released after rusqlite 0.32's bundled-amalgamation cutoff (mid-2024) will fail `cargo audit --deny warnings` → fail CI → emergency bump under time pressure. Sitting on a 14-month-old SQLite bundle is exactly the silent-failure-via-stale-dep pattern `cargo audit` exists to surface.
- **Related**: `docs/discovery-notes.md` DN-007 (cross-reference); `docs/code-reviews/session-01-round1.md § T5` (the finding that surfaced this).

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

## Closed

- **TD-003** (heartbeat join) — closed 2026-05-28 in session 2 (see entry above for the remediation).
- **TD-008** (decode constructor dead_code) — closed 2026-05-28 in session 2 (see entry above for the remediation).
