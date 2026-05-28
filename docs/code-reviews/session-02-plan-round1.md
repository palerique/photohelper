# session-02 plan-review Round 1

> Per `docs/quality-assurance.md § Plan-review protocol`. Cadence A → Tier 5
> (plan-review), full 8-agent suite fired in parallel against
> `docs/plans/session-02.md` v1 (commit `b377aed`, 299 lines, top block /
> session contract only). Findings consolidated by **theme** (not by agent)
> per `docs/quality-assurance.md § Consolidation discipline`. When multiple
> agents flagged the same theme, agents cited in brackets.

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
    - pr-review-toolkit:type-design-analyzer
    - pr-review-toolkit:silent-failure-hunter
    - pr-review-toolkit:comment-analyzer
    - pr-review-toolkit:pr-test-analyzer
    - pr-review-toolkit:code-simplifier
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## Triage summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 16 | Architectural, contract, and silent-failure issues that block plan v2. The plan is functional but under-specified; most CRITICALs are remediable inline. |
| **HIGH** | 17 | Convention violations, type-design gaps, scope-bundling rationales that need tightening or splitting. |
| **MEDIUM** | 14 | Polish + small refactors + cross-reference precision. Some defer to plan v3 if v2 is tight. |
| **LOW** | 9 | Hygiene + count drifts + heading-style + minor doc-comment fixes. |
| **NOTES** (strengths + new bug classes) | 8 | Confirm in R2. |

Agent suite: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer` (type),
`pr-review-toolkit:silent-failure-hunter` (sfh),
`pr-review-toolkit:comment-analyzer` (com),
`pr-review-toolkit:pr-test-analyzer` (test),
`pr-review-toolkit:code-simplifier` (simp).

The comment-analyzer slot ran TWO passes (com-1: cross-ref accuracy; com-2:
plan-as-contract testability) per the §3 8-agent budget; our
`pr-review-toolkit` lacks a distinct 8th specialized agent so the second
pass uses the same agent type with a different lens.

---

## CRITICAL

### PR1-T1 — `parse_exif_for(path, extension)` dispatch axis is wrong AND a YAGNI abstraction; silently fails for CR2/NEF/ARW/RAF

**Agents**: arch + sfh + simp (3-way CRITICAL convergence)

`ingest.rs:27` declares `RAW_EXTS = &["cr3", "cr2", "arw", "nef", "raf",
"orf", "rw2", "dng"]` (verified by 9th agent). The plan's
`parse_exif_for(path, extension)` at `docs/plans/session-02.md:92-95` routes
`*.cr3` → LibRaw and "other extensions" → kamadak-exif. By definition
that's 7 of the 8 allowed RAW formats silently routing to a parser that
on DN-006/DN-011's evidence cannot handle ANY ISO-BMFF / TIFF-derived RAW
container. A user feeding a mixed Canon CR3 + Sony ARW directory gets:
CR3 → LibRaw success; ARW → kamadak-exif `InvalidFormat` → `(default(),
true)` → `no_exif` counter bumps → NULL catalog columns → operationally
indistinguishable from "we don't support that camera," but the operator
sees `no-exif: N` and assumes "EXIF was missing" rather than "we routed
those files to a parser that doesn't speak their format." This is the
exact DN-006/DN-011 production bug, scoped down to non-CR3 RAW.

**Independently**, simp flags `parse_exif_for` as a YAGNI abstraction —
v0.1 supports only CR3, so dispatching by extension introduces an
abstraction without a second consumer (R1.T2 anti-pattern: Pipeline
trait + PipelineCtx + Sidecar enum were rejected for the same reason in
session 01).

**Remediation (plan v2)**: pick one of two paths:
(a) **Tighten `RAW_EXTS` to `["cr3"]` for v0.1** + add `parse_raw_exif(path) -> Result<ExifMetadata, Error>` (single function, no dispatch) that wraps LibRaw. Document the narrowing in CLAUDE.md "What this repo is." When other cameras land, RAW_EXTS grows in lockstep with LibRaw EXIF verification. OR
(b) **Route ALL `RAW_EXTS` through LibRaw EXIF** (LibRaw supports CR2/NEF/ARW/RAF/ORF/RW2/DNG natively). Pre-flight verify against fixtures of each format before shipping. If a format LibRaw doesn't handle, return `Error::RawFormatNotSupportedYet { ext }` not silent kamadak-exif fall-through.

Either way: kamadak-exif stays for JPEG-sidecar / non-RAW work in session 04+, called from a SEPARATE function with explicit JPEG path semantics — NOT as a fallback for unrecognized RAW.

### PR1-T2 — `Error::RawExifMissing` / `Error::RawOpenFailed` have multiple compounding issues (cause type unspecified; fail-open dispatch; LibRaw error codes coalesced; over-decomposition)

**Agents**: arch + rev + sfh + type + simp (5-way CRITICAL+HIGH convergence)

Four issues converge on these new `Error` variants:

1. **`cause` field type unspecified** (sfh CRITICAL; arch HIGH; type HIGH). If `cause: BoxedSourceError` (the existing `error.rs:17` pattern), operators lose the LibRaw numeric-code signal; if `cause: libraw_sys::Error`, `photohelper-core` gains a LibRaw dependency that breaks R2-T26's "core → ⊥" strength claim. The plan picks neither.

2. **Dispatch-site routing is unspecified** (rev HIGH; sfh HIGH). If `parse_exif_for` matches `Err(_) => (ExifMetadata::default(), true)` (the existing R2-T5-fix pattern), every `Error::RawExifMissing` silently becomes "WARN + empty EXIF + `no_exif` counter bump" — R2-T20 reborn. `--strict` then regresses to fail-open on corrupt-but-LibRaw-openable CR3s.

3. **LibRaw numeric error codes coalesced** (sfh CRITICAL). LibRaw exposes distinct codes for `LIBRAW_FILE_UNSUPPORTED` / `LIBRAW_OUT_OF_ORDER_CALL` / `LIBRAW_NO_THUMBNAIL` / `LIBRAW_MEMPOOL_OVERFLOW` / `LIBRAW_UNSUPPORTED_THUMBNAIL` / `LIBRAW_INPUT_CLOSED` — each suggesting a different operator response. Coalescing them into one `RawOpenFailed` is the same anti-pattern as R2-T3's `query_row(...).ok()`.

4. **Two variants where one + cause-enum suffices** (simp HIGH; type HIGH). Operators handle "couldn't open" and "opened but EXIF absent" identically (skip file, log WARN, continue). Two variants doubles the surface area of every match.

**Remediation (plan v2)**: collapse to one variant with a typed-enum cause:
```rust
// in photohelper-raw::Error (NOT in photohelper-core)
pub enum Error {
    RawExifUnavailable {
        path: PathBuf,
        cause: RawExifCause,
    },
}
pub enum RawExifCause {
    FileNotFound,
    UnsupportedFormat { libraw_make: String, libraw_model: String },
    CorruptInput { libraw_code: i32, offset_or_op: &'static str },
    ResourceExhausted { libraw_code: i32 },
    ExifFieldsMissing,
}
```
The variant lives in `photohelper-raw`; `photohelper-core` stays storage-agnostic. The dispatch site in `parse_exif_for` is required to `match cause` and route accordingly: `ExifFieldsMissing` bumps `no_exif`; `CorruptInput` propagates as an `errored` count (which `--strict` rejects); `UnsupportedFormat` propagates as `unknown_camera`. Add test rows asserting each cause variant exits the strict-mode gate correctly.

### PR1-T3 — DN-008 row enumeration drift + "row 32-equiv" invented + row 17 wrongly deferred

**Agents**: gp + com-1 + com-2 + test (4-way CRITICAL convergence)

9th-agent verifier confirmed (PR1-T3-A): `docs/discovery-notes.md:93`
binding trigger is the 12-row set `{6, 12, 13, 14, 17, 18, 19, 34, 39,
42, 43, 49}` (row 17 hardlink restored per R2 rewrite). Plan
`session-02.md:122` (PR1-T3-B verified) commits coverage of `{6,
32-equiv (real CR3), 39, 42, 43, 49}` — three drift classes:

1. **Row 32 is NOT in DN-008's list**. Session-01 plan row 32 is the
   "ingest CLI happy path" already closed in session 01; "32-equiv" is
   a phantom label the plan invents to make the count work.
2. **Counts don't add up**. 5 actually-DN-008 rows covered (`{6, 39,
   42, 43, 49}`) + 7 deferred (`{12, 13, 14, 17, 18, 19, 34}`) = 11, short
   of the 12 DN-008 obligated.
3. **Row 17 (hardlink) is wrongly deferred**. The plan's deferral
   rationale ("cull pipeline / dup-group catalog tables / multi-camera
   fixtures") doesn't apply to row 17 — hardlink tests need only a single
   CR3 fixture + the existing `AlreadyCatalogued` branch
   (`catalog.rs:324-330`). It's in-scope cost-wise.

DN-008's binding trigger explicitly says "Session 02's first plan commit
MUST enumerate which of these rows it intends to cover and explicit DN
cross-references for any deferred further" — the plan enumerates but
uses a non-DN-008 row identifier and omits the deferral-cross-ref.

**Remediation (plan v2)**: rewrite Deliverable 6's row-coverage commitment as:
> "Lands the `poison_for_testing` knob plus tests for DN-008 rows **{6, 17, 39, 42, 43, 49}** (6 rows). Defers DN-008 rows **{12, 13, 14, 18, 19, 34}** (6 rows) to session 03+ with explicit DN-008 cross-reference: rows 12-13 require cross-process file-lock infrastructure; rows 18-19 require dup-group catalog tables (DN-005); row 34 requires multi-camera fixtures. Total = 12 rows accounted for, matching DN-008's binding trigger. **Row 32 is a session-01 closed row; session-02 separately *flips its EXIF assertions* from `is_none()` to `Some('canon-r8')` per DN-006 closure protocol — that's an acceptance criterion (§Acceptance 2), not a DN-008 row obligation.**"

### PR1-T4 — R2-T18 closure is 3 of 4; heartbeat-death deferred via "if added"; "lock-file-create" vs "file-lock" op-tag collision

**Agents**: gp + sfh + com-1 + test (4-way CRITICAL convergence)

9th-agent verified (PR1-T4-A, PR1-T4-B): plan `session-02.md:172`
parenthesizes "(heartbeat death tested via the deferred
`panic_for_testing` knob — **if added**)"; session-01 R2 review at
`docs/code-reviews/session-01-round2.md:265-277` enumerates four R1.T10
WARN paths. Plan §Deliverable 6 claims "R2-T18 closure" but the test
plan covers only 3.

"If added" is the exact "TODO: come back to this" pattern `CLAUDE.md §
No Acceptable Trade-offs Policy` forbids. Per `docs/quality-assurance.md
§ Findings triage`: "a deferral without a plan is a CRITICAL finding on
its own."

Sub-issue: the test plan row mentions `"lock-file-create"` op-tag (the
R2-T11 sibling) while saying it covers R2-T18 (which is about
`"file-lock"`). Per `session-01-round2.md:204` these are independent
WARN paths (`File::create` vs `try_lock`); covering one doesn't close
the other.

**Remediation (plan v2)**: pick one:
(a) **Land `panic_for_testing` this session** (~10 LoC `#[cfg(test)]`
hook on `heartbeat_loop`) + write the 4th regression test. Removes the
deferral.
(b) **Downgrade R2-T18 closure to 3/4** and file new TD-004 ("heartbeat-death WARN regression test") with binding trigger "next session that touches `heartbeat_loop` OR by 2026-08-01" — same as TD-003's trigger, so bundle in TD-003. Update §Cross-references to "R2-T18 → partially advanced (3/4 WARN paths); 4th deferred via TD-004."

