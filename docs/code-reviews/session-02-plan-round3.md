# session-02 plan-review Round 3

> Per `docs/quality-assurance.md § Plan-review protocol § Double-review
> protocol`. Cadence A → Tier 5 plan-review. **Round 3 fires when Round 2
> remediation introduces regressions large enough to need another cycle.**
> Full 8-agent suite reduced to 7 (skip type-design-analyzer — R2 closure
> verified; skip duplicate comment-analyzer pass) fired in parallel against
> `docs/plans/session-02.md` v3 (1000 lines, post-R2 remediation).
> Findings consolidated by **theme**.

```yaml
session_config:
  schema_version: 1
  model_claimed: "Opus 4.7 [1m]"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
  ask_user_question_id: null
  user_response: option-1
  gate_state: pass
  cache_used: true
```

```yaml
plugin_availability:
  schema_version: 1
  agents_requested:
    - general-purpose
    - feature-dev:code-architect
    - feature-dev:code-reviewer
    - pr-review-toolkit:silent-failure-hunter
    - pr-review-toolkit:comment-analyzer
    - pr-review-toolkit:pr-test-analyzer
    - pr-review-toolkit:code-simplifier
  agents_unavailable: []
  agents_skipped:
    - pr-review-toolkit:type-design-analyzer (R2 type-design CRITICALs verified closed; new types in v3 are well-shaped per R3 architect/reviewer reads)
  fallback_used: false
  fallback_agents: []
```

## Triage summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 7 | 1 audit-trail fabrication regression (R2-T1 anti-pattern reborn); 1 LibRaw symbol fabrication; 1 lint-violation (workspace `panic = "warn"`); 1 silent-failure (heartbeat env-var); 1 design hole (SensorBitDepth); 1 cargo lint coordination (unused_crate_dependencies); 1 dispatch-site routing (RawDecodeCause) |
| **HIGH** | 9 | Sanitize-check exiftool `-ee` doesn't descend into preview; §4a atomic commit missing 5 files; PathBuf::new() empty error path; Acceptance 8 mechanism mislabeled as "Workspace lint"; cite-target drift (R2-T8/T4 misattribution); TD-001 verb+note contradiction; `superseded`/`skipped_non_raw`/dead `no_exif` silent-failure surfaces |
| **MEDIUM** | 8 | Plan-revisions log v3 still 33 bullets (R2-T21 partial); per-counter table inconsistent labels; `Decision-doc 0001` § History vs § Amendments name collision; channel-mapping comment at :202 stale; sad-path script missing Deliverable owner; test coverage gaps (WhiteBalance negative-value branch; ColorMatrix all-zero; BayerPlane happy-path; op-tag discrimination); rusqlite trigger >15 vs >20 inflation |
| **LOW** | 4 | Polish; duplicated bullets in revisions log; heading parallelism; trybuild row mechanical |
| **NOTES** (strengths + new bug classes) | 4 | See below |

7-agent suite labels: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:silent-failure-hunter` (sfh),
`pr-review-toolkit:comment-analyzer` (com),
`pr-review-toolkit:pr-test-analyzer` (test),
`pr-review-toolkit:code-simplifier` (simp).

---

## CRITICAL

### R3-T1 — Phantom R2-* IDs in plan v3 (R2-T1 anti-pattern REBORN at R3 level)

**Agents**: gp + com + test + simp (4-way CRITICAL convergence); orchestrator self-verified via grep.

Plan v3 cross-references **R2-S2, R2-T26, R2-PT2, R2-PT3, R2-PT4, R2-PT7, R2-PT8** at multiple locations. Orchestrator grep against the R2 artifact: R2 defines findings **R2-T1 through R2-T23**, **R2-M1 through R2-M12**, **R2-L1 through R2-L6** — no `R2-S#`, no `R2-PT#`, no `R2-T26` exist. Verified via `grep -oE "R2-(T|M|L|PT|S)[0-9]+" docs/plans/session-02.md | sort -u`:
- `R2-S2` (1 occurrence at `:112`) — invented prefix for "syntax"
- `R2-T26` (3 occurrences at `:379, :532, :953`) — does not exist
- `R2-PT2/PT3/PT4/PT7/PT8` (multiple occurrences) — invented prefix for "pr-test-analyzer"

