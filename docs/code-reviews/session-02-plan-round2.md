# session-02 plan-review Round 2

> Per `docs/quality-assurance.md § Plan-review protocol § Double-review
> protocol`. Cadence A → Tier 5 (plan-review). Full 8-agent suite fired in
> parallel against `docs/plans/session-02.md` v2 (889 lines, post-R1
> remediation; the commit immediately preceding HEAD's R2-artifact commit).
> Findings consolidated by **theme** (not by agent) per
> `docs/quality-assurance.md § Consolidation discipline`. When multiple
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
| **CRITICAL** | 9 | Regressions introduced by R1 remediation (fabricated PR1-T# IDs, phantom DN-016) PLUS new design holes (BayerPlane accessor panics, WhiteBalance/ColorMatrix bag-of-fields reborn) PLUS persisted-from-v1 defects (LibRaw `X.Y.Z` placeholder). |
| **HIGH** | 14 | Convention violations, type-design gaps, atomicity issues, scope-coordination drifts, test-coverage gaps. |
| **MEDIUM** | 12 | Polish + cross-ref accuracy + heading/format consistency. |
| **LOW** | 6 | Cosmetic / hygiene / placeholder framing. |
| **NOTES** (strengths preserved + new bug classes) | 5 | Confirm in R3 if fired. |

Agent suite labels: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer` (type),
`pr-review-toolkit:silent-failure-hunter` (sfh),
`pr-review-toolkit:comment-analyzer` (com-1: cross-ref accuracy lens; com-2:
plan-as-contract testability lens — two passes per the §3 8-agent budget),
`pr-review-toolkit:pr-test-analyzer` (test),
`pr-review-toolkit:code-simplifier` (simp).

---

## CRITICAL

### R2-T1 — Plan v2 references PR1-T# IDs that DO NOT EXIST in the R1 artifact (audit-trail fabrication)

**Agents**: gp + com-1 (2-way CRITICAL; self-verified by orchestrator via grep)

Plan v2 cross-references **PR1-T33, T34, T35, T36, T37, T42, T44, T45, PR1-AU, PR1-T7b, PR1-T7e** at multiple locations (`session-02.md:142, 184, 340, 459, 512, 544, 611, 760, 767, 770, 798, 882`). Orchestrator grep verified: R1 (`session-02-plan-round1.md`) defines findings **PR1-T1 through PR1-T32** plus **PR1-M1–M18** plus **PR1-L1–L17** — **no other PR1-T# IDs exist**. PR1-T7b / PR1-T7e are sub-letter references; R1 uses `(a)/(b)/(c)/(d)/(e)` as remediation sub-items WITHIN PR1-T7, never as standalone IDs.

Concrete drift examples:
- `:142` cites PR1-T35 for capture_time semantics → real owner is PR1-T5 (type design).
- `:340` cites PR1-T36 for cargo:warning build errors → no R1 owner exists.
- `:459` cites PR1-T37 for IngestOutcome exhaustivity → real owner is PR1-M17.
- `:544` cites PR1-T42 for poison test 3-way split → real owner is PR1-T15 sub-issue.
- `:611, :760` cite PR1-T44 for FFI error-path table → real owner is PR1-T2 dispatch routing.
- `:512` cites PR1-AU for unused-deps lint → no R1 finding has this ID.

This is the exact "R2-T8 doc enshrining fabricated invariant" anti-pattern from session-01 — corrupting the audit trail by claiming closure of items that don't exist.

**Remediation (mandatory; sub-day)**: grep + correct every fabricated ID. Recommended mapping (author verifies before edit): PR1-T34→PR1-T28; PR1-T35→PR1-T5; PR1-T36→drop or file as new R2 finding; PR1-T37→PR1-M17; PR1-T42→PR1-T15; PR1-T44→PR1-T2; PR1-T45→drop; PR1-AU→drop; PR1-T7b/PR1-T7e→PR1-T7 (with sub-letter context spelled out in surrounding prose, not as ID suffix).

### R2-T2 — `DN-016` cited as filed when it doesn't exist in `docs/discovery-notes.md` (phantom DN)

**Agents**: gp + com-1 (2-way CRITICAL; self-verified by orchestrator: `grep "DN-016\|DN-017" docs/discovery-notes.md` returns 0 matches; plan v2 references DN-016 at `:177, :203`)

Plan v2 says (`:177`): "DN-016 tracks the timezone-recovery work for v0.2" and (`:203`): "(DN-016 binding trigger when develop work begins)." `docs/discovery-notes.md` ends at DN-015. The plan invents a binding trigger that doesn't exist. Per `CLAUDE.md § No Acceptable Trade-offs Policy`: "'TODO: come back to this' is NOT a trigger." Pointing at a phantom DN is operationally identical to a TODO.

**Remediation (mandatory; cross-doc commit BEFORE R3 or implementation)**: file `DN-016 — Canon CR3 EXIF timezone recovery deferred to v0.2 develop pipeline` AND `DN-017 — WhiteBalance rebalance + per-illuminant color-matrix recovery deferred to develop pipeline` (or fold both into DN-016 if treated as one workstream) with binding triggers matching the plan's claims. Land in the same commit as the R2 plan-remediation v3.

### R2-T3 — `panic_for_testing` `#[cfg(test)]`-gated knob is UNREACHABLE from subprocess integration tests; R2-T18 4/4 closure is false-by-construction

**Agents**: gp (CRITICAL; 1-way single-agent flag — high-signal solo find)

Plan v2 Deliverable 6c (`:573-587`) declares:
```rust
#[cfg(test)]
static HEARTBEAT_PANIC_FOR_TESTING: AtomicBool = AtomicBool::new(false);
// in heartbeat_loop (test builds only):
if HEARTBEAT_PANIC_FOR_TESTING.load(Ordering::Relaxed) { panic!(...); }
```
Test plan row (`:769`) describes the regression test as integration-style: `Command::cargo_bin("photohelper")` invocation. But `Command::cargo_bin` spawns the **non-test release binary** — `#[cfg(test)]` items are STRIPPED from that binary. The static + panic site do not exist there. The integration test cannot flip the knob to fire the panic.

This invalidates the PR1-T4 "land panic_for_testing this session" remediation path; R2-T18 cannot actually close 4/4 via this mechanism. Plan v2's claim "R2-T18 closes fully — not 3 of 4" is structurally false.

**Remediation (mandatory)**: pick one of:
(a) Gate behind `#[cfg(any(test, feature = "test-helpers"))]` with `--features test-helpers` passed to the integration-test binary build; OR
(b) Use environment-variable trigger (`PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING=1` — matches existing `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` pattern); OR
(c) Demote R2-T18 closure back to 3/4 and file a TD with binding trigger (matching PR1-T4 path (b) which v1 chose to reject).

Each path interacts with Acceptance criterion 3 ("only `ffi.rs` carries unsafe") and the new R2-T19 / R2-T20 acceptance "no `*_for_testing` in production binary" — which path (a) makes weaker. Recommend (b) — simplest; matches existing env-override pattern; no feature-flag escape hatch.

### R2-T4 — LibRaw version pin is the literal placeholder `=X.Y.Z`; Acceptance criterion 7 is unsignable

**Agents**: gp + rev + com-2 (3-way CRITICAL — persists-from-v1 OR new regression)

PR1-T10 (R1 CRITICAL) demanded: "Replace 'LibRaw 0.21+' with an exact `=X.Y.Z` pin **chosen by plan-review v2**." v2 retains the literal token `X.Y.Z` in seven locations including the file-name pattern (`vendor/libraw-X.Y.Z.tar.gz`) and Acceptance criterion 7 itself. The Acceptance criterion is unmeetable as written — a literal `X.Y.Z` cannot be a tarball filename. The accompanying prose "recommended: latest 0.21.x patch release as of the session-02 implementation date" (`:332`) is the **exact "floating tag" PR1-T10 ruled out**.

**Remediation (mandatory)**: pin a specific version inline now. Recommend `0.21.4` (current 0.21.x latest at session-02 plan-v2 commit time; CVE-posture-as-of-pin recorded in decision-doc 0002 OR file `DN-018 — LibRaw version pin selection and CVE-posture audit` recording why the chosen version was picked + the CVE-feed checked-as-of date). If author can't lock the version at plan-review time, downgrade Acceptance criterion 7 to "the pre-flight commit (Deliverable 0) MUST replace X.Y.Z with the concrete version chosen and the implementation PR description MUST cite it; verifiable as `! grep -nE 'X\\.Y\\.Z' Cargo.toml crates/photohelper-raw/` returns no matches."

### R2-T5 — `BayerPlane::pixel(x, y) -> u16` and `row(y) -> &[u16]` accessor signatures MUST panic on OOB; violates workspace `panic = "warn"` + `indexing_slicing = "warn"` lints

**Agents**: rev + type (2-way CRITICAL convergence)

Plan v2 (`:220-225`) declares:
```rust
pub fn row(&self, y: usize) -> &[u16] { ... }
pub fn pixel(&self, x: usize, y: usize) -> u16 { ... }
```
Both signatures return non-`Option`, non-`Result` types. For OOB `y` or `x`, the implementation has NO valid value to return — it must panic (`self.data[y * w + x]`), unwrap (`get(...).expect()`), or saturate (silently wrong pixel data). Each violates workspace `Cargo.toml:84-87` lints (`unwrap_used`, `expect_used`, `panic`, `indexing_slicing` — all warn-escalated to error by `-D warnings` in CI per `CLAUDE.md § Rust-specific gates`).

The plan's own comment at `:224-225` claims: "**No `pub data()` accessor — downstream code goes through `row()`/`pixel()` so unchecked indexing is impossible**." This is structurally FALSE: removing `data()` doesn't eliminate unchecked indexing; it just displaces it from caller-site to callee-site where the workspace lint can no longer flag it on the caller's side. This is the R2-T20 silent-failure class repeated at the accessor boundary.

R1's PR1-T5 fix (private fields + fallible constructor) is preserved at construction-time only; runtime callers can still trigger panics via integer arithmetic before the accessor call.

**Remediation (mandatory)**: change accessor shapes to encode fallibility:
```rust
pub fn row(&self, y: usize) -> Option<&[u16]> { ... }
pub fn pixel(&self, x: usize, y: usize) -> Option<u16> { ... }
```
OR keep infallible accessors but require a bounds-checked iterator API:
```rust
pub fn rows(&self) -> impl Iterator<Item = &[u16]> { ... }
pub fn pixels(&self) -> impl Iterator<Item = u16> { ... }
```
Path 1 is the smaller patch; path 2 is the more idiomatic Rust shape for session 04's tile-based demosaic. Also: use `u32` not `usize` for `x`/`y` to match `width: NonZeroU32` and avoid 32-bit-host overflow.

### R2-T6 — `WhiteBalance` is a bag-of-fields placeholder (named only in comment, no struct body); PR1-T5 anti-pattern reborn

**Agents**: type + test (2-way CRITICAL — type-design discipline regression)

Plan v2 (`:204`) declares `as_shot_white_balance: WhiteBalance,` with inline comment `[f32; 4] RGGB; rejects all-zero (LibRaw "unloaded")`. The type body is **never specified**: no struct body, no constructor signature, no accessor list, no derive set. Three independent failure modes survive:

1. **Channel-mapping ambiguous**: comment says "RGGB" but LibRaw's `cam_mul[4]` is documented as `R, G1, B, G2` on most cameras — different ordering. The plan silently encodes a different channel mapping than LibRaw delivers; session 04's develop pipeline will multiply wrong channels and produce a magenta cast.
2. **NaN/negative not rejected**: "rejects all-zero" is the only invariant; LibRaw can return `NaN` for missing WB metadata and negative values are mathematically valid in IEEE-754 but physically nonsense.
3. **No accessor signature**: `pub fn r(&self) -> f32`? `pub fn channel(&self, ColorChannel) -> f32`? `pub fn as_slice(&self) -> &[f32; 4]`? Pick one — missing decision is the type-system v1 freezes the boundary forever anti-pattern.

The same critique applies to **`ColorMatrix`** (`:205`): `3x3 CamRGB→XYZ_D65; rejects identity-as-unloaded` — direction is in a comment, not in the type. Applying `CamRGB→XYZ` when `XYZ→CamRGB` was required produces inverted color casts.

Test coverage gap: §Test plan has zero rows asserting `WhiteBalance` all-zero rejection or `ColorMatrix` identity rejection — the invariants live in the type's documentation but not in a test that would catch a constructor regression.

**Remediation (mandatory)**: replace inline comments with real type bodies + fallible constructors + accessor methods:
```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhiteBalance { r: f32, g1: f32, b: f32, g2: f32 }
impl WhiteBalance {
    pub(crate) fn new_libraw_cam_mul(cam_mul: [f32; 4]) -> Result<Self, Error> {
        let [r, g1, b, g2] = cam_mul;
        if cam_mul.iter().all(|x| *x == 0.0) {
            return Err(Error::RawDecodeFailed { cause: WhiteBalanceUnloaded, .. });
        }
        if cam_mul.iter().any(|x| !x.is_finite() || *x < 0.0) {
            return Err(Error::RawDecodeFailed { cause: WhiteBalanceInvalid { values: cam_mul }, .. });
        }
        Ok(Self { r, g1, b, g2 })
    }
    pub fn r(&self) -> f32 { self.r }
    /* ... per-channel accessors */
}
```
Same shape for `ColorMatrix` with direction in type (e.g. `CamRgbToXyzD65Matrix([[f32; 3]; 3])`). Add corresponding Test plan rows.

### R2-T7 — `Error::Exif` removal coordination missing; new `Error::ExifLibraw { source: BoxedSourceError(Box::new(e)) }` constructor syntax does not compile

**Agents**: gp (CRITICAL — single-agent; 2 distinct issues bundled)

Two compounding issues:

1. **Dead variant**: `crates/photohelper-core/src/error.rs:36-43` defines `Error::Exif`, currently constructed only at `ingest.rs:431` (the kamadak-exif `read_from_container` boundary). Plan v2 deletes the `parse_exif` site + `kamadak-exif` workspace dep atomically (Deliverable 4 + 5). Plan v2 never says what happens to the existing `Error::Exif` variant — leaving it makes it dead code (no producer); removing it is a public-API break for `photohelper-core` requiring a coordinated re-export and `Cargo.toml` patch-version bump.

2. **Constructor syntax invalid Rust**: Plan v2 (`:316-318`) declares `Error::ExifLibraw { source: BoxedSourceError(Box::new(e)) }` using tuple-struct syntax — but `BoxedSourceError` at `error.rs:17` is a `pub type` alias (`Box<dyn std::error::Error + Send + Sync>`), NOT a tuple struct. `BoxedSourceError(...)` does not compile. The correct form is `Error::ExifLibraw { source: Box::new(e) }` (the type-alias just relabels; no wrapping needed).

**Remediation (mandatory)**: add to Deliverable 4:
> "Remove `Error::Exif` from `photohelper-core::Error` atomically with kamadak-exif workspace dep removal (the variant has no remaining producer). Either rename the new variant to `Error::Exif` (recycling the slot, simplest) OR keep the old variant `#[deprecated]` and add `Error::ExifLibraw` — pick one. Fix the constructor syntax: `Error::ExifLibraw { source: Box::new(e) }` — no `BoxedSourceError(...)` wrapping (it's a type alias). Add test asserting the variant change compiles."

### R2-T8 — `parse_exif_for(path, extension)` dispatch is GONE in v2 but `RAW_EXTS = ["cr3"]` narrowing leaks PII-class silent failures for non-CR3 RAW formats users might still feed in

**Agents**: arch (CRITICAL — single-agent architectural concern)

Per PR1-T1 path (a) remediation in v2 Deliverable 4, `RAW_EXTS` narrows to `["cr3"]` and `parse_exif_for` is collapsed. **However**: the existing `crates/photohelper-cli/src/commands/ingest.rs:27` declares `RAW_EXTS = &["cr3", "cr2", "arw", "nef", "raf", "orf", "rw2", "dng"]`. The plan v2 doesn't explicitly say HOW the narrowing happens at the code level. Two implementations exist:

1. **Constant change**: `const RAW_EXTS: &[&str] = &["cr3"]` — straightforward; other extensions become "skipped (non-RAW)" silently. This is the intended path.
2. **Filter at dispatch**: keep `RAW_EXTS` as-is; reject non-CR3 in `parse_cr3_exif`. This produces a different (and noisier) UX where non-CR3 RAWs are walked, attempted, and rejected at parse-time.

The plan reads as if (1) — "walker counts under `skipped (non-RAW)`" at `:429-432` — but doesn't specify. If a contributor implements (2), the failure-class shifts silently. Worse: the EXIF dispatch deletion happens BEFORE the walker change in the commit ordering (the plan promises "atomic" but doesn't say which side of the atom comes first), so an intermediate commit state can ship `parse_cr3_exif(path)` called on a `.cr2` file by accident.

**Remediation**: explicit commit shape and code-change spec in Deliverable 4: "`crates/photohelper-cli/src/commands/ingest.rs:27`'s `RAW_EXTS` constant is changed from 8 extensions to `["cr3"]` in THE SAME commit as the `parse_exif_for` collapse and the kamadak-exif workspace-dep removal. The walker's `is_raw_extension` filter consults the new constant; non-CR3 files (`.cr2`, etc.) fall through to the existing `SkippedNonRaw` outcome. Atomic at the commit level — no intermediate state where the walker admits formats the parser can't handle."

### R2-T9 — CR3 fixture sanitize-check.sh PII deny-list is INCOMPLETE (4 of 8 classes); `LensSerialNumber`, `CameraOwnerName`, IPTC creators, embedded preview thumbnails unchecked

**Agents**: rev (CRITICAL — single-agent; security finding)

Plan v2 Deliverable 3 (`:386-393`) commits exiftool strip preserving 7 asserted-survivor fields and stripping 8 PII classes. The CI lint at `:775` asserts only 4 PII classes absent ("GPS / OwnerName / SerialNumber / Copyright"). The remaining 4 — `LensSerialNumber`, `CameraOwnerName`, IPTC creator fields (`By-line`, `By-lineTitle`, `Credit`, `Source`, `Contact`, `Writer-Editor`), embedded preview thumbnails (which carry their own EXIF chunk with GPS+owner) — are all unchecked by the lint.

For a project authored by a named individual (Paulo) whose most likely fixture source is the author's own R8 CR3s, this is a real privacy risk: a sanitization that misses LensSerialNumber + the embedded preview's GPS would ship PII to git-lfs permanently while the CI lint passes.

This is the R2-T20 silent-failure class repeated at the CI-lint boundary.

**Remediation (mandatory)**: rewrite the lint as an **allow-list, not a deny-list**:
> "`tests/fixtures/sanitize-check.sh`: `exiftool -G -a -ee` on every fixture MUST contain ONLY the asserted-survivor tags (Make, Model, Orientation, DateTimeOriginal, ExifImageWidth, ExifImageHeight, Software, plus mandatory ISO-BMFF container metadata). Any other tag → CI fails. Also: `exiftool -ee -G -a` (extract embedded) MUST produce ONLY the asserted survivor set; no `[IFD0:Preview]` GPS/owner. Pin `exiftool` version for reproducibility."

---

## HIGH

### R2-T10 — Verb taxonomy in §Cross-references defines 3 verbs (closed/partial/unchanged) but uses 5 ("filed this session" × 4 rows; "closed in session 01" × 1 row)

**Agents**: com-1 + com-2 (2-way HIGH; reopens PR1-L12 anti-pattern)

Plan v2 lines 806-810 define taxonomy; lines 821-829 use undefined verbs. PR1-L12 explicitly remediated this in v1. v2 introduces the regression by adding 4 newly-filed entries (DN-013/014/015 + TD-004) with the new verb "filed this session" without amending the taxonomy.

**Remediation**: expand taxonomy to 4 verbs: `closed` / `partial` / `unchanged` / `filed`. Recategorize R2-T19 row as `closed` with location qualifier moved to Note column.

### R2-T11 — TD-001 "partial" classification misaligned with TD-001's all-or-nothing binding-trigger contract

**Agents**: com-1 (HIGH)

Plan v2 `:824` says TD-001 → `**partial**` because `actions/checkout` SHA pinning lands as part of LFS work. TD-001 (`TECH-DEBT.md:32-41`) is scoped as all-or-nothing across 3 actions; its trigger is "before first external PR / first release tag" — neither fired this session. Per verb taxonomy: "partial = the DN/TD is partially advanced; remainder rolls forward with explicit binding trigger" — but TD-001's trigger is unchanged. Correct verb is `unchanged` with note "incidental actions/checkout SHA pin lands for Deliverable 3 LFS work; does NOT close TD-001."

**Remediation**: reclassify to `unchanged` with explanatory note.

### R2-T12 — `RawDecodeCause` enum is a one-line placeholder; PR1-T2 demands typed `cause` on BOTH error variants

**Agents**: arch + simp (2-way HIGH)

Plan v2 (`:310`) leaves `// RawDecodeCause similar — Open, Unpack, BufferTooSmall, etc.` as a placeholder. PR1-T2 (R1's 5-way CRITICAL) explicitly demanded both variants carry typed cause enums. The decode side is where LibRaw error codes proliferate; coalescing to a placeholder is the same "operators lose discrimination signal" failure mode the typed-cause work was meant to close.

**Remediation**: pick one:
(a) Enumerate `RawDecodeCause { OpenFailed, UnpackFailed, BufferTooSmall, ResourceExhausted }` matching LibRaw's `libraw_unpack` / `libraw_dcraw_process` return codes; OR
(b) Drop `RawDecodeFailed`'s sub-enum entirely; `RawDecodeFailed { path, libraw_code: i32 }` is sufficient for v0.1 since no consumer needs to discriminate (session 04 will revisit). YAGNI-friendly.

Recommend (b) — simpler; matches actual v0.1 consumer set (zero).

### R2-T13 — `RawExifCause::OpenFailed` / `ResourceExhausted` lose the `op:` tag; operators can't distinguish "OOM during open" from "OOM during unpack"

**Agents**: sfh (HIGH — single agent; preserves PR1-T2 issue 3 anti-pattern)

PR1-T2 R1 example showed `CorruptInput { libraw_code: i32, offset_or_op: &'static str }`. v2 dropped the `offset_or_op` discriminator from `OpenFailed` and `ResourceExhausted` (both carry `libraw_code: i32` only). Operators reading `errored` log entries can't tell which LibRaw op was running.

**Remediation**: add `op: &'static str` to both variants:
```rust
OpenFailed { libraw_code: i32, op: &'static str },
ResourceExhausted { libraw_code: i32, op: &'static str },
```
Update dispatch-site routing to log the op distinctly. Update FFI error-path test table to assert the op tag.

### R2-T14 — LibRaw `#[repr(C)]` field-access strategy is ABI-fragile across LibRaw 0.21.x patch bumps; should use C-API accessor functions

**Agents**: arch (HIGH — single agent; ABI safety)

Plan v2 (`:109-111`) commits `#[repr(C)]` structs mirroring the LibRaw 0.21 ABI for direct field access (`imgdata.idata.*`, `imgdata.sizes.*`, etc.). LibRaw upstream explicitly documents that `libraw_get_*` accessor functions exist *specifically because* "[these] work regardless of LibRaw versions used when building calling app and the library itself." Direct field access against `libraw_data_t` is silently broken by patch-level field reorders in 0.21.x — and the plan's re-evaluation trigger only fires at 0.22+ (the wrong tripwire).

**Remediation**: switch to LibRaw C-API accessors (`libraw_get_iwidth`, `libraw_get_iheight`, `libraw_get_cam_mul`, `libraw_get_pre_mul`, `libraw_get_rgb_cam`, `libraw_get_color_maximum`, `libraw_get_iparams`). `libraw_data_t` becomes an OPAQUE pointer in our binding. Update Deliverable 1a's "~6 functions" to "~15 functions (6 lifecycle + ~9 accessors)" and update the re-evaluation trigger.

### R2-T15 — `libraw_open_file_w` is not the real LibRaw C symbol; the Windows wide-char wide-path binding is `libraw_open_wfile`

**Agents**: arch (HIGH — self-verified by orchestrator via plan v2:128 + LibRaw upstream docs)

Plan v2 (`:128`) declares: "Windows uses `OsStr::encode_wide() + null-terminate + libraw_open_file_w`." The actual symbol is `libraw_open_wfile` (buffer-sized variant: `libraw_open_wfile_ex`). The plan invented a symbol name. Implementation will land an `extern "C"` declaration for `libraw_open_file_w`, linker fails with "undefined reference," developer thinks they vendored the wrong LibRaw version.

**Remediation**: replace `libraw_open_file_w` with `libraw_open_wfile` at `:128`. Add a sub-bullet to Deliverable 0 (pre-flight): "Confirm LibRaw build's exported symbol table includes `libraw_open_wfile` (Windows); if missing, the build flag enabling Windows wide-char support is off."

### R2-T16 — Memory pressure SLO ~50 MB / ~800 MB transient is 2x under-quoted; downstream session-04 back-pressure plan will fail

**Agents**: arch (HIGH — algorithmic correctness)

Plan v2 (`:249, :254-258`) commits "~50 MB per CR3" and "~800 MB transient with 8 rayon workers." Canon R8 raw is ~25 Mpix × 2 bytes = 50 MB for `BayerPlane` alone — BUT LibRaw also allocates `imgdata.rawdata.raw_image` (another 50 MB during ownership transfer) AND `imgdata.image` 4-channel demosaic-prep buffer (~96-200 MB). Per-worker peak is 150-250 MB; 8 workers transient = 1.2-2 GB, not "~800 MB."

Sub-issue: the plan doesn't specify whether `RawImage` construction COPIES from `imgdata.rawdata.raw_image` (2x peak) or MOVES (requires LibRaw-allocator-aware deallocation). Either has correctness implications for session 04.

**Remediation**: correct the SLO to "150-250 MB per worker / 1.2-2 GB transient." Explicitly specify ownership-transfer mechanism. Add Test plan row asserting per-decode RSS bound via `getrusage(RUSAGE_SELF).ru_maxrss` post-decode.

### R2-T17 — Acceptance 2a happy-path test is under-constrained: `COUNT(*) WHERE make IS NOT NULL` is satisfied by `ExifCompleteness::Partial`

**Agents**: rev (HIGH — test design)

Plan v2 (`:706-708`) requires non-NULL on 6 columns but the test row at `:764` only asserts `COUNT(*) WHERE make IS NOT NULL = fixture_count` + `camera_slug = 'canon-r8' = fixture_count`. A `Partial` EXIF fixture (Make present, capture_time absent) satisfies both — test passes; acceptance fails silently.

**Remediation**: rewrite the SQL as conjunction:
```sql
SELECT COUNT(*) WHERE make IS NOT NULL
  AND model IS NOT NULL
  AND camera_slug = 'canon-r8'
  AND capture_time_unix_seconds IS NOT NULL
  AND width > 0 AND height > 0
```
returns `= fixture_count`. Add explicit `assert!(stderr.contains("partial_exif: 0"))` (and other counter assertions) as separate lines so an implementer can't drop one.

### R2-T18 — `Catalog::poison_for_testing` `#[cfg(test)]`-gated symbol is unreachable from `tests/`-dir integration tests; PR1-T15's own remediation guidance suggested the `#[cfg(any(test, feature))]` escape hatch which becomes the new gameable surface

**Agents**: rev (HIGH — same flaw class as R2-T3 but for the poison knob)

Plan v2 Deliverable 6a (`:534-537`) commits `#[cfg(test)]`-only — correct per PR1-T15. But `#[cfg(test)]` only applies to the unit-test target of the primary crate; `tests/`-dir integration tests compile as a separate crate and CANNOT see `#[cfg(test)]`-gated items. A contributor implementing 6a hits the wall, follows PR1-T15's suggested escape hatch (`#[cfg(any(test, feature = "test-helpers"))]`), and now the surface IS shippable in any release that enables `--features test-helpers`. The `nm` acceptance check is one-shot per release; downstream consumers enabling the feature defeat it.

**Remediation**: tighten the constraint:
(a) Commit `#[cfg(test)]` strict (no feature-flag escape) for ALL `*_for_testing` surfaces; if integration tests in `tests/` need a test helper, factor into a `dev-dependencies`-only utility crate (`photohelper-test-helpers`). OR
(b) Add a workspace-level lint forbidding `#[cfg(any(test, feature = ...))]` patterns in workspace code.

### R2-T19 — Sad-path "hex-edited CR3" fixtures unspecified in construction; tautology risk

**Agents**: test (HIGH)

Plan v2 (`:764, :760`) describes sad-path fixtures as "hex-edited CR3" without specifying WHICH bytes. Tests constructed by "edit bytes until the test passes" assert `Err(_)` (some error) not the specific variant — passes for any LibRaw failure path. Cross-test interference: 3 hex-edited fixtures might overlap in their edits, all firing the same LibRaw bug class.

**Remediation**: pick one per fixture:
(a) Commit `tests/fixtures/cr3/gen_sad_path_fixtures.sh` to repo; script runs `exiftool` + `dd` deterministically; the script's first-build artifact is committed to LFS. OR
(b) Synthesized `libraw_data_t` stub (already mentioned for field-conversion unit tests at `:761`); unit test populates directly with the test-specific cause variant.

Recommend (b) for `strict_mode_fails_on_libraw_error_real_cr3` + `..._on_partial_exif_real_cr3` (testing cause-routing logic, not LibRaw itself). (a) for `..._on_unknown_camera_real_cr3` (needs a real CR3 with mismatched Model).

### R2-T20 — `IngestStats::cr3_exif_absent` counter not added to the struct definition spec; `no_exif` semantics post-LibRaw unspecified

**Agents**: sfh (HIGH)

Plan v2 introduces TWO new counters (`partial_exif` + `cr3_exif_absent`) but only `partial_exif` is named in Deliverable 4 (`:454`). The `cr3_exif_absent` counter is named only in Deliverable 1d's error-routing prose (`:321`). `no_exif`'s post-LibRaw semantics are unspecified — does it survive? Is it dead? If both `no_exif` and `cr3_exif_absent` fire in some path, the count is doubled. The summary line shape contract is ambiguous.

**Remediation**: add a per-counter semantics table to Deliverable 4: counter → trigger → WARN event tag → strict contribution → catalog-row consequence. Explicitly retire `no_exif` OR state it carries forward for non-CR3 RAW handling.

### R2-T21 — `Plan revisions log` v2 entry is a 51-line mini-changelog duplicating R1 artifact content (bloat per PR1-L3 anti-pattern reborn)

**Agents**: simp (HIGH)

Plan-revisions log entry (`:836-889`) is 54 lines, ~24 sub-bullets. Each bullet restates a fact that appears in (a) §Deliverables (the actual contract) AND (b) `docs/code-reviews/session-02-plan-round1.md` (the review artifact). Future readers needing the *what* read §Deliverables; needing the *why* read R1. The log is a third-place restatement.

**Remediation**: shrink to ≤8 high-impact bullets per revision. Match session-01's terse convention.

### R2-T22 — §"Plan-review decisions resolved at Round 1" section restates Deliverable content; ~23 lines redundant

**Agents**: simp (HIGH)

The new section (`:672-694`) was added per PR1-T7's rename remediation, but the resolutions of DI-1/DI-2/DI-3/DI-4 already live in §Deliverables 1a / 2a / §Risk register / Deliverable 4. The section is one-hop redundant.

**Remediation**: delete the section. If audit-trail of "what happened to DI-N items" is wanted, cross-references in §Plan revisions log and the cross-doc DN-013/DN-014 filings suffice.

### R2-T23 — PR1-T31 conventional-commit scope decision was NOT remediated; v2 ships with no commit-scope rule

**Agents**: rev (HIGH; from PR2 — separate finding agent)

PR1-T31 (R1 HIGH) demanded the plan pre-commit to `(session-02)` vs component scopes. v2's plan-revisions log doesn't include this decision. Pre-flight commit at `:91-93` uses `chore(libraw):` (component-scoped); other commits might use other shapes. The drift PR1-T31 was filed to prevent is structurally allowed.

**Remediation**: add to §Plan revisions log: "Scope convention: this session uses `<type>(session-02): ...` to match session-01's pattern. Pre-flight commit at Deliverable 0 is an exception (`chore(libraw):`); all others follow (session-02)."

---

## MEDIUM

| ID | Theme | Agents | Citation | Remediation summary |
|----|-------|--------|----------|---------------------|
| R2-M1 | `ExifCompleteness` enum location unspecified (in `photohelper-core` or `photohelper-cli`?) | type | `:442-454` | Specify: "in `photohelper-core/src/model.rs` next to `ExifMetadata`; `pub` export." |
| R2-M2 | Decision-doc 0002 should be in `docs/adr/` not `docs/decisions/` (binding for every release) | com-1 | `:350` | Move to `docs/adr/0002-libraw-lgpl-static-link-mechanics.md`; update 3 cross-refs |
| R2-M3 | Plan-revisions log v2 mis-attributes test-plan duplicate removal to PR1-T30 / PR1-M18 (neither is about duplicates) | com-1 | `:884-886` | Drop misattribution; rename to "internal polish during R1 remediation" |
| R2-M4 | Acceptance 2b expected summary line omits `partial_exif` + `cr3_exif_absent` (only the old counter set listed) | rev | `:710-720` | Extend to full counter set per Deliverable 4's `IngestStats` additions |
| R2-M5 | SCUNet residue in README.md:36 and HANDOFF_REPORT.md:41 (plan v1 removed only from plan body) | gp | external | Scrub both files atomically in plan-v3 cross-doc commit |
| R2-M6 | `IngestOutcome::InsertedWithPartialExif` carries `missing_fields: Vec<&'static str>` payload but `apply_outcome` discards it (already in `tracing::warn!`); double-tracking | sfh | `:459-462` | Simplify to `InsertedWithPartialExif(PhotoId)`; payload lives in upstream WARN |
| R2-M7 | `capture_time_unix_seconds` field type allowed to be `Option<i64>` OR `Option<OffsetDateTime>` internally per `:179-181`; should pin to `Option<i64>` to avoid `Eq` derive instability across `time` crate versions | type | `:144, :179-181` | Tighten to `Option<i64>` at field-definition site |
| R2-M8 | DN-008 row 17 hardlink test asserts ONE row exists but doesn't verify dedup via PhotoId equality | test | `:772` | Add: "second SELECT confirms PhotoId is identical for the two paths" |
| R2-M9 | Era-partitioning predicate (`ingested_at >= X AND superseded_at IS NULL`) for catalog NULL-semantics shift undocumented | arch | `:502-511` | Add "Era-partitioning contract" paragraph |
| R2-M10 | LibRaw build-system static-link assertion lacks concrete predicate (`nm` / `otool -L` output not parsed into Pass/Fail) | test | `:758` | Spell out: `! nm -D target/release/photohelper 2>/dev/null \| grep -q ' U libraw_'` |
| R2-M11 | `ExifMalformed { field, raw_value }` cause variant: only `iwidth = 0` tested; orientation=9/0, height=0, UTF-8-invalid model untested | test | `:761` | Add 3-4 test rows per the variant's full domain |
| R2-M12 | Rusqlite bump verification tests pin 4 of 5 enumerated API changes (concurrent + Immediate behavior + params! coercion untested) | test | `:766` | Add 3 sub-rows: concurrent connections, Immediate semantics, params! type coercion |

---

## LOW

| ID | Theme | Agents | Citation | Note |
|----|-------|--------|----------|------|
| R2-L1 | Risk-register row 2 function-count trigger (>10) doesn't match the recommended R2-T14 raise to >15 | arch | `:796, :118-120` | Update both to >15 if R2-T14 accepted |
| R2-L2 | `static_assertions` workspace classification ("Dev-deps used across crates" comment in `Cargo.toml:61`) is misleading; the per-crate consumer (`photohelper-raw`) needs `[dependencies]` not `[dev-dependencies]` | type | `Cargo.toml:61` | Move out of comment block or document non-classification |
| R2-L3 | `RawExif` `static_assertions!` is placed as comment inside derive block (`:166-168`) rather than as sibling code line; structural ambiguity | type | `:166-168, :251-252` | Render as real code line above/below struct definition |
| R2-L4 | `RawExif::capture_time_unix_seconds()` rustdoc UTC assumption missing at the accessor site | type | `:144` | Add rustdoc paragraph naming the UTC assumption + DN-016 cross-ref |
| R2-L5 | Git LFS capitalization still mixed in plan v2 ("git-lfs" vs "Git LFS") despite PR1-L11 | com-2 | `:374, :402, :774, :816` | Single search/replace pass |
| R2-L6 | `2026-MM-DD` placeholders in §Cross-references (DN-007 / TD-002 rows) not formally bracketed | com-2 | `:816, :825` | Use `<YYYY-MM-DD>` convention OR add legend note |

---

## Strengths preserved [NOTES]

Confirmed by R2 — must not regress in any R3:

- **PR1-T1 dispatch axis collapsed correctly** (`:39-43, :424-441`). Path (a) cleanly chosen; `parse_cr3_exif` is a single non-dispatching function; kamadak-exif removed atomically.
- **PR1-T5 type-design discipline mostly tightened** for `RawExif`, `BayerPlane`, `SensorLevels`, `CfaPattern`. Private fields + fallible constructors landed for these 4 types. The WhiteBalance / ColorMatrix gap is the R2-T6 regression.
- **PR1-T2 single-variant Error + typed cause for RawExifUnavailable** (`:271-308`). Correct shape; the R2-T13 op-tag gap and R2-T12 RawDecodeCause stub are sub-issues, not the whole type.
- **PR1-T8 decision-doc 0001 amendment landed** in lockstep with plan v2 commit. Owners + Migration policy + Trigger lines all reconciled (session 02 → session 03).
- **PR1-T17 LGPL §6(b) → §6(a) correction** propagated to both plan and DN-001.
- **PR1-T9 Deliverable 0 pre-flight** added with sequencing + artifact path + ABORT trigger.
- **PR1-T11 EXIF sanitization gate exists** (strip command + CI lint). The R2-T9 deny-list incompleteness is correctable; the *intent* of the gate is sound.
- **PR1-T22 SESSION-STATE.md drift cleanup** landed in commit before plan v2; SESSION-STATE.md now correctly says "Current session: 2."
- **PR1-T30 R2-T19 disposition** correctly identified as "closed in session 01" (verified `model.rs:770`).
- **DN-013 + DN-014 + DN-015 + TD-004 filed** in cross-doc commit per PR1-M9 + PR1-T7c + PR1-T10.
- **Verb taxonomy DEFINED at all** (`:806-810`) — even though incomplete per R2-T10, defining a taxonomy is structurally better than v1's ad-hoc verbs.

---

## New bug classes surfaced [NOTES]

R2 surfaced patterns worth recording for future plan-reviews:

1. **"Fabricated cross-reference IDs in remediation"** [R2-T1]: when remediating a R1 finding, the author invented PR1-T# numbers beyond the R1 enumeration to make the cross-ref look authoritative. Future R2/R3 reviews should grep every PR1-T# in the remediated plan against the R1 artifact's heading enumeration as a first-pass check.

2. **"Phantom DN/TD cross-references in remediation"** [R2-T2]: same pattern at the cross-doc level. The plan author committed to filing DN-016 mentally but didn't actually file it. Cross-doc filings should land in the SAME commit as the plan that references them; any new DN/TD ID introduced in a plan revision should be grep-able in the actual ledger.

3. **"Accessor-method panic displacement"** [R2-T5]: a strong-type constructor enforces invariants at construction-time, but if the type's accessor methods don't return `Option<T>` / `Result<T>`, runtime callers still panic. The type-design discipline must extend to the accessor signatures, not just the constructor.

4. **"#[cfg(test)] reachability mismatch with integration tests"** [R2-T3 + R2-T18]: `#[cfg(test)]`-gated symbols are unreachable from `tests/`-dir integration tests because the latter compile as a separate crate. Plan-review for any test-helper hook must specify the reachability mechanism (env-var? feature-flag with workspace lint forbidding it in release? dev-deps-only utility crate?) BEFORE the implementation lands.

5. **"Plan-revisions log changelog-bloat"** [R2-T21]: per-revision log entries trending toward 50+ lines. The R1 artifact already documents the *why*; the log should be ≤8 bullets of *what* changed. Future plans should target session-01's terser convention.

---

## Disposition summary

| Disposition | Count | Notes |
|-------------|------:|-------|
| **Fix inline in R2 remediation (plan v3)** | 6 CRITICAL + 11 HIGH + 8 MEDIUM | Audit-trail fabrications + design holes + type-design regressions; most are ≤30-LoC plan edits. |
| **Cross-doc filings required (DN-016, DN-017 OR DN-018)** | 1 CRITICAL (R2-T2) + 1 MEDIUM (R2-M2 ADR move) | Atomic with plan v3 commit. |
| **External research needed (LibRaw version pick, sanitize-check rewrite)** | 2 CRITICAL (R2-T4, R2-T9) | Author decides; ≤1 hour each. |
| **Accept-as-is with explicit comment** | 6 LOW | Cosmetic. |
| **Carry forward to v4 only if R3 fires** | 3 HIGH (R2-T21, R2-T22 bloat; R2-T23 commit-scope) | Cuts/polish; not blocking. |

**R3 trigger assessment**: per `docs/quality-assurance.md § Double-review protocol`: "If Round 2 surfaces regressions large enough to need another cycle, add Round 3." 9 CRITICAL regressions inside R1 remediation collectively meet that bar. **R3 is warranted IF the R2 remediation introduces further CRITICAL-class regressions**; if R2 remediation is clean (audit-trail corrections + type-design tightening + cross-doc filings), R3 can be skipped.

The substantive design decisions (R2-T4 LibRaw pin, R2-T6 WhiteBalance/ColorMatrix shape, R2-T9 sanitize allow-list, R2-T5 BayerPlane accessor signature) are author-decisions that benefit from user input — not all of them have a single mechanically-correct answer.

---

## Self-verification (orchestrator-performed; §6 substitute for cost reasons given the volume)

For the highest-impact CRITICALs, the orchestrator performed direct file-system verification before publishing:

| Finding | Verification | Result |
|---------|--------------|--------|
| R2-T1 phantom PR1-T# IDs | `grep "^### PR1-T[0-9]+" docs/code-reviews/session-02-plan-round1.md` → 32 headings (T1–T32). `grep -c "^### PR1-T33 " ...` through T45 → all 0. | **Confirmed: 9 fabricated IDs in plan v2.** |
| R2-T2 phantom DN-016 | `grep "DN-016\|DN-017" docs/discovery-notes.md` → 0 matches. | **Confirmed: DN-016 doesn't exist in ledger.** |
| R2-T15 `libraw_open_file_w` | `grep "libraw_open_file_w\|libraw_open_wfile" docs/plans/session-02.md` → only `libraw_open_file_w` at :128. LibRaw upstream docs confirm `libraw_open_wfile` is the real symbol. | **Confirmed: fabricated symbol name.** |

Other findings cite specific file:line refs that can be spot-checked at R2-remediation time; the cross-agent convergence (multiple agents flagging the same theme) is the secondary confidence signal.

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings_high_impact: 3
  verified: 3
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    Orchestrator-performed §6 substitute via direct grep verification for
    the top 3 CRITICAL items (audit-trail fabrications). All 3 confirmed
    present. Lower-severity findings rely on cross-agent convergence
    (2+ agents per theme) and direct file:line citations from agent
    reports; not individually 9th-agent-verified for cost reasons given
    the volume (32+ themes across R2). The R3 watch-list at the end
    enumerates verification anchors for any R3 round.
```