Either way: add a separate test row for the `lock-file-create` op-tag (R2-T11 sibling) so both paths are pinned.

### PR1-T5 — Type design: `RawExif` + `RawImage` are bag-of-public-fields anti-pattern; invariants live in tests instead of types

**Agents**: type (6 sub-findings: bag-of-fields, orientation, capture_time, NonZeroU32, cfa_pattern, levels invariant); arch (RawImage shape deferred); 7-way HIGH/CRITICAL convergence

The plan's `RawExif { make, model, orientation, capture_time_utc,
width, height }` and `RawImage { pixels, width, height, black_level,
white_level, cfa_pattern, white_balance_multipliers, color_matrix }`
are listed as field-only structs with no constructor, no privacy, no
invariants. Concrete gaps (each scored against the `PhotoId` baseline
which is 10/10/10/10):

| Field | Plan as-written | Workspace convention | Why it matters |
|-------|-----------------|---------------------|----------------|
| `orientation` | unspecified (test asserts `1..=8`) | `ExifOrientation` enum (`model.rs:354`) | Closed set of 8 values; raw int fails R2-T20 (silently-discarded-by-`if let Ok`) |
| `width`/`height` | unspecified | `NonZeroU32` | Test asserts `> 0` at runtime — type system should enforce |
| `cfa_pattern` | unspecified (test asserts `.len() == 4`) | `CfaPattern { Rggb, Bggr, Grbg, Gbrg }` enum + `ColorChannel` enum | Bayer is a closed 4-variant set; `[u8; 4]` accepts garbage |
| `capture_time_utc` | unspecified (test asserts `.is_some()`) | `Option<time::OffsetDateTime>` (workspace dep already pinned for RUSTSEC-2026-0009) | Field name implies typed datetime but plan doesn't commit; risks `Option<String>` |
| `pixels` | `Vec<u16>` (test asserts `.len() == width * height`) | Private + constructor enforces invariant; consider `Box<[u16]>` or accessor-only | Invariant lives in test, not type; unchecked indexing in session-04 demosaic |
| `black_level` / `white_level` | unspecified (test asserts `<`) | `SensorLevels { black: u16, white: u16 }` newtype | Pair invariant; division-by-zero hazard if violated |

The R2-T21 finding (`Photo::from_filesystem` accepting unverified
triples) is the canonical anti-pattern this repeats; the R2-M6 finding
(`IngestStats` 11 `pub AtomicU64` fields) is the structural twin.
Session 02 sets the precedent for every later RAW format crate adds —
under-encapsulated v1 freezes the decoder boundary forever.

**Remediation (plan v2)**: commit field privacy + fallible constructors
+ accessor methods + strong types for ALL fields. Example for `RawExif`:
```rust
pub struct RawExif {
    make: String,
    model: String,
    orientation: ExifOrientation,
    capture_time_utc: Option<OffsetDateTime>,
    width: NonZeroU32,
    height: NonZeroU32,
}
impl RawExif {
    pub(crate) fn from_libraw_fields(...) -> Result<Self, Error> { ... }
    pub fn make(&self) -> &str { ... }
    // accessors per field
}
```
Same shape for `RawImage` with a `BayerPlane` newtype carrying the
`pixels.len() == width.get() * height.get()` invariant. Plan must also
commit `Send + Sync` `static_assertions!` at module scope (not
`#[cfg(test)]`-only per R2-M2 lesson).

### PR1-T6 — Acceptance criterion #2 is unmeetable (clean-catalog assumption + non-portable path); "371/371" count is also wrong

**Agents**: gp + com-1 + com-2 + test (4-way HIGH/CRITICAL convergence)

9th-agent verified (PR1-T6-A, PR1-T6-B): `discovery-notes.md:68` records
the DN-011 trace as `walked: 371, no-exif: 370, ingested: 0,
already-catalogued: 370, skipped (non-RAW): 1`. Plan
`session-02.md:235-239` asserts `walked: 371, no-exif: 0, ingested:
371, already-catalogued: 0, skipped (non-RAW): 1`. Two compounding bugs:

1. **Arithmetically impossible without manual catalog wipe**. DN-011's
   trace already inserted 370 rows. A second run hits those rows; the
   summary cannot show `already-catalogued: 0` unless
   `/Users/ph/Pictures/tests/.photohelper/catalog.db` is deleted
   between runs — which the criterion doesn't call out as a precondition.

2. **Non-portable path**. `/Users/ph/Pictures/tests` exists only on
   the author's machine. CI cannot verify the criterion. The plan's
   git-lfs fixtures ship a 1-CR3-per-camera starter pack — not 371.
   Criterion #2 is satisfiable only on Paulo's laptop.

3. **"371/371" framing is wrong**. The plan claims (lines 21-22, 98)
   "kamadak-exif fails on 371/371 real Canon R8 CR3s" and "370/371
   files." DN-011 trace shows 370 of 370 *reached* the parser (1 file
   was skipped pre-parse as non-RAW). So it's 370/370 not 371/371.

**Remediation (plan v2)**: split into two acceptance criteria:
- **2a (CI-verifiable)**: `cargo test --workspace` against the LFS-committed CR3 fixture(s) asserts the catalog rows have non-NULL `make`/`model`/`camera_slug` + `--strict` exits 0. MUST pass in CI.
- **2b (manual smoke; recorded in PR description)**: pre-merge, the author runs `rm -rf /Users/ph/Pictures/tests/.photohelper/ && photohelper ingest /Users/ph/Pictures/tests --strict` on the 371-CR3 set (preceded by the clean-catalog precondition); copy-pastes the summary line into the PR body. Not a CI gate.
- Fix the "371/371 → 370/370" count drift at lines 21-22 and 98.

### PR1-T7 — DI-N is undefined identifier class; DI-1/DI-2 frame research as plan-review work; DI-3/DI-4 are "may surface" placeholders

**Agents**: com-1 CRITICAL + simp CRITICAL + com-2 CRITICAL + arch HIGH + rev HIGH (5-way CRITICAL convergence)

Three compounding issues:

1. **`DI-N` is not in repo conventions**. `CLAUDE.md:158-173` enumerates
   the allowed ID prefixes (`DN-NNN`, `BUG-NNN`, `ANL-NNN`, `TD-NNN`,
   ADR-NNNN, decision-NNNN). `DI-N` is invented for this plan with no
   contract. The §Out-of-scope table at line 159-160 even points
   tracking-column entries at "NEW (DI-3 below if it lands)" / "NEW
   (DI-4 below if it lands)" — circular pointers to a class with no
   binding-trigger semantics.

2. **DI-1 (LibRaw wrapper choice) and DI-2 (build mechanism) frame
   discovery work as plan-review's job**. Both say "TBD per Discovery
   item" but the actual question is empirical (crates.io maintenance
   dates, open CVEs, cross-compile feasibility). Plan-review's role is
   to *evaluate*, not *research*. Worse, the §Deliverable list
   pre-supposes the answers: Deliverable 2 already commits the static-link path; Acceptance criterion 3 ("only crate with `unsafe` blocks") pre-supposes DI-1 = (b) hand-rolled.

3. **DI-3 and DI-4 are "may surface" placeholders**. Either they're
   discovery items the plan owes plan-review (in which case they're
   mandatory and "may surface" is wrong) or they're contingencies (in
   which case they belong in §Risk register, not §Discovery items).
   Listing them at "may surface" creates work-without-payoff.

**Remediation (plan v2)**:
(a) **Rename §"Discovery items expected up-front" → §"Plan-review decisions required up-front"** and label items "Decision 1: …", "Decision 2: …" (drop the `DI-N` prefix entirely).
(b) **Author runs the DI-1 + DI-2 spikes BEFORE plan-review Round 2**: crates.io maintenance facts; open CVE list; cross-compile feasibility; recommendation with citation. Plan-review then evaluates the recommendation, not researches it.
(c) **Move DI-3 (Windows cross-compile) to §Risk register** as a real risk with concrete mitigation ("if Windows cross-compile fails, ship Linux+macOS in v0.1 per §Out of scope"); file DN-013 NOW unconditionally with binding trigger ("by v0.2 cut OR first Windows-using contributor"); drop the "if it lands" qualifier.
(d) **DECIDE DI-4 inline** (keep kamadak-exif → JPEG path for session 04+ sidecar work, OR drop it → narrow RAW_EXTS per PR1-T1) and delete the discovery-item entry.
(e) Update §Out of scope tracking column at lines 159-160 to point at the real DN-013 (not "NEW (DI-3)") and either drop the "other RAW formats" row (Canon-R8-CR3-only is project-level scope, not session-02 scope) or file DN-014 for it.

### PR1-T8 — Decision doc 0001 vs plan disagree on v1→v2 migration framework ownership (session 02 per the decision doc; session 03 per the plan)

**Agents**: gp CRITICAL + com-1 HIGH (2-way convergence)