This is the exact R2-T1 anti-pattern reborn: while remediating v2's phantom PR1-T# IDs, the v3 write fabricated 7 new R2-* IDs. The v3 plan-revisions log at `:969` boasts "Phantom PR1-T# IDs corrected throughout (R2-T1)" while simultaneously introducing the same fabrication class.

**Self-criticism**: the R3 orchestrator (this Claude session) authored plan v3 and committed exactly this anti-pattern. R3 agents caught it via cross-agent convergence; orchestrator grep verified.

**Remediation**: grep + correct every fabricated ID. Recommended mapping:
- `R2-S2` → drop (no R2 owner; the unsafe-code Cargo syntax fix is implementer-discretion polish)
- `R2-T26` → drop (the `unused_crate_dependencies` lint addition belongs to R2-T8's atomic commit shape; cite R2-T8 if needed)
- `R2-PT2` → `R2-M6` (IngestOutcome counter-wiring)
- `R2-PT3` → `R2-M10` (build-system static-link predicate)
- `R2-PT4` → `R2-M11` (ExifMalformed orientation/height coverage)
- `R2-PT7` → `R2-M7` (the row already cites M7; drop the duplicate)
- `R2-PT8` → drop or `R2-M11` (CR2 wrong-format test)

### R3-T2 — `libraw_get_cdesc` is a fabricated LibRaw C symbol (R2-T15 anti-pattern reborn at R3)

**Agents**: arch (CRITICAL, single-agent high-signal); verified via LibRaw upstream `src/libraw_c_api.cpp`.

Plan v3 `:111` enumerates `libraw_get_cdesc (CFA pattern string)` as one of ~15 named C-API accessors. LibRaw's actual `libraw_get_*` function set is 12 functions: `libraw_get_iparams`, `libraw_get_lensinfo`, `libraw_get_imgother`, `libraw_get_decoder_info`, `libraw_get_raw_height`, `libraw_get_raw_width`, `libraw_get_iheight`, `libraw_get_iwidth`, `libraw_get_cam_mul`, `libraw_get_pre_mul`, `libraw_get_rgb_cam`, `libraw_get_color_maximum`. **`libraw_get_cdesc` is not among them.**

Compounding: even reading `cdesc` correctly (via `libraw_get_iparams()->cdesc`) returns a *color-channel-naming string* (e.g. `"RGBG"`), NOT a *2x2 CFA mosaic pattern*. The CFA mosaic is in `imgdata.idata.filters` (32-bit bitmask) — NOT in `cdesc`. The plan's `:882` test row asserts `CfaPattern` derivability from `cdesc[4]` — impossible because R8 cdesc = `"RGBG"` for all 4 mosaic variants (RGGB, BGGR, GRBG, GBRG).

This is the R2-T15 symbol-fabrication pattern reborn for a different symbol.

**Remediation**: drop `libraw_get_cdesc`; add `libraw_get_iparams()->filters` for CFA discrimination via `LIBRAW_COLOR(filters, row, col)` recipe; rewrite the test row at `:882`.

### R3-T3 — `heartbeat_loop` panic site violates workspace `panic = "warn"` lint; R2-T18 4/4 closure structurally fails CI

**Agents**: rev (CRITICAL).

Plan v3 `:721` declares `panic!("heartbeat death triggered by ...")` inside `heartbeat_loop` — a production-path function with no `#[cfg(test)]` gate. Workspace `Cargo.toml:86` declares `panic = "warn"`, escalated to error by `cargo clippy --all-targets --all-features --workspace -- -D warnings` per CLAUDE.md. The atomic commit at §4a + §6c will fail `just ci` on the clippy gate. Acceptance criterion 1 (`just ci` green) becomes unsatisfiable; R2-T18 4/4 closure is false-by-construction.

**Remediation**: pick one:
(a) Add `#[allow(clippy::panic)]` at the panic site with one-line justification + file `TD-005 — heartbeat env-var panic is a test-affordance in a production-path function` with binding trigger.
(b) Replace `panic!()` with `std::process::abort()` (different lint surface; `panic = "warn"` doesn't fire) — but this skips the panic-handler stderr emission, breaking the test's substring assertion.
(c) Move the panic site behind a `#[cfg(debug_assertions)]` gate so release builds never see the panic (closes the R3-T1-sibling silent-failure surface that ANY user setting the env-var would DoS production).

Recommend (c) — strongest discipline; release-build immune to env-var DoS.

### R3-T4 — `unused_crate_dependencies` workspace lint addition will fail CI on existing `photohelper-core/Cargo.toml`'s unused `trybuild` dev-dep

**Agents**: rev (CRITICAL).

Plan v3 §4a item 6 (`:532`) commits "ALL of (1-7) in one commit so `just ci` is green at every commit boundary" and adds `unused_crate_dependencies = "warn"` to workspace lints. But `crates/photohelper-core/Cargo.toml:31` declares `trybuild.workspace = true` as a `[dev-dependencies]` entry with the explicit comment "declared but unused in this session." The new lint fires on every declared dep not consumed via `use` — `grep trybuild crates/photohelper-core/` returns ONLY the Cargo.toml line. The atomic commit will FAIL CI immediately.

Per Deliverable 6d row 6 ("DN-008 row 6: `trybuild` compile-fail test for `assert_send_sync!(Arc<Catalog>)` invariant") the trybuild test is in-scope; but the row is in a different commit than §4a. Ordering matters.

**Remediation**: extend §4a's atomic commit to ALSO land the row-6 trybuild test in lockstep (item 8 in the 7-file list becomes 8-file). OR sequence: trybuild test lands FIRST (separate commit; consumes the dep), THEN §4a's atomic adds the lint. Plan must say which.

### R3-T5 — `SensorBitDepth(u8)` "constrained 8..=16" invariant is comment-only; no fallible constructor; `SensorLevels::new`'s `1u32 << bit_depth.0` runtime-panics on `bit_depth.0 >= 32`

**Agents**: arch + rev + test (3-way CRITICAL).

Plan v3 `:266` declares `pub struct SensorBitDepth(u8); // constrained 8..=16`. No `impl SensorBitDepth { pub(crate) fn new(...) -> Result<Self, Error> }` is specified. `SensorLevels::new` at `:278` computes `(1u32 << bit_depth.0) - 1`; for `bit_depth.0 == 32`, this is a RUNTIME PANIC in debug builds (shift overflow per RFC 560). The "panic = warn" clippy lint doesn't catch arithmetic-derived panics. R2-T6 type-design anti-pattern (invariant in comment, not in code) reborn for SensorBitDepth specifically.

**Remediation**: spell out a fallible constructor in the plan body matching the `WhiteBalance` shape:
```rust
impl SensorBitDepth {
    pub(crate) fn new(value: u8) -> Result<Self, Error> {
        if !(8..=16).contains(&value) {
            return Err(Error::RawInvalidBitDepth { value });
        }
        Ok(Self(value))
    }
    pub fn get(&self) -> u8 { self.0 }
}
```
Add `Error::RawInvalidBitDepth { value: u8 }` variant. Add Test plan row asserting `new(7) == Err`, `new(8) == Ok`, `new(16) == Ok`, `new(17) == Err`.

### R3-T6 — `RawDecodeCause` variants have NO dispatch-site routing in §4d; `RawImageDimensionMismatch` / `RawInvalidLevels` / `RawPath` similarly absent

**Agents**: sfh (CRITICAL).

Plan v3 §4d's strict predicate (`:593-601`) and §4c per-counter semantics table (`:574-588`) enumerate routing for `RawExifCause` variants only. `Error::RawDecodeFailed { cause: RawDecodeCause }`, `Error::RawImageDimensionMismatch`, `Error::RawInvalidLevels`, `Error::RawPath` — all silently fold into `errored` via "any other unhandled error" catch-all. The R2-T6 invariants the plan invested in (`WhiteBalanceUnloaded`, `ColorMatrixUnloaded`) lose their discrimination signal at the dispatch boundary.

Future contributor adding a new variant gets NO compile-error signal because `#[non_exhaustive]` forces a wildcard arm at the dispatch site. The R2-T20 per-counter semantics table is incomplete.

**Remediation**: add explicit dispatch routing rows in §4d for every error variant `read_cr3` and `read_raw` can return. EITHER split the error enum so `read_cr3` returns only EXIF-class errors (type-level guarantee) OR extend §4d to enumerate every cross-call combination explicitly.

### R3-T7 — Heartbeat panic test asserts `.success()` on a process that panics — directly self-contradictory

**Agents**: sfh (CRITICAL).

Plan v3 `:728` (Deliverable 6c body) AND `:889` (Test plan row) both assert:
```rust
.env("PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING", "1")
.args(["ingest", fixture_dir])
.assert().success()
.stderr(contains("heartbeat death triggered"))
```

Either:
- Process exits 0 because panic was swallowed inside the spawned heartbeat thread → test passes but does NOT prove the WARN path fires (R2-T18 4/4 closure becomes false-by-construction AGAIN).
- Process exits non-zero because panic aborts → `.success()` assertion fails before reaching stderr check; implementer flips to `.failure()` and the test passes for ANY panic on ANY thread.

The plan does not specify: does the heartbeat thread panic kill the parent process? Is there a `JoinHandle::join()` somewhere that escalates? Per `ingest.rs:136` existing pattern, `thread::spawn(move || heartbeat_loop(...))` does NOT join — the parent process survives. So `.success()` is achievable, but the panic message reaching stderr depends on Rust's default panic handler which writes to stderr — works for `panic = "unwind"` but R3-T3 may switch to `abort` which has different stderr emission.

**Remediation**: pin the expected exit shape + WARN-emission contract:
1. State explicitly what happens to parent process when heartbeat thread panics (degraded-continue exit 0 OR clean-abort non-zero).
2. Update assertion to match.
3. Add sub-row pinning to parent-emitted WARN substring, NOT the panic-site message (panic message may never reach stderr if `panic = "abort"`).

---

## HIGH

| ID | Theme | Agents | Citation | Remediation summary |
|----|-------|--------|----------|---------------------|
| R3-T8 | `exiftool -ee` does NOT descend into IFD0:Preview embedded JPEGs in CR3; R2-T9 PII gap structurally unmet | rev | `:501, :893` | Two-stage check: `exiftool -b -PreviewImage` extracts preview blob, second exiftool run on the blob with allow-list |
| R3-T9 | §4a atomic commit missing kamadak-exif comment scrubbing in `ingest.rs:212, 349` + `tests/cli.rs:65, 411, 432` + SESSION-STATE.md/HANDOFF_REPORT.md doc updates | rev | `:527-533` | Extend §4a's enumeration to 9-10 items (add CR3 test assertion flip; add comment scrub; add doc updates) |
| R3-T10 | All constructor-time error variants use `PathBuf::new()` (empty path); Display renders "RAW image decode failed at : ..." with no file path; operator log line is unactionable | sfh | `:219-223, :271, :275, :280, :305, :312, :341, :348, :393` | Constructors take `path: &Path` as first arg, OR caller `with_path()` enricher pattern |
| R3-T11 | Acceptance criterion 8 calls a CI grep gate a "Workspace lint"; no such rustc/clippy lint exists | rev + gp | `:870-871` | Rewrite Acceptance 8 mechanism description; reference the actual `scripts/check-no-test-helpers.sh` + `! rg "cfg\(any\(test, feature"` shell gate |
| R3-T12 | Cite-target drift: R2-T8 (RAW_EXTS) misattributed at `:116, :836` for `forbid(unsafe_code)` ratchet; R2-T4 (LibRaw version) misattributed at `:879` for path-encoding boundary-pair | gp + com | `:116, :836, :879` | Strike misattributions; cite PR1-T21 alone for forbid semantics; drop cite at `:879` |
| R3-T13 | TD-001 verb `unchanged` contradicts Note ("actions/checkout SHA-pin lands"); verb-taxonomy `unchanged = no action this session` semantically violated | com | `:945` | EITHER refine taxonomy to distinguish trigger-state from codebase-action; OR introduce 5th verb `trigger-unfired`; OR change TD-001 verb to `partial` with binding-trigger explainer |
| R3-T14 | `superseded` excluded from `--strict` despite representing unexpected catalog state (content changed under operator's feet) | sfh | `:579, :593-601` | Add `superseded > 0` to predicate OR add `--allow-superseded` flag + document semantic |
| R3-T15 | `skipped_non_raw` silently absorbed by `--strict`; user with mixed-content directory gets no signal that JPEGs/CR2 were ignored | sfh | `:584, :593-601` | Add `--strict=skip-non-raw` flag OR file DN for the CR3-only-era fail-open hole |
| R3-T16 | Dead `no_exif: 0` counter in summary_line misleads operators ("we checked" signal where we never check) | sfh | `:582, :600, :629, :811, :826` | Remove `no_exif` from summary line entirely OR rename to make "we didn't check" explicit |

---

## MEDIUM

| ID | Theme | Citation | Remediation summary |
|----|-------|----------|---------------------|
| R3-M1 | Plan-revisions log v3 entry is 32-33 bullets (R2-T21 demanded ≤8); claims "trimmed (R2-T21)" but it's 4× the cap | `:968-1000` | Shrink to 8 thematic bullets OR document the exception |
| R3-M2 | Per-counter semantics table mixes label conventions (`(none)`, `INFO`, `WARN \`event=...\``, `—`); `no_exif` row uses `—` | `:582` | Normalize to `(none)` for no-event rows |
| R3-M3 | Plan v3 §5c § History append vs decision-doc 0001's existing § Amendments section — naming collision | `:670-679` + `0001-catalog-schema-v1.md:152` | Pick one section name; align plan grep target |
| R3-M4 | Channel-mapping comment at `:202` says "RGGB-order" — exact phrase R2-T6 explicitly warned was misconception; actual struct body at `:289-298` correctly says "R/G1/B/G2 NOT RGGB" | `:202` | Fix the stale comment |
| R3-M5 | `gen_sad_path_fixtures.sh` referenced at `:880, :884, :992` but no §Deliverable commits to creating it | `:880, :884, :992` | Add Deliverable 3 sub-bullet OR fold into Deliverable 6 |
| R3-M6 | `WhiteBalance::from_libraw_cam_mul` test row covers all-zero + NaN but NOT negative-value branch | `:882` | Add `[-0.5, 1.0, 1.0, 1.0]` → `WhiteBalanceInvalid` test row |
| R3-M7 | `CamRgbToXyzD65Matrix::from_libraw_rgb_cam` test row covers identity + NaN-entry but NOT all-zero matrix | `:882` | Either reject all-zero in constructor + test; OR document admission |
| R3-M8 | R2-T14 trigger silently inflated from R2-L1's recommended `>15` to plan v3's `>20`; not acknowledged in revisions log | `:913, :103` | Either tighten to >15 OR explain >20 buffer |
| R3-M9 | Acceptance criterion 8 + new `scripts/check-no-test-helpers.sh` have NO §Test plan row asserting the script's own correctness; CI gate without self-test | `:866-871` | Add Test plan row with canary stub assertion |
| R3-M10 | Pre-flight commit-message grep (`cve-posture:` / `pass-rate:`) has no §Test plan row; session-end can't enforce the contract mechanically | `:88-90` | Add Test plan row asserting `git log --grep='cve-posture:' ...` returns ≥ 1 |
| R3-M11 | Env-var `v == "1"` silently ignores `"true"` / `"yes"` / `"TRUE"` / `" 1"` (whitespace) — silent test no-op | `:715-717` | Document strict-`"1"`-only contract; OR switch to fail-loud `panic!(invalid value)` on garbage; OR permissive truthy parser |
| R3-M12 | `BayerPlane::pixel(x, y) -> Option<u16>` happy-path coverage absent; only OOB tested; refactor returning `None` for all inputs would pass OOB test | `:882` | Add `pixel(0, 0) == Some(known_value)` assertion |

---

## LOW

| ID | Theme | Citation | Note |
|----|-------|----------|------|
| R3-L1 | Plan revisions log has 2 duplicate bullets about hardlink dedup (R2-M8 cited twice) | `:997, :1000` | Delete one |
| R3-L2 | §6c `panic_on_first_tick` flag-name misleading: panics every tick, not first | `:715-724` | Rename to `should_panic` |
| R3-L3 | Heading parallelism: §6a/§6f have no "(per R2-T#)" while §6b/§6d/§6e do | `:683, :753, vs :699/:730/:743` | Document convention OR apply uniformly |
| R3-L4 | `chunks_exact(w)` in `BayerPlane::rows()` semantically misleading (says "exact" but truncates silently); invariant is in `new()`, not in `rows()` | `:242-245` | Use `chunks(w)` instead; same semantics for valid inputs, clearer intent |

---

## Strengths preserved [NOTES]

Confirmed by R3 — must not regress in any R4:

- **Substantive R2 CRITICAL closures**: R2-T2 (DN-016/017/018 filed) + R2-T4 (LibRaw 0.21.4 pin) + R2-T5 (BayerPlane fallible accessors) + R2-T6 (WhiteBalance/ColorMatrix newtypes — modulo R3-M6/M7) + R2-T7 (Error::Exif recycle + Box::new(e) syntax) + R2-T8 (atomic commit shape — modulo R3-T9 gaps) + R2-T9 (sanitize allow-list intent — modulo R3-T8 -ee scope) + R2-T13 (op tags) + R2-T14 (C-API accessors — modulo R3-T2 cdesc fabrication) + R2-T16 (memory SLO + ownership) + R2-T17 (conjunctive SQL) + R2-T19 (gen_sad_path_fixtures.sh referenced) + R2-T20 (per-counter semantics table — modulo R3-M2 + R3-T6 dispatch gaps) + R2-T22 (§ deletion) + R2-T23 (commit-scope §).
- **DN-016/017/018 + TD-004 ledger filings verified** in `docs/discovery-notes.md:134-156` + `TECH-DEBT.md:71`; binding triggers match plan v3 cross-references.
- **R2-T15 `libraw_open_wfile` correction** verified at `:106-107`; the fabricated `libraw_open_file_w` is gone.
- **R2-T18 mechanism switch** to env-var addressed the unreachable-from-subprocess concern (modulo R3-T3 lint violation + R3-T7 contradictory assertion).
- **Decision-doc 0001 § Amendments** correctly amended for migration framework reschedule.

---

## New bug classes surfaced [NOTES]

1. **"R(N)-* fabricated IDs in R(N)-T1's own remediation"** [R3-T1]: closing R2-T1 (phantom PR1-T# IDs) opens R3-T1 (phantom R2-* IDs). Same pattern, one level deeper. **General lesson**: every plan-review round's remediation work must include a `grep -oE "R(N-1)-[A-Z]+[0-9]+" plan | sort -u` cross-check against the R(N-1) artifact's heading enumeration BEFORE committing. Add to plan-review skill pre-commit hook.

2. **"LibRaw symbol fabrication recurrence"** [R3-T2]: R2-T15 corrected `libraw_open_file_w` → `libraw_open_wfile`; R3-T2 reveals `libraw_get_cdesc` is also fabricated. The orchestrator inventing C symbols is a recurring failure mode. **General lesson**: every external API symbol cited in a plan should be verified via direct file-system grep of the vendored source OR via fetched upstream documentation BEFORE the plan-write commit. Add to plan-review checklist.

3. **"Lint violation through remediation"** [R3-T3, R3-T4]: R2 remediation added two compile-time-fatal lint violations (`panic = "warn"` violated by env-var hatch; `unused_crate_dependencies = "warn"` violated by existing dev-dep). Both are remediation work that fails the very `just ci` gate the plan claims as Acceptance 1. **General lesson**: every plan-revision adding workspace lints OR `panic!`/`unwrap!`-shaped surfaces should be sanity-checked against the existing workspace lint set + dep declarations BEFORE commit.

4. **"R2 closure with derivative R3 silent-failure"** [R3-T6, R3-T7, R3-T10, R3-T16]: R2 type-design + Error-enum work was structurally correct but introduced 4 distinct silent-failure surfaces in v3 (RawDecodeCause unrouted; panic-test contradictory assertion; PathBuf::new() empty path; dead no_exif counter). **General lesson**: type-design remediation should be accompanied by an end-to-end dispatch/display trace through the call stack to catch derivative silent failures.

---

## Disposition summary

| Disposition | Count | Notes |
|-------------|------:|-------|
| **Fix inline in R3 remediation (plan v4)** | 7 CRITICAL + 9 HIGH + 8 MEDIUM | All sub-day plan edits; most are ≤30-LoC patches. |
| **Cross-doc filings required** | 1 (R3-T3 — file TD-005 for `panic = "warn"` allow + remediation plan) | Atomic with plan v4 commit. |
| **Accept-as-is** | 4 LOW | Polish + duplicates. |
| **R3 carry-forward (acceptable per R3 simp + agent consensus)** | R3-M1 (plan-revisions log bullet count) | R2-T21 explicitly authorized for v4 carry-forward. |

---

## R4 trigger assessment

Per `docs/quality-assurance.md § Double-review protocol`: "If Round 2 surfaces regressions large enough to need another cycle, add Round 3." Symmetrically for R3 → R4.

R3 surfaced **7 CRITICAL regressions** inside R2 remediation. Three are fabrication-class (R3-T1 phantom IDs, R3-T2 fabricated LibRaw symbol, R3-T12 cite-target drift) — exactly the bug class that has recurred at every round (R2-T1 caught it in v2 → R3-T1 caught it in v3 → R4 would likely catch it in v4). Three are design-class (R3-T3 lint violation, R3-T5 SensorBitDepth, R3-T6 RawDecodeCause routing) — first-occurrence; remediation is bounded. One is silent-failure-class (R3-T7 assert success on panicking process) — also first-occurrence.

**Agent consensus on R4**: 6 of 7 R3 agents converge on "R4 NOT required IF v4 remediation cleanly closes R3 CRITICALs." 1 agent (sfh) recommends "single-pass silent-failure sweep after R3 remediation, before declaring R3 closed."

**Diminishing-returns observation**: each round closes most prior-round CRITICALs but introduces ~3-7 new ones. Continuing indefinitely is not productive; at some point the residual gap must be accepted as TDs OR the plan-review protocol itself revised.

---

## Verification (orchestrator-performed; §6 substitute for cost reasons)

| Finding | Verification | Result |
|---------|--------------|--------|
| R3-T1 phantom R2-* IDs | `grep -oE "R2-(T|M|L|PT|S)[0-9]+" docs/plans/session-02.md | sort -u` → 7 fabricated IDs present (R2-PT2/PT3/PT4/PT7/PT8, R2-S2, R2-T26); R2 artifact has T1-T23, M1-M12, L1-L6 only. | **Confirmed.** |
| R3-T2 `libraw_get_cdesc` fabrication | LibRaw upstream `src/libraw_c_api.cpp` enumerates 12 `libraw_get_*` functions; `libraw_get_cdesc` not present. | **Confirmed (per arch agent verification).** |
| R3-T3 `panic` lint violation | `Cargo.toml:86` declares `panic = "warn"`; workspace `-D warnings` in CI per CLAUDE.md. Plan `:721` adds literal `panic!()` in non-test function. | **Confirmed (lint will fire).** |
| R3-T4 `unused_crate_dependencies` collateral | `crates/photohelper-core/Cargo.toml:31` declares `trybuild.workspace = true` as unused dev-dep. | **Confirmed via plan-text grep.** |

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings_high_impact: 4
  verified: 4
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    Orchestrator-performed §6 substitute via direct grep + cross-doc
    inspection for the 4 highest-impact CRITICAL items. All 4 confirmed
    present. R3 surfaced 7 CRITICAL total; the remaining 3 (R3-T5
    SensorBitDepth, R3-T6 RawDecodeCause dispatch, R3-T7 assert.success
    contradiction) are spec-internal verifications based on plan-body
    re-reading, not file-system queries. Cross-agent convergence is the
    secondary confidence signal.
```