9th-agent verified (PR1-T8-A, PR1-T8-B): decision doc 0001 Owners line
(`docs/decisions/0001-catalog-schema-v1.md:4-5`) commits **session 02**
to v1→v2 migration framework; § Migration policy at line 122-131
reads "The next change (v1 → v2 in session 02) introduces the migration
FRAMEWORK simultaneously with adding tables." Plan §Out-of-scope line
152 defers "`cull-score` + `dup-group` catalog tables + migration
framework v1 → v2" to session 03; §Cross-references line 266-267
records DN-005 as "partially advanced." The decision doc and plan
contradict; the plan does not amend the decision doc.

R2-T8 already taught the workspace that decision docs enshrining
fabricated invariants corrupt the audit trail. This is the same flaw
class: a plan asserts a scope tightening that the authoritative
decision doc disagrees with.

**Remediation (plan v2)**: pick one and execute:
(a) **Land v1→v2 framework + cull-score/dup-group tables in session 02** (matches decision doc 0001). Substantially expands session 02 scope; probably wrong given the LibRaw work already in flight.
(b) **Amend `docs/decisions/0001-catalog-schema-v1.md`** to reschedule the migration framework from session 02 to session 03. Update Owners line, § Migration policy, and Trigger-to-revisit lines in lockstep. File `docs/decisions/0002-...` is not enough — the existing decision-doc 0001 must be amended in the same commit as the session-02 plan update. Update DN-005's "Session 02 still owes" sentence accordingly.

Recommend (b). Add a Deliverable 5 sub-bullet committing to the
decision-doc amendment in the same commit as the rusqlite bump.

### PR1-T9 — Pre-flight 371-CR3 check is orphaned (in risk register only; not in deliverables, test plan, or acceptance criteria)

**Agents**: test CRITICAL + arch MEDIUM + com-1 HIGH (3-way convergence)

Risk row 1 (`session-02.md:256`) reads "Pre-flight check on the user's
371-CR3 set BEFORE writing the wire-up; if it fails, escalate scope
(raise plan-review)." This is the load-bearing precondition — if
LibRaw also fails on the user's R8 firmware, the entire plan is moot.
But:
- No Deliverable produces a pre-flight report.
- §Test plan does not include the pre-flight as a deliverable test.
- §Acceptance criteria verify END-state behavior, not feasibility.
- No artifact path is assigned (e.g. `docs/analysis/ANL-001-libraw-cr3-preflight.md`).
- §Sequencing is undefined: where in the implementation order does it fire?

An orphaned mitigation is no mitigation: a future contributor reading
the plan sees no obligation to run the probe, sees no exit criterion
if it fails, and proceeds straight to FFI wiring.

**Remediation (plan v2)**: add **Deliverable 0: Pre-flight feasibility probe**.
> "BEFORE Deliverable 4's `ingest` rewire (but AFTER DI-1/DI-2 decisions
> in Deliverable 1-2), invoke the chosen LibRaw entry against a sample
> (N≥10 OR all 371) of the user's `/Users/ph/Pictures/tests` set.
> Captured in `docs/analysis/ANL-001-libraw-cr3-preflight.md` with:
> (a) LibRaw version + commit, (b) per-file pass/fail, (c) extracted
> Make/Model/Orientation/CaptureTime per file, (d) any LibRaw errors.
> **ABORT trigger**: if any field is missing on >5% of files, raise
> plan-review v3 with scope-escalation options. The pre-flight is its
> own session-02 commit (`chore(libraw): pre-flight EXIF extraction
> against user's 371-CR3 set`) so its result is auditable in `git log`."

Update §Risk register row 1 mitigation to cross-reference Deliverable 0.

### PR1-T10 — LibRaw version "0.21+" is unbounded; cargo-audit doesn't see LibRaw CVEs

**Agents**: rev CRITICAL (1-way; only rev surfaced this)

9th-agent verified (PR1-T10-A): plan `session-02.md:57` reads "LibRaw
0.21+ headers" — unbounded floating lower-bound, no upper bound, no
exact pin. Two compounding issues:

1. **Reproducible-build discipline violation**. `CLAUDE.md` mandates
   `rust-toolchain.toml` pinning and CI-parity via `just ci`; a
   floating C-library dependency breaks the same discipline.
2. **`cargo audit` does not see LibRaw CVEs**. RustSec advisory DB
   only catalogs Rust-ecosystem crates. LibRaw has a non-trivial CVE
   history since 2020 (multiple buffer-overflow / out-of-bounds-read).
   Acceptance criterion 4 ("`cargo audit --deny warnings` clean on the
   bumped `rusqlite` + the new LibRaw build inputs") is misleading: a
   clean cargo-audit says nothing about LibRaw's CVE posture.

**Remediation (plan v2)**:
(a) Replace "LibRaw 0.21+" with an exact `=X.Y.Z` pin chosen by plan-review v2, recorded in `crates/photohelper-raw/build.rs` (vendored tarball SHA-256) AND in decision doc 0002.
(b) Add Acceptance criterion 7: "LibRaw upstream pinned to exact `X.Y.Z`; vendored tarball SHA-256 recorded at `crates/photohelper-raw/vendor/libraw-X.Y.Z.tar.gz.sha256`, verified at build-time."
(c) File TD-004 ("LibRaw C-library CVE monitoring is manual until a CVE-DB scanner is wired") with binding trigger ("first session touching `photohelper-raw` after 2026-08-01 OR on any LibRaw CVE disclosure").
(d) Add to decision-doc 0002: "LibRaw CVE-monitoring mechanism is manual via upstream GitHub releases + Security Advisories subscription; `cargo audit` does NOT cover LibRaw."

### PR1-T11 — CR3 fixture pipeline ships personal EXIF (GPS, owner name, serial number) without sanitization gate

**Agents**: rev CRITICAL (1-way; only rev surfaced this — high-signal)

9th-agent verified (PR1-T11-A): Deliverable 3 commits "license audit"
(CC0) but says nothing about EXIF PII sanitization. CR3 files routinely
contain `GPSLatitude`/`GPSLongitude`, `OwnerName`, `SerialNumber`,
`Copyright`, `LensSerialNumber`, `CameraOwnerName`, IPTC creator
fields, and embedded JPEG thumbnails (which carry the same metadata
again). CC0 is a copyright assertion, not a privacy assertion.

For a project authored by a named individual (Paulo), the most likely
fixture source is the author's own R8 CR3s — directly committing GPS
coordinates of the author's home/workplace + camera serial number to
the public git-lfs repo forever.

**Remediation (plan v2)**: extend Deliverable 3 to require sanitization:
> (a) every fixture passes through `exiftool -all= -tagsfromfile @ -Make
> -Model -Orientation -DateTimeOriginal -ExifImageWidth
> -ExifImageHeight -Software` (or equivalent) so only the fields the
> tests assert on survive; (b) GPS / owner / serial / embedded preview
> all stripped; (c) `tests/fixtures/cr3/README.md` records the
> sanitization invocation + `exiftool -G -a` "after" dump; (d) a CI
> lint at `tests/fixtures/cr3/sanitize-check.sh` (run from `just ci`)
> re-asserts no GPS/owner/serial tags appear on any fixture so an
> unsanitized drop-in is caught at PR time.

Tighten the test assertion at line 167 to assert *only* on the
sanitization-set survivors.

### PR1-T12 — rusqlite 0.32 → 0.40 "API-compatible" without enumerating 8 minor versions of breaking changes

**Agents**: sfh CRITICAL + test MEDIUM (2-way convergence)

9th-agent verified (PR1-T12-A): TD-002 Fundamental fix asserts
"rusqlite 0.40 is API-compatible for `Connection::open` / `execute` /
`query_row` / `Transaction` / `params!` — the operations photohelper
uses." Plan §Deliverable 5 cites this verbatim. But rusqlite has
historically broken `OpenFlags`, `params!`, `Transaction` lifetimes,
`TransactionBehavior` defaults, and `Error` variant set across minor
bumps. Concrete silent-failure modes:

1. **`conn.execute_batch("PRAGMA journal_mode = WAL")` semantics**.
   Some rusqlite versions return rows on this PRAGMA; if 0.40 changes
   the behavior, the WAL flip silently no-ops and the
   `wal_checkpoint` recovery branch is testing dead code.
2. **`Error` variant set additions**. The `match
   rusqlite::Error::QueryReturnedNoRows` at `catalog.rs:321,343` may
   silently dispatch to a wildcard arm if 0.40 added new variants.
3. **`OpenFlags::SQLITE_OPEN_NO_MUTEX` default changes**. Could mean
   double-locking between rusqlite's internal mutex and the workspace's
   `Mutex<Connection>`.

The risk-register row at line 259 acknowledges "API-breaks" with a
fallback to "rusqlite 0.3X intermediate" — but if 0.40 is dropped
mid-session, the bundling rationale collapses while the schema-touching
work continues.

**Remediation (plan v2)**: extend Deliverable 5 with an explicit
enumeration of every rusqlite API change between 0.32 and 0.40
touching the catalog call sites: `Connection::open` /
`open_with_flags` flag changes; `TransactionBehavior::Immediate`
signature; `params!` variadic / coercion; `execute_batch` PRAGMA row
handling; `Error` variant additions. Add Test-plan rows pinning:
(a) `PRAGMA journal_mode = WAL` read-back returns `"wal"` post-init;
(b) roundtrip test (open → write → close → re-open → read row) with
bumped rusqlite. Drop the §Risk register fallback to "rusqlite 0.3X"
OR add a binding trigger that requires re-firing plan-review when it
activates.

### PR1-T13 — `git-lfs` not fetched on fresh checkout silently passes tests

**Agents**: sfh CRITICAL + test HIGH + rev MEDIUM + com-1 MEDIUM + com-2 MEDIUM (5-way convergence)

When `git lfs install` / `git lfs fetch` hasn't run on a fresh checkout,
fixture paths exist but contain ~130-byte LFS pointer text files
(`version https://git-lfs.github.com/spec/v1\noid sha256:...`). LibRaw
called on those returns `RawOpenFailed` → test asserts the failure path
→ test passes WITHOUT exercising real CR3 (false confidence). This is
operationally identical to R2-T13 ("DN-006 fallback was implicit
blanket coverage") — implicit fixture-resolution dependency masks a
structurally different bug.

The plan's test plan row (`session-02.md:174`) reads "passes locally
with a fresh `git lfs fetch`; CI configures `git lfs install` before
checkout" — conditional on the developer having done lfs fetch; nothing
in the plan describes what happens when they haven't.

**Remediation (plan v2)**: add a fixture-sanity gate as a Test-plan
row + Deliverable sub-bullet:
> "`fixture_is_real_cr3(path)` helper: verifies the fixture file is
> ≥1MB AND first 16 bytes are NOT the LFS pointer magic
> (`version https://git-lfs`). Tests that depend on the fixture MUST
> call this helper at top; if it fails, the test PANICS with an
> actionable message ('git lfs install && git lfs fetch && git lfs
> checkout'). Silent-skip is explicitly rejected."

Also update the CI workflow path: `actions/checkout@<pinned-SHA>` with
`lfs: true` parameter (LFS objects fetched at checkout time), not a
post-checkout `git lfs install` step. Add a developer-onboarding note
to README.md: `git lfs install` is now a `cargo test` prerequisite.

### PR1-T14 — R2-T5 "succeeded but yielded zero fields" WARN gate fails on partial-EXIF (only `is_empty()` checked)

**Agents**: sfh CRITICAL (1-way; only sfh surfaced this)

The plan claims (`session-02.md:96-99`) the R2-T5 gate "stays valid
because LibRaw now genuinely populates the fields for CR3." Misread of
the gate's failure mode: the gate at `ingest.rs:345` fires `if
exif.is_empty()` — meaning **all** fields absent. If LibRaw returns
Make + Model but no `capture_time_unix_seconds` (corrupt
DateTimeOriginal box; IFD0 boxes fine), `is_empty()` returns *false*
(at least one field set), WARN doesn't fire, catalog row has NULL
`capture_time_unix_seconds`. Strict-mode at `:214` checks `no_exif > 0`
— only bumps when `is_empty()` is true. **Partial-EXIF is silently
accepted as full-EXIF.**

Acceptance criterion 2 requires non-NULL `make`/`model`/`capture_time_unix_seconds`/`width`/`height`/`camera_slug` —
a CR3 with partial EXIF passes ingest, passes `--strict`, but FAILS the
acceptance criterion silently.

**Remediation (plan v2)**: replace `is_empty()` semantics with a
structured "completeness" predicate:
```rust
pub enum ExifCompleteness {
    Full,
    Partial { missing: Vec<&'static str> },
    Empty,
}
impl ExifMetadata {
    pub fn completeness(&self) -> ExifCompleteness { ... }
}
```
Route `Partial` to its own WARN (naming the missing fields); add
`IngestStats::partial_exif` counter; `--strict` fails on `no_exif > 0
|| partial_exif > 0`. Add Test-plan row covering "CR3 fixture where
DateTimeOriginal is corrupt but Make/Model fine" → asserts
`partial_exif` bumps + `--strict` exits non-zero.

### PR1-T15 — `Catalog::poison_for_testing` visibility unspecified; risks repeating R2-T15 dead-pub-API anti-pattern

**Agents**: rev CRITICAL + sfh MEDIUM (2-way convergence)

9th-agent verified (PR1-T15-A, PR1-T15-B): plan `session-02.md:120`
introduces `Catalog::poison_for_testing` without specifying
`#[cfg(test)]`-only visibility. R2-T15 already flagged
`Catalog::open_with_retry_delay` (declared `pub fn` with `#[doc(hidden)]`,
zero callers) as the "dead-code-shipped-as-if-it-fixed-something"
anti-pattern. Repeating it in the same session that R2-T15 closed is
a regression.

If `poison_for_testing` ships as `pub fn`, it (a) inherits R2-T15
critique verbatim, (b) becomes a production surface a malicious or
buggy caller can trigger, (c) opens a one-line DoS attack against
the catalog (poison is permanent per `error.rs:111-117` — every
subsequent op returns `CatalogPoisoned`).

Sub-issue (sfh): the plan's test description "next call recovers
cleanly" contradicts the production contract: poison is PERMANENT.

**Remediation (plan v2)**: tighten Deliverable 6:
> "**`#[cfg(test)] impl Catalog { fn poison_for_testing(&self) { ... } }`**
> — visibility constrained to test builds; not part of the public
> API; not reachable from production binaries. If a test-helper must be
> reachable from integration tests in `tests/`, use `pub(crate)` +
> `#[cfg(any(test, feature = "test-helpers"))]` with the feature flag
> OFF in release builds."

Add acceptance criterion: "No `*_for_testing` method exists in any
production binary symbol table" (verifiable via `nm`). Cross-reference
R2-T15 in the plan so remediation cannot miss it. Fix the test
description to match the production contract: "poison is permanent;
every subsequent `upsert` returns `CatalogPoisoned`."

### PR1-T16 — Plan §"Detailed implementation" h2 explicitly empty; contract is partial by design

**Agents**: com-2 CRITICAL + simp MEDIUM (2-way convergence)

Lines 296-299 promote "Detailed implementation" to an h2 with body
"(intentionally empty until plan-review v1→v2 lands; the top block
above is the only thing under review at plan-review Round 1.)". Two
issues:

1. **Review-boundary confusion**. Plan-review is asked to grade
   `session-02.md`, not `session-02.md § Session contract`. Lines
   296-299 still appear in the artifact under review.
2. **Divergence from session-01**. Session-01's plan
   (`session-01.md:625-628`) places the equivalent note as a *closing
   italic prose* paragraph, not as an empty h2. Session-02 chose the
   opposite convention without recording why.

**Remediation (plan v2)**: pick one:
(a) **Match session-01**: delete lines 294-299 and append an italic
prose note at the bottom of §Session contract.
(b) **Keep h2 with explicit review-scope note**: rewrite the body to
"**This h2 is not part of the v1 plan-review scope.** Plan-review
Round 1 reviews only `## Session contract` (lines 10-292). This h2
lands in plan-revision v2 post-Round-2 remediation."

Recommend (a) — fewer lines, matches the prior session's precedent.

---

## HIGH

### PR1-T17 — LGPL sub-clause citation is wrong (§6(b) is shared-library; vendored-source path is §6(a))

**Agents**: gp HIGH (verified by 9th-agent PR1-T17-A)

Plan repeatedly cites "LGPL §6(b)" as the source-tarball-shipping
requirement (lines 78, 211, 265). LGPL-2.1 §6(b) is "Use a suitable
shared library mechanism" — the *opposite* of static-linking-plus-vendored-source. The vendored-source-plus-relink-instructions
path is §6(a) ("Accompany the work with the complete corresponding
machine-readable source code … so that the user can ... relink to
produce a modified executable"). Misattribution propagates from DN-001
(line 26 also says "§6(b)") — plan-review is the moment to catch it,
because decision doc 0002 will enshrine whichever clause the plan
cites, corrupting the legal-audit trail (same flaw class as R2-T7's
RUSTSEC misattribution + R2-T8's BEGIN IMMEDIATE doc fabrication).

**Remediation**: replace every "§6(b)" with "§6(a)" in the plan + amend
DN-001 in the same commit. Quote the LGPL-2.1 §6(a) clause verbatim in
decision doc 0002 when it lands.

### PR1-T18 — DN-001 "owned this session" contradicts §Out-of-scope deferring the release-engineering half

**Agents**: gp HIGH + rev MEDIUM (2-way)

DN-001 Owner field at `docs/discovery-notes.md:28` is two-headed:
"session that introduces `photohelper-raw` LibRaw FFI (likely session 02)
**+** the eventual release-engineering session." Plan §Cross-references
line 265 says "DN-001 → owned this session (decision doc 0002)"
(full ownership); plan §Out-of-scope line 153 simultaneously defers
the release-workflow wiring. The GitHub Release workflow IS the
artifact-shipping mechanism for the §6(a) tarball; without it,
decision doc 0002 is decision-without-implementation.

**Remediation**: replace the §Cross-references entry with "DN-001 →
**partially advanced** (decision doc 0002 records §6(a) artifact
shape; release-workflow wiring deferred to dedicated release session)."
Confirm DN-001's Status line will be updated to "partially resolved
2026-MM-DD" form at session-end. Optionally split DN-001 → DN-001a
(decision) + DN-001b (release wiring) for trackability.

### PR1-T19 — White-balance multipliers / color_matrix / black_level are camera-condition-dependent algorithmic over-promises

**Agents**: arch HIGH (1-way)

`white_balance_multipliers` is per-shot in CR3 (varies by Auto / Daylight
/ Tungsten / custom-WB at capture). LibRaw exposes FOUR distinct WB
sources (`cam_mul`, `pre_mul`, `WB_Coeffs[256][4]`, `WBCT_Coeffs[64][5]`).
A single `white_balance_multipliers: [f32; 4]` destroys the develop
pipeline's ability to recover/re-balance (a core Lightroom-equivalent
feature this project promises per CLAUDE.md). Same critique for
`color_matrix` (illuminant-dependent — Adobe DNG uses
`ColorMatrix1`/`ColorMatrix2`/`ForwardMatrix*` interpolated by CCT) and
`black_level` (Canon has per-channel + optical-black region, not a
single scalar).

**Remediation**: split WB into `as_shot_wb: WhiteBalance` + `wb_presets:
HashMap<WbTag, WhiteBalance>` (preserves re-balance capability), OR
explicitly name in the plan that v0.1 only supports as-shot WB and
expanding the type later requires a binding-trigger DN. Same treatment
for `color_matrix` (illuminant-tagged) and `black_level` (per-channel
+ optical-black region).

### PR1-T20 — FFI path encoding boundary unspecified (non-UTF-8 paths on Unix; Windows `wchar_t`; NUL byte handling)

**Agents**: arch HIGH (1-way)

LibRaw's `open_file()` takes `const char *fname`; the Rust caller has
`&Path`. Failure modes the test plan doesn't cover: NUL-byte interior
in path; non-UTF-8 path on Linux (Latin-1 from external drive); emoji
/ CJK on macOS APFS vs HFS+ normalization; Windows paths >MAX_PATH
requiring `\\?\` prefix + `open_file_w`. Real photographers have
all-emoji folder names on external drives.

**Remediation**: add explicit FFI path-encoding test rows: NUL-byte
interior → typed error; non-UTF-8 path on Unix → typed error (not
panic); non-ASCII on macOS APFS → succeeds; on Windows, `\\?\`-prefixed
long path → succeeds. Specify in the FFI module docs which conversion
is used per OS. Define a typed `RawPath` newtype that runs validation
once.

### PR1-T21 — `photohelper-raw` Cargo.toml `unsafe_code` override is missing despite the comment claiming intent

**Agents**: gp HIGH + sfh HIGH (verified by 9th-agent PR1-T21-A)

`crates/photohelper-raw/Cargo.toml:15` has `workspace = true` under
`[lints]`, which inherits the workspace-level `unsafe_code = "forbid"`
— meaning any `unsafe` block added in session 02 is a clippy error
under `-D warnings`. The crate's comment at lines 12-14 acknowledges
the intent ("Override workspace-level `unsafe_code = forbid`") but the
actual override is missing.

Sub-issue (sfh): `// SAFETY:` comments are convention-only; the clippy
lint `undocumented_unsafe_blocks` exists and is not enabled.

**Remediation**: add as Deliverable 1 sub-bullet:
> "Amend `crates/photohelper-raw/Cargo.toml [lints.rust]` to override
> `unsafe_code = "allow"` for this crate only. Add
> `#![deny(unsafe_op_in_unsafe_fn)]` at module top of `ffi.rs` so every
> `unsafe fn` body still requires an inner `unsafe { ... }` block with
> a `// SAFETY:` comment. Other modules in `photohelper-raw`
> (`exif.rs`, `decode.rs`, `lib.rs`) declare `#![deny(unsafe_code)]`
> at file head so the crate-level override applies ONLY to `ffi.rs`.
> Add `#![deny(clippy::undocumented_unsafe_blocks)]` at workspace lints
> so every `// SAFETY:` omission is a compile error. CLAUDE.md
> line 106-108 records this as expected; the override MUST land in the
> same commit as the first `unsafe` block."

Update Acceptance criterion 3 to "enforced by lints, not by convention."

### PR1-T22 — SESSION-STATE.md drift housekeeping promised "before plan-review fires" but plan-review IS firing with SESSION-STATE still stale

**Agents**: gp HIGH (verified by 9th-agent PR1-T22-A)

9th-agent confirmed: `SESSION-STATE.md:14` still says "Current session:
1 (R2 REMEDIATION APPLIED — ready for `just ci` + PR push)" despite
PR #1 having merged at `c120819`. Plan lines 185-188 promise the
cleanup "before session 02 plan-review fires" — but plan-review IS
firing now. The author has tunnel vision on LibRaw scope; the upstream
state hygiene was not honored.

**Remediation**: either (a) land the SESSION-STATE.md housekeeping
commit BEFORE plan-review Round 2 fires (and update the false-tense
claim at lines 185-188 to past-tense + cite the commit SHA); OR
(b) demote the promise to "as part of session-02 implementation (first
commit after plan-review Round 2 lands)" and note that plan-review
readers must mentally merge session-01's PR state when reading
SESSION-STATE.md.

Recommend (a). Bundle with the SCUNet reference removal (PR1-M14).

### PR1-T23 — Acceptance criterion 6 ("R2 surfaces zero CRITICAL items") is unverifiable / circular

**Agents**: arch HIGH + com-2 HIGH + rev MEDIUM (3-way convergence)

Criterion 6 is human-judgment-dependent and circular: the session-end
review is itself reviewing whether acceptance criteria are met; one
criterion is "the review found no CRITICAL items." Compare with
criteria 1-5: each is a scriptable check. Worse: criterion 6's phrasing
"MEDIUM and LOW findings ship with TD/DN entries" silently accepts
MEDIUM findings without remediation — but `docs/quality-assurance.md §
Findings triage` requires MEDIUM "before session end" (only LOW is
freely deferrable).

**Remediation**: rewrite as
> "Zero CRITICAL findings OPEN AT SESSION END (closed inline or filed
> as TD/DN with binding triggers per the No-Acceptable-Trade-offs
> Policy). MEDIUM findings remediated before session end (per
> `quality-assurance.md § Findings triage`). LOW findings ship with
> TD/DN entries OR accepted explicitly. HIGH carry-forward budget ≤ 2."

Mirror the `SESSION-STATE.md` metrics table line.

### PR1-T24 — Scope-bundling rationale partially fabricated (TD-002 trigger not actually fired by this session)

**Agents**: gp HIGH + com-1 HIGH + simp HIGH (3-way convergence)

The plan's Scope rationale (lines 38-48) justifies the rusqlite bump
with "the binding trigger (TD-002) requires the bump before the next
schema-touching session, and session 02 will modify `Catalog::upsert`
to populate the EXIF columns." But:

- TD-002 trigger at `TECH-DEBT.md:51` says "before session 02 introduces
  new catalog schema **columns**" — not "the next schema-touching
  session" and not "modifying upsert."
- Plan line 108-115 explicitly states NO new columns are added.
- So TD-002's structural trigger is NOT fired by this session; only
  the calendar trigger ("by 2026-08-01") applies — soft, not
  bundling-justifying.

The rusqlite bump is voluntary; the cross-ref calling it "closed by
the trigger" is incorrect framing. Repeats R2-T13's "stop-gap rationale
substitution" pattern.

**Remediation**: reword §Scope rationale to:
> "TD-002's calendar trigger ('by 2026-08-01') will fire before session
> 03 absent action; bundling the bump into session 02 minimizes churn
> because (a) we're already in catalog-crate code for the populate
> work, and (b) closing TD-002 inside the LibRaw landing simplifies
> the dependency story for the release session. TD-002's structural
> trigger ('before session 02 schema columns') is NOT fired because
> no schema columns are added — populate-existing-NULLs is DML, not
> DDL."

Update §Cross-references TD-002 entry to "closed this session
(voluntarily ahead of the calendar trigger; bundled with LibRaw work
for churn-minimization)."

### PR1-T25 — DN-012 polish "where naturally touched" is discipline-decay; binding triggers fire but plan leaves items unenumerated

**Agents**: simp HIGH + com-2 HIGH + gp MEDIUM + sfh MEDIUM (4-way)

"Where naturally touched" is the unfalsifiable phrasing the
No-Acceptable-Trade-offs Policy is designed to catch. DN-012's binding
trigger at `discovery-notes.md:79` lists 4 specific surfaces; session
02 will touch `Cargo.toml` (rusqlite bump → DN-012 trigger surface 2)
and `catalog.rs::UpsertOutcome` (catalog rewire → DN-012 trigger
surface 4) — so 2 of 4 DN-012 binding triggers fire. The plan commits
to 2 items ("KnownCamera Display impl + UpsertOutcome `#[non_exhaustive]`")
but does NOT enumerate disposition for the workspace-clippy-comments
item (whose trigger fires because rusqlite bump touches
`Cargo.toml`).

**Remediation**: split Deliverable 7 into:
- **7a** (concrete): KnownCamera Display + UpsertOutcome `#[non_exhaustive]` + workspace-clippy-comments item (each landed this session because trigger fires).
- Move "remaining DN-012 items (Windows case-sensitivity walker filter) deferred to next session that touches `ingest.rs::WalkBuilder`" to §Out-of-scope with explicit DN-012 cross-ref + new binding trigger ("session that touches `ingest.rs::WalkBuilder` filter OR by 2026-08-01").

Per-item enumeration replaces "where naturally touched."

### PR1-T26 — DI-4 contradicts Deliverable 4 (decision pre-made vs decision open)

**Agents**: gp MEDIUM + com-1 MEDIUM + simp HIGH (3-way; severity tightening)

9th-agent verified (PR1-T26-A): Deliverable 4 line 94 commits "other
extensions (JPEG fallback for future sidecar work) → kamadak-exif as
today" (decided: keep). DI-4 line 221 says "Plan-review decides whether
to keep it or drop it" (open). Internal contradiction.

Sub-issue (com-1): "JPEG fallback for future sidecar work" is also a
category confusion — sidecars are XMP per CLAUDE.md, not JPEG. The
kamadak-exif path is for JPEG INGEST (if a future session ingests
JPEGs), not for sidecar I/O.

**Remediation**: DECIDE DI-4 inline. If "keep": tighten Deliverable 4
wording to "JPEG ingest in future sessions (sidecar I/O is XMP, not
JPEG)"; drop DI-4. If "drop" (per PR1-T1 remediation): collapse
`parse_exif_for` to a single CR3-only LibRaw call; drop the
kamadak-exif workspace dep + the JPEG-path test; update DN-006
"kamadak-exif fallback" language to "kamadak-exif removed in
session 02."

### PR1-T27 — DI-4 silent removal would drop the R2-T26 `unused_crate_dependencies` lint regression

**Agents**: rev HIGH + sfh HIGH (2-way)

9th-agent verified (PR1-T27-A) with drift: R2-T26 mandated adding
`unused_crate_dependencies = "warn"` to workspace lints. If kamadak-exif
is dropped per DI-4 (and per PR1-T26 remediation) but the workspace
dep declaration remains in `Cargo.toml:48`, `unused_crate_dependencies`
flags it and CI fails. The plan doesn't pick up the lint addition or
co-ordinate the dep removal with the dispatch removal.

**Remediation**: in plan v2's Deliverable 5 (rusqlite bump touches
Cargo.toml anyway), add a sub-bullet:
> "Verify (or land) `unused_crate_dependencies = "warn"` in
> `[workspace.lints.rust]` per R2-T26. If DI-4 resolves to 'drop
> kamadak-exif', the dep removal MUST land in the same PR as the
> dispatch removal (Deliverable 4) — atomic, no intermediate broken
> state."

### PR1-T28 — `Vec<u16>` memory pressure for batch ingest unaddressed (96 MB peak per CR3 × 8 workers = ~800 MB transient)

**Agents**: arch CRITICAL (the agent grouped it as part of CRITICAL #2; severity is HIGH per cross-agent calibration since it's a session-04 concern)

Canon R8 full-frame raw is ~25 Mpix × 2 bytes = ~51 MB per CR3 in the
`Vec<u16>`. LibRaw's internal `imgdata.image` 4-channel buffer is ~96
MB. With rayon's 8-worker default, transient working set ≈ 800 MB
during decode. The plan's `read_raw(path) -> Result<RawImage>` is
**eager and owned** — no streaming, no `read_raw_into(path, &mut
buffer)`, no `read_metadata_only(path)`, no `read_raw_tile(path,
region)` for tile-based denoise. Session 04 will discover this and
refactor `RawImage`'s constructor surface.

**Remediation**: either (a) explicitly defer `decode::read_raw` to a
later session (ship only `exif::read_cr3` this session — the DN-011
load-bearing fix), OR (b) lock the `RawImage` ownership model with a
documented "this allocation is per-photo; downstream consumers MUST
drop before iterating to next photo" contract + a
`peak_memory_per_decode_bytes` SLO row in the test plan.

Recommend (a) per PR1-T34 (scope splitting). Bundling EXIF + decode is
a single-pass FFI economy argument, but the develop-pipeline-shape
unknowns make decode premature.

### PR1-T29 — `--strict` sad-path on real CR3 fixtures unspecified

**Agents**: test HIGH + sfh HIGH (2-way)

The plan tests `--strict` exits 0 on the happy path (LibRaw populates
all fields). It does NOT test:
- LibRaw extracts Make/Model that don't match `CameraRegistry` → `unknown-camera`.
- LibRaw extracts SOME fields but not others (`exif.is_empty()` is false but `make.is_none()`) — see PR1-T14.
- LibRaw call itself errors (`RawOpenFailed`) — what does this do to per-file stats / strict path?

A regression re-introducing R2-T12's fail-open semantics (e.g. LibRaw
`Err` → `ExifMetadata::default()` silent substitution → `--strict`
doesn't fire) would NOT be caught.

**Remediation**: add test plan rows: `strict_mode_fails_on_unknown_camera_real_cr3` (Make recognized but Model not in registry); `strict_mode_fails_on_libraw_error_real_cr3` (corrupted CR3 → strict exits non-zero, stderr cites error count); `strict_mode_fails_on_partial_exif` (per PR1-T14). Cross-reference R2-T12 in test docstrings.

### PR1-T30 — R2-T19 96KB arithmetic confusion; the test was ALREADY landed at session-01 R2 remediation

**Agents**: gp HIGH + rev MEDIUM + test HIGH (3-way; 9th-agent verified PR1-T30-A)

9th-agent verified: R2-T19's discriminating PhotoId test is ALREADY
present at `crates/photohelper-core/src/model.rs:770` — the test
function is named `photoid_derive_window_disjoint_distinguishes_overlap_region_changes` and
its rustdoc references R2-T19 directly. So the plan's claim to "close
R2-T19 via test replacement" is REDUNDANT — it's already closed in
session 01 R2 remediation commit `681a3a2`.

Additionally, test agent flagged that even if a new test were needed,
the plan's "96KB test where bytes [60KB..68KB) differ" arithmetic
discriminates only on `[61440, 65536)` (4KB), not the full 32KB overlap
region `[32768, 65536)` — same bug class as the 128KB test that R2 flagged.

**Remediation**: rewrite §Cross-references entry for R2-T19:
> "R2-T19 → **closed in session 01 R2 remediation** (commit `681a3a2`,
> `model.rs:770` test). No session-02 action; the existing test
> correctly discriminates pre-fix vs post-fix in the disjoint-window
> overlap region. Remove the R2-T19 row from §Test plan; it's
> done-work, not session-02-scope."

### PR1-T31 — Conventional-commit scope convention not pinned; risks scope drift across commits

**Agents**: rev HIGH (1-way)

CLAUDE.md's conventional-commit enumeration is type-only (`feat`,
`fix`, `docs`, `chore`, …). Session-01 commits used `feat(session-01):`
/ `fix(session-01):` consistently. Plan should pre-commit to the scope
convention to avoid mid-session drift (e.g. half the commits using
`feat(session-02):` and half `feat(photohelper-raw):`).

**Remediation**: add to §Cross-references or §Plan revisions log:
"Scope convention: this session uses `<type>(session-02): ...` to match
session-01's pattern; `git log --first-parent main` then shows one
merge per session with consistent scoping. Per-component scopes (e.g.
`feat(photohelper-raw):`) are a project-level convention change
requiring its own ADR." Pre-commits the rule before implementation.

### PR1-T32 — R2-M8 silent `let _ = conn.execute("ROLLBACK", [])` path is not addressed by the poison_for_testing closure

**Agents**: sfh HIGH (1-way)

The existing poisoned-mutex recovery at `catalog.rs:297` is `let _ =
conn.execute("ROLLBACK", [])` — R2-M8 flagged this as swallowing every
SQLite error from the recovery rollback. The plan's
`poison_for_testing` test triggers the poisoned state but does not
address whether the rollback succeeded. The test ships as "green"
while the silent-rollback bug-class is wide open; R2-M8 stays MEDIUM
with no new mitigation.

**Remediation**: extend Deliverable 6 to also close R2-M8: replace `let
_` with `match conn.execute("ROLLBACK", [])` distinguishing
`SqliteFailure(_, Some("cannot rollback - no transaction is active"))`
(expected) from everything else (log via `tracing::warn!` with error
chain). Add Test-plan row "poison + ROLLBACK fails because of disk-full
→ WARN fires."

---

## MEDIUM

| ID | Theme | Agents | Citation | Remediation summary |
|----|-------|--------|----------|---------------------|
| PR1-M1 | DN-007 closure cross-ref path half-stated | com-1 | `:270` | Append "(DN-007's Owner is TD-002 per discovery-notes.md:86; TD-002 closure IS DN-007 closure. Update DN-007 Status accordingly.)" |
| PR1-M2 | §Deliverable 5 "§ Status" section doesn't exist in decision doc 0001 | gp | `:113-115` | Reword to specify which section to amend: Owners line (per PR1-T8), new § History sub-section, or both |
| PR1-M3 | Test plan claims `tracing-test` OR stderr-capture without picking | gp | `:172` | Pick `tracing-test` (already a stable crate); add as workspace dev-dep |
| PR1-M4 | `From<RawExif> for ExifMetadata` conversion not committed | type | `:91-99` | Commit `impl From<RawExif> for ExifMetadata` in `photohelper-cli::commands::ingest`; unit test pinning field-by-field mapping |
| PR1-M5 | `white_balance_multipliers` / `color_matrix` arrays unspecified shape; no invariants | type | `:25, :64-66` | Commit `[f32; 4]` / `[[f32; 3]; 3]` (or NewType wrappers); constructor rejects all-zero (LibRaw "unloaded" signal) |
| PR1-M6 | rusqlite bump verification regression-only — no NEW test pins the bumped contract | test | `:170` | Add `assert!(rusqlite::version_number() >= 3_045_000)` or exercise 0.40-specific API surface |
| PR1-M7 | FFI-safety wrong-format inputs other than PNG | test | `:166` | Add CR2 fixture asserting `read_cr3` returns `RawOpenFailed` (or `WrongRawFormat`) — pins dispatch contract |
| PR1-M8 | `git-lfs` ADR + decision absent | rev | `:83-88` | File `docs/adr/0002-git-lfs-for-raw-fixtures.md` covering quota budget + retention policy + offline-CI fallback (rename LGPL doc to `0003-` to free `0002-` for this ADR if needed) |
| PR1-M9 | DI-3 pre-emptively conditions DN-013 filing on "if it materially changes scope" | rev | `:214-219` | File DN-013 unconditionally as part of plan-v2; drop conditional |
| PR1-M10 | Plan-revisions log v1 entry pre-discloses what plan-review will say | com-2 | `:289-292` | Shrink v1 entry to one-line; move log to front-matter; delete `*(future revisions ...)*` placeholder |
| PR1-M11 | "Checkpoints fired" past-tense for future events | com-1 | `:176` | "Checkpoints firing this session (Cadence A)" matches session-01 convention |
| PR1-M12 | TD-003 binding trigger only 1 of 3 clauses confirmed unfired | com-1 | `:280-281` | Spell out: trigger (a) not fired, (b) date headroom, (c) test-flake not observed — all three confirmed unfired |
| PR1-M13 | SCUNet model-name leak (ungoverned) in §Out-of-scope row | com-1 | `:149` | Change to "AI denoise (`develop` subcommand; model TBD pending session 04 plan-review)" — OR file an ADR choosing SCUNet |
| PR1-M14 | NULL catalog column semantics shift (pre-02 = "didn't try"; post-02 = "tried + failed") | arch | `:108-115` | Add migration-intent paragraph: catalogs created by v0.1 with NULL CR3 columns NOT backfilled by session-02 binary; ingested_at_unix_seconds discriminates eras |
| PR1-M15 | Schema details re-stated when 0001 owns them | simp | `:108-115` | Replace with pointer to decision-doc 0001 + a one-sentence "columns populated, schema unchanged" |
| PR1-M16 | Checkpoints table mixes session-specifics with always-on protocol | simp | `:178-184` | Note above table: "Always-on per Cadence A: plan-review + session-end. Session-specific sub-component reviews below:" |
| PR1-M17 | `apply_outcome` exhaustive match has no IngestOutcome variant for partial EXIF | sfh | `:117-122` | Add `IngestOutcome::InsertedWithPartialExif { missing: Vec<&'static str> }`; thread through `apply_outcome` |
| PR1-M18 | DN-001 LGPL §6(a) decision deferred to plan-review without legal-counsel review | sfh | `:77-81, :245` | Defer decision-doc-0002 final-shape signing-off to release-engineering session with explicit legal-review trigger; this session ships DRAFT only |

---

## LOW

| ID | Theme | Agents | Citation | Note |
|----|-------|--------|----------|------|
| PR1-L1 | R2-T22/T23 cross-ref dismissal contradicts session-01 R2 disposition | gp + com-1 | `:284-285` | Verify whether R2-T22/T23 closed in commit `681a3a2` or shipped as deferred; correct framing |
| PR1-L2 | Risk register row about rusqlite 0.40 fallback inconsistent with TD-002 "API-compatible" claim | gp | `:259` | Reword to "introduces unforeseen API surface" instead of "API-breaks" |
| PR1-L3 | Plan-revisions log placeholder bullet | simp + com-2 | `:289-292` | Delete `*(future revisions per plan-review rounds)*` line |
| PR1-L4 | "Detailed implementation (populated AFTER ...)" parenthetical meta-noise | simp | `:294-299` | Delete entire section OR move to closing italic note (matches session-01) — covered by PR1-T16 |
| PR1-L5 | ADR vs decisions classification for LGPL doc | com-1 | `:78` | Promote to `docs/adr/0002-libraw-lgpl-static-link.md` (binding for every release) |
| PR1-L6 | §Acceptance criteria item 6 cross-ref to wrong § | com-1 | `:247` | Cite `§ Findings triage` (where CRITICAL is defined as blocking) instead of `§ Double-review protocol` |
| PR1-L7 | RawExif / RawImage `Clone` / `Send` / `Sync` derives unspecified | type | `:60-66` | Commit derives explicitly; add `static_assertions::assert_impl_all!(RawExif: Send, Sync)` at module scope |
| PR1-L8 | Test plan range assertions weaker than type-system equivalents | type | `:167-168` | After PR1-T5 type-system fixes land, prune redundant runtime assertions (subsumed by `NonZeroU32` / `CfaPattern` enum) |
| PR1-L9 | Test-plan deliverables column conflates unit + integration for LibRaw build-system | test | `:166-174` | Add row "LibRaw build-system" with CI-gated invariants (build clean on linux+macos; binary statically links LibRaw via `nm`/`otool -L`) |
| PR1-L10 | Bash code fence missing in §Acceptance criteria #2 | com-2 | `:235` | Wrap command in `bash` fence + break out expected summary line / SQL row check into separate fences |
| PR1-L11 | "git-lfs" vs "Git LFS" capitalization inconsistent | com-2 | `:83, :84, :174, :260` | Standardize on "Git LFS" (official capitalization); use `git-lfs` only when referencing the CLI binary |
| PR1-L12 | §Cross-references verb taxonomy inconsistent (6 distinct verb-forms) | com-2 | `:263-285` | Define taxonomy at top of section OR collapse to 2 verbs (`closed` / `partial`) |
| PR1-L13 | §Cross-references duplicates §Out-of-scope tracking column | simp | `:144-160, :263-285` | Drop Tracking column from OOS table; Cross-references is authoritative |
| PR1-L14 | §Risk register Likelihood column right-aligned with string values | com-2 | `:254` | Left-align (right-align is for numeric data) |
| PR1-L15 | §Scope rationale h3 style inconsistent (colon vs parenthetical) | com-2 | `:38` | Rename to "Scope rationale (why bundle EXIF + decode + rusqlite bump)" to match other h3s |
| PR1-L16 | Plan title "libraw-cr3-decode" understates scope after EXIF was elevated | rev | `:1-3` | Add one-line clarifier under title: "Despite the slug, this session lands LibRaw EXIF read AND RAW pixel decode — see Scope rationale" |
| PR1-L17 | Risk register row 6 "scope creep" is meta-noise | rev + simp | `:261` | Delete row; the protocol IS the mitigation. (Severity bumped to MEDIUM/HIGH by simp; treating as LOW because cosmetic) |

---

## Strengths preserved [NOTES]

Confirmed by R1 — must not regress in R2:

- **Goal section explicitly cross-references DN-011 and the R2-T13 production trace** (`:18-30`). The framing "DN-011 critical-path remediation" + "the only path to a usable `--strict` mode" correctly identifies the user-facing pain. Keep this framing if scope splits per PR1-T28/T34.
- **§Scope rationale section exists** (`:38-48`) — explicit narrative reasoning for bundling is uncommon and valuable. Replaces "why this scope" reviewer questions with explicit text. Keep modulo the TD-002 trigger correction (PR1-T24).
- **Acceptance criteria are mostly concrete + falsifiable** (`:230-250`). Criteria 1, 3, 4, 5 are script-checkable. Only criterion 2 (PR1-T6) and criterion 6 (PR1-T23) need rework.
- **`photohelper-raw::ffi` named as the only `unsafe` site** (`:53-56, :240-242`) — explicit + matches CLAUDE.md § Rust-specific gates. Needs the actual override per PR1-T21.
- **DN-006 / DN-011 named as "closed by construction"** (`:266-275`) — tying "fixed by LibRaw landing" to the dispatch rewire is precise. Tighten per PR1-T41 (verification conditions).
- **§Out-of-scope table has explicit Owner + Tracking columns** — disciplined per CLAUDE.md § No-Acceptable-Trade-offs Policy. Needs cleanup for DI-N references (PR1-T7) and OOS row tracking IDs.
- **§Risk register lists 6 concrete risks with concrete mitigations** — not "monitor"/"be careful." Drop only the meta-noise row 6 (PR1-L17).
- **§Test plan distinguishes unit-vs-integration boundaries** per `quality-assurance.md § Plan-review protocol` mandate. Most rows are real tests (modulo the duplicates per BF and the orphaned `git-lfs` row per AG).
- **Error enum convention explicit** (`:68-69`) — thiserror, no `#[from]` across public boundaries, `#[non_exhaustive]`. Matches `crates/photohelper-core/src/error.rs:20-21` byte-for-byte.
- **Plan revisions discipline pre-committed** (`:8`) — sets up the v2 → v3 chain so future reviewers know the format.

---

## New bug classes surfaced [NOTES]

R1 surfaced eight structural patterns worth recording for future plan-reviews and session-end reviews:

1. **"Discovery items as research-deferral rather than decision-evaluation"** (PR1-T7). DI-1/DI-2 frame *empirical research* (crates.io maintenance, CVE history, cross-compile feasibility) as "plan-review must pick" — but plan-review's role is to *evaluate* a recommendation, not *produce* one. Future plan-reviews should reject any Discovery item that lacks "Facts gathered" subsections naming concrete data sources.

2. **"Identifier-prefix invention without convention amendment"** (PR1-T7). The plan minted a new `DI-N` prefix without amending CLAUDE.md § Where things live. Future plan-reviews should reject any new ID prefix that isn't pre-declared in the conventions table.

3. **"Cross-doc decision contradictions where plans silently override decision docs"** (PR1-T8). Plan defers v1→v2 migration framework to session 03 even though decision doc 0001 commits session 02. Future plan-reviews should run a cross-doc consistency check on every DN/decision-doc the plan claims to "partially advance" or "defer" — the source doc and the plan must agree, or one of them must be amended in the same commit.

4. **"Closure-by-construction overclaim"** (PR1-T41 / PR1-T30 / PR1-T4). Multiple findings on the form "the plan claims R2-TN closed but the binding trigger has conditions the plan doesn't honor." Future plan-reviews should require a per-closure verification matrix (binding trigger condition → plan-side fulfillment), not a free-text claim.

5. **"`if added` / `where naturally touched` / `may surface` qualifier escape hatches"** (PR1-T4, PR1-T7, PR1-T25). All three phrases convert deliverables into soft commitments. Future plan-reviews should grep for these (plus "TBD", "to be decided") in the plan and flag each as either a real deferral (file TD/DN with binding trigger) or a real commitment (drop the qualifier).

6. **"Acceptance criteria that depend on author's personal machine state"** (PR1-T6). Criterion 2 names `/Users/ph/Pictures/tests` — not CI-runnable. Future plan-reviews should require every acceptance criterion to specify either CI-verifiable OR explicitly-labeled manual-smoke-only.

7. **"Type-design v1 freezes the decoder boundary forever"** (PR1-T5). `RawExif` / `RawImage` set the precedent for every future RAW format crate adds; under-encapsulated v1 is impossible to refactor later without breaking downstream sessions. Future plan-reviews should require strong-type discipline (private fields + fallible constructors + accessor methods) for any v1 type that downstream sessions will consume.

8. **"`cargo audit` blindness to C-library dependencies"** (PR1-T10). LibRaw CVEs are invisible to `cargo audit`. Future plan-reviews touching C-library FFI should require an explicit CVE-monitoring mechanism filed as TD (manual subscription to upstream advisories) AND a clarifying note in any acceptance criterion citing `cargo audit` clean as proof.

---

## Disposition summary

| Disposition | Count | Notes |
|-------------|------:|-------|
| **Fix inline in R1 remediation (plan v2)** | 16 CRITICAL + 17 HIGH + 14 MEDIUM | All scope-bearing + cross-ref accuracy + type-design fixes |
| **Verify-and-amend cross-doc (decision doc 0001 + DN-001 + DN-013 filings)** | 3 (PR1-T8, PR1-T18, PR1-T7 sub-d) | Touches docs outside the plan |
| **File new TD entries with binding triggers** | 2 (TD-004 heartbeat-death OR LibRaw CVE monitoring) | Per No-Acceptable-Trade-offs Policy |
| **Defer with explicit DN cross-ref (filed in v2)** | 1 (DN-013 Windows cross-compile) | Currently conditional |
| **Accept-as-is with explicit comment** | 9 LOW | Cosmetic; ship after substantive remediation |

If R2 surfaces CRITICAL-class regressions introduced by R1 remediation → fire R3.

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 20
  verified: 18
  drifted: 2
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    20 high-impact CRITICAL+HIGH findings verified by the 9th-agent
    verifier; 18 cleanly present, 2 drifted (PR1-T4-B and PR1-T27-A —
    both retained with corrected line). MEDIUM/LOW findings
    (~23 items) NOT individually 9th-agent-verified for cost reasons;
    they carry direct file:line citations from the original agent
    reports and can be spot-checked during R1 remediation. Plan-review
    Round 1's high CRITICAL count (16) reflects that v1 is the
    pre-plan-review draft — many gaps surface in R1 precisely
    because that's R1's job; v2 should collapse most of these. The 9th
    agent also independently flagged PR1-T21 (Cargo.toml override
    intent vs reality), confirming the photohelper-raw lint-override
    gap is a real bug not a misread.
  details:
    - {finding_id: PR1-T1-A, file: crates/photohelper-cli/src/commands/ingest.rs, line: 27, present: yes, retain: yes, reason: "RAW_EXTS 8-extension list present verbatim.", evidence_snippet: 'const RAW_EXTS: &[&str] = &["cr3", "cr2", "arw", "nef", "raf", "orf", "rw2", "dng"];'}
    - {finding_id: PR1-T3-A, file: docs/discovery-notes.md, line: 93, present: yes, retain: yes, reason: "DN-008 binding trigger lists exactly the 12 rows including 17.", evidence_snippet: 'tests `{6, 12, 13, 14, 17, 18, 19, 34, 39, 42, 43, 49}` (12 rows; row 48 is closed by the R2-T6 deterministic heartbeat test; row 17 hardlink was missing from the prior list and is restored).'}
    - {finding_id: PR1-T3-B, file: docs/plans/session-02.md, line: 122, present: yes, retain: yes, reason: 'Plan explicitly lists "32-equiv" which is not in DN-008 rows.', evidence_snippet: 'this session lands tests for rows **{6, 32-equiv (real CR3), 39, 42, 43, 49}**'}
    - {finding_id: PR1-T4-A, file: docs/plans/session-02.md, line: 172, present: yes, retain: yes, reason: '"if added" confirms deferral without binding trigger.', evidence_snippet: '(heartbeat death tested via the deferred `panic_for_testing` knob — if added).'}
    - {finding_id: PR1-T4-B, file: docs/code-reviews/session-01-round2.md, line: 268, present: drifted, retain: yes-with-corrected-line, reason: "R2-T18 four-WARN enumeration starts at line 269+; finding accurate but cited at heading line.", evidence_snippet: 'R1.T10 added four new `tracing::warn!` arms that are runtime-observable but completely untested:'}
    - {finding_id: PR1-T6-A, file: docs/discovery-notes.md, line: 68, present: yes, retain: yes, reason: "DN-011 production trace summary present verbatim.", evidence_snippet: 'Summary: `walked: 371, no-exif: 370, ingested: 0, already-catalogued: 370, skipped (non-RAW): 1`.'}
    - {finding_id: PR1-T6-B, file: docs/plans/session-02.md, line: 235, present: yes, retain: yes, reason: "Plan asserts ingested=371/already-catalogued=0 incompatible with DN-011 trace.", evidence_snippet: '`walked: 371, no-exif: 0, ingested: 371, already-catalogued: 0, skipped (non-RAW): 1`.'}
    - {finding_id: PR1-T8-A, file: docs/decisions/0001-catalog-schema-v1.md, line: 4, present: yes, retain: yes, reason: "Owners line names session 02 for v1->v2 migration framework.", evidence_snippet: '**Owners**: this session (v1 minimal schema); session 02 (v1 → v2 migration when cull-score + dup-group tables land per DN-005).'}
    - {finding_id: PR1-T8-B, file: docs/decisions/0001-catalog-schema-v1.md, line: 122, present: yes, retain: yes, reason: "Migration policy asserts session 02 introduces migration FRAMEWORK.", evidence_snippet: 'v1 stays at `PRAGMA user_version = 1` forever. The next change (v1 → v2 in session 02) introduces the migration FRAMEWORK'}
    - {finding_id: PR1-T10-A, file: docs/plans/session-02.md, line: 57, present: yes, retain: yes, reason: 'Plan commits "LibRaw 0.21+" unbounded floating tag.', evidence_snippet: 'hand-rolled FFI shim against LibRaw 0.21+ headers, whichever survives'}
    - {finding_id: PR1-T12-A, file: TECH-DEBT.md, line: 50, present: yes, retain: yes, reason: "TD-002 asserts API-compat for the operations listed.", evidence_snippet: 'rusqlite 0.40 is API-compatible for `Connection::open` / `execute` / `query_row` / `Transaction` / `params!` — the operations photohelper uses'}
    - {finding_id: PR1-T15-A, file: docs/plans/session-02.md, line: 120, present: yes, retain: yes, reason: "poison_for_testing introduced without #[cfg(test)] visibility constraint.", evidence_snippet: '`Catalog::poison_for_testing` knob (closes DN-008 row "poison test knob"). Used by the new test for the `BEGIN IMMEDIATE` + `ROLLBACK` poison-recovery path.'}
    - {finding_id: PR1-T15-B, file: docs/code-reviews/session-01-round2.md, line: 235, present: yes, retain: yes, reason: "R2-T15 flagged dead pub fn with #[doc(hidden)] anti-pattern as claimed.", evidence_snippet: '`crates/photohelper-catalog/src/catalog.rs:82-87` exposes `pub fn open_with_retry_delay(...)` behind `#[doc(hidden)]`. The only caller is the production `Catalog::open` at line 77 with the production `LOCK_RETRY_DELAY` constant. **Zero tests use the helper.**'}
    - {finding_id: PR1-T17-A, file: docs/plans/session-02.md, line: 78, present: yes, retain: yes, reason: "Plan cites §6(b) for vendored-tarball path; LGPL §6(b) is shared-library mechanism, vendored-source is §6(a).", evidence_snippet: 'DN-001 by recording the §6(b) artifact shape (e.g. a per-release `vendor/libraw-X.Y.Z.tar.gz` shipping alongside the binary in GitHub Releases).'}
    - {finding_id: PR1-T21-A, file: crates/photohelper-raw/Cargo.toml, line: 15, present: yes, retain: yes, reason: "Cargo.toml [lints] has workspace=true with NO per-crate unsafe_code override despite comment claiming the intent.", evidence_snippet: "[lints]\n# Override workspace-level `unsafe_code = \"forbid\"` — libraw FFI requires\n# `unsafe` blocks (scoped to one module). The lint stays `deny` at file scope\n# via `#![deny(unsafe_op_in_unsafe_fn)]` once we wire libraw.\nworkspace = true"}
    - {finding_id: PR1-T22-A, file: SESSION-STATE.md, line: 14, present: yes, retain: yes, reason: "SESSION-STATE still says session 1 R2 remediation applied despite PR #1 merged at c120819.", evidence_snippet: '**Current session**: 1 (R2 REMEDIATION APPLIED — ready for `just ci` + PR push).'}
    - {finding_id: PR1-T26-A, file: docs/plans/session-02.md, line: 94, present: yes, retain: yes, reason: "Deliverable 4 commits kamadak-exif keep; DI-4 at 221 considers dropping — internal contradiction.", evidence_snippet: 'other extensions (JPEG fallback for future sidecar work) → `kamadak-exif` as today'}
    - {finding_id: PR1-T27-A, file: docs/code-reviews/session-01-round2.md, line: 358, present: drifted, retain: yes-with-corrected-line, reason: "R2-T26 heading at 358 documents unused-dep finding; recommendation to add unused_crate_dependencies = warn likely in remediation body — flag for human triage on specific lint-add claim.", evidence_snippet: '### R2-T26 — `photohelper-core` declares unused `kamadak-exif` + `tracing` deps; breaks "core → ⊥" strength claim'}
    - {finding_id: PR1-T30-A, file: crates/photohelper-core/src/model.rs, line: 770, present: yes, retain: yes, reason: "R2-T19 96KB discriminating test ALREADY landed at session-01 R2 remediation; plan claim to close R2-T19 is redundant.", evidence_snippet: "    #[test]\n    fn photoid_derive_window_disjoint_distinguishes_overlap_region_changes() {\n        // R2-T19 rewrite: the previous `..._exactly_128k` test used an\n        // all-0xAA 128KB file"}
    - {finding_id: PR1-T11-A, file: docs/plans/session-02.md, line: 84, present: yes, retain: yes, reason: "Deliverable 3 commits CC0 license audit but no EXIF PII sanitization for GPS/owner/serial.", evidence_snippet: '**License audit recorded**: every fixture is CC0 or equivalent unencumbered; sources cited in `tests/fixtures/cr3/README.md`.'}
```

---

## R2 watch-list

R2 must verify R1 remediation against these themes:

1. **PR1-T1 dispatch axis** — was `parse_exif_for` collapsed (path a) or generalized to all RAW_EXTS via LibRaw (path b)? Either is acceptable; the silent fall-through to kamadak-exif for non-CR3 RAW MUST be eliminated.
2. **PR1-T2 Error variants** — collapsed to one variant with typed-enum cause? `cause` field type pinned? Dispatch site routing specified?
3. **PR1-T3 DN-008 row enumeration** — covered/deferred lists sum to 12 DN-008 rows? Row 17 hardlink moved to covered? "32-equiv" gone?
4. **PR1-T4 R2-T18 closure** — 4/4 (panic_for_testing landed) or 3/4 (TD-004 filed with binding trigger)? `file-lock` vs `lock-file-create` op-tags distinguished?
5. **PR1-T5 type design** — `RawExif` / `RawImage` have private fields + fallible constructors + strong types per field? `ExifOrientation` enum used? `NonZeroU32` for dimensions? `CfaPattern` enum?
6. **PR1-T6 acceptance criterion #2** — split into 2a (CI-verifiable) + 2b (manual smoke)? "371/371" corrected to "370/370"?
7. **PR1-T7 DI-N** — prefix dropped/replaced? DI-1/DI-2 spikes done with cited data? DI-3 moved to risk register? DI-4 decided inline?
8. **PR1-T8 decision-doc 0001 vs plan** — amendment landed in plan-v2 commit; Owners line + Migration policy + Trigger lines all reconciled?
9. **PR1-T9 pre-flight** — Deliverable 0 added; sequencing explicit; artifact path named?
10. **PR1-T10 LibRaw pinning** — exact `=X.Y.Z`; SHA-256 verified at build-time; TD-004 (or similar) filed for C-CVE monitoring; Acceptance criterion 4 clarified?
11. **PR1-T11 fixture sanitization** — `exiftool` step + CI lint added to Deliverable 3?
12. **PR1-T12 rusqlite** — enumerated API surface changes between 0.32-0.40 added to Deliverable 5? PRAGMA `journal_mode = WAL` round-trip test added?
13. **PR1-T13 git-lfs** — `fixture_is_real_cr3` helper committed; `actions/checkout@... with: lfs: true` used; silent-skip explicitly rejected?
14. **PR1-T14 partial-EXIF** — `ExifCompleteness` enum committed; `partial_exif` counter added; `--strict` rejects partial?
15. **PR1-T15 poison_for_testing** — `#[cfg(test)]`-only visibility constraint added; "no `*_for_testing` in production symbol table" acceptance criterion added?
16. **PR1-T16 detailed implementation h2** — deleted OR converted to closing italic note?
17. **PR1-T17 LGPL §6(a)** — every §6(b) reference replaced; DN-001 amended in same commit; LGPL clause quoted verbatim in decision doc 0002?
18. **PR1-T21 photohelper-raw Cargo.toml** — `unsafe_code = "allow"` override landed at crate level + `#![deny(unsafe_op_in_unsafe_fn)]` at `ffi.rs` head + workspace `undocumented_unsafe_blocks` lint?
19. **PR1-T22 SESSION-STATE.md** — drift cleanup landed BEFORE R2 plan-review fires?
20. **PR1-T28/T34 scope split** — if scope split into EXIF-only-now + decode-later, did the EXIF-only path retain DN-011 closure narrative?

For MEDIUM/LOW: spot-check the items most likely to regress in remediation
(esp. PR1-M14 — NULL semantics shift — and PR1-T25 — DN-012 enumeration).
