# Session 06 — Plan Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent); agents pinned to opus"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
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

| Severity | Count |
|---|---|
| CRITICAL | 3 |
| HIGH | 9 |
| MEDIUM | 6 |
| LOW | 0 |
| **Total** | **18** |

---

## Theme A — XMP sidecar path convention is wrong for Lightroom RAW files [CRITICAL]

**Agents**: code-architect, pr-test-analyzer (multiple)

The plan specifies sidecar path as `photo.CR3.xmp` (extension **appended**). Adobe Camera Raw and
Lightroom Classic use the **extension-replaced** convention for RAW files: `photo.CR3 → photo.xmp`.
The `.CR3.xmp` convention is Darktable's convention, not Lightroom's.

`docs/plans/session-06.md:377` test: `path_written_as_dotxmp_extension | For photo.CR3 → sidecar at photo.CR3.xmp`

If implemented as written, every user who runs `photohelper develop` and opens Lightroom will see
zero applied settings — the sidecar exists on disk but Lightroom never finds it. The entire purpose
of the `develop` subcommand is defeated.

**Verified**: present=yes at plan:377 (`photo.CR3.xmp`).

**Remediation**: Use `Path::with_extension("xmp")` (replaces the last extension). For `IMG_0001.CR3`
this produces `IMG_0001.xmp`. Update the test name + assertion. Document the choice with a
`docs/decisions/` entry citing the XMP Part 3 spec. Add an optional `--sidecar-convention
<lightroom|darktable>` flag for future-proofing if Darktable users (`.CR3.xmp`) are also target
audience — but default to the Lightroom convention.

---

## Theme B — Atomic XMP write not specified; partial sidecar on crash corrupts Lightroom [CRITICAL]

**Agents**: pr-review-toolkit:silent-failure-hunter

The plan proposes `write_xmp(path, settings) -> Result<(), Error>` writing directly to the target
path with no atomic-write semantics. If the write fails mid-stream (disk full, SIGKILL,
power loss), a partial `.xmp` file is left on disk. Lightroom reads sidecars eagerly at startup —
a corrupt sidecar causes Lightroom to error or silently drop develop settings for that photo.

A subsequent `photohelper develop` run calls `merge_and_write`, which calls `read_xmp` on the
corrupt file, returns `Err(XmlParse)`, counts the photo as `errored`, and moves on —
**leaving the corrupt sidecar on disk permanently with no recovery path**.

No mention of `atomic`, `temp`, `rename`, or `mktemp` anywhere in the plan (verified).

**Remediation**: Specify atomic write in D3b: write to `<path>.phdev.tmp`, `fsync`, then
`fs::rename` to the target. Add `Error::AtomicWriteFailed` variant. Add test
`write_xmp_atomic_no_partial_on_io_error`.

---

## Theme C — Conflict resolution undefined when timestamps absent; silent data loss [CRITICAL]

**Agents**: pr-review-toolkit:silent-failure-hunter, feature-dev:code-architect

The plan's conflict resolution (lines 86-98) specifies:
1. Read `xmp:MetadataDate` from existing file.
2. Read `ph:LastProcessedAt` from our prior write "(if present)".
3. Compare and decide.

But steps 3-4 assume both timestamps exist. The plan does not specify behavior for:
- **Case A** (`xmp:MetadataDate` absent, `ph:LastProcessedAt` present): A sidecar from a tool that
  doesn't write `xmp:MetadataDate`. Undefined comparison → implementation defaults to "we are newer"
  → silently overwrites user's existing `crs:` edits.
- **Case B** (`ph:LastProcessedAt` absent, `xmp:MetadataDate` present): A Lightroom-written sidecar
  with no `ph:` namespace. Same silent overwrite risk.
- **Case C** (both absent): A bare third-party sidecar.

The plan claims "No silent data loss: the conflict decision is always logged at INFO level" —
but this guarantee is impossible to fulfill without specifying the missing-timestamp cases.

**Verified**: conflict resolution section at plan:90 — `(if present)` but no fallback specified.

**Remediation**: Add explicit table covering all 4 cases with conservative defaults (preserve on
ambiguity). Also specify that timestamps must be parsed as `time::OffsetDateTime` (not compared as
strings) to handle timezone-offset differences correctly. See `docs/plans/session-06.md § Remediation spec` below.

Proposed resolution table:
| `xmp:MetadataDate` | `ph:LastProcessedAt` | Action |
|---|---|---|
| `Some(md)` | `Some(lp)` | `md > lp` → ConflictPreserved; else Overwrite |
| `Some(md)` | `None` | ConflictPreserved (first run; preserve existing `crs:`) |
| `None` | `Some(_)` | ConflictPreserved + `tracing::warn!` (unknown existing timestamp) |
| `None` | `None` | Check if any `crs:` present → if yes, ConflictPreserved + `tracing::warn!`; if no, Created |
| `--force` | any | ForcedOverwrite regardless |

---

## Theme D — Phantom constant `MODEL_MANIFEST_NAME_NIMA`; NIMA scores always None [HIGH]

**Agents**: general-purpose (consistency), feature-dev:code-architect

Plan line 438: `catalog.all_photos_with_cull_scores(MODEL_MANIFEST_NAME_NIMA)`
Plan line 473: `Wire NIMA_MODEL_SLUG constant (from photohelper_ai::MODEL_MANIFEST_NAME)`

Neither `MODEL_MANIFEST_NAME_NIMA` nor `NIMA_MODEL_SLUG` exist in `crates/photohelper-ai/src/model_bytes.rs`.
The actual constants are:
- `MODEL_SLUG = "nima-aesthetic-v1"` — **this is what `cull.rs` writes into `cull_scores.model_slug`**
- `MODEL_MANIFEST_NAME = "nima_mobilenet_aesthetic"` — the manifest section name (NOT the catalog column value)

If the query is called with `MODEL_MANIFEST_NAME` (`"nima_mobilenet_aesthetic"`), the `LEFT JOIN`
`AND cs.model_slug = ?1` matches zero rows. Every photo returns `nima_score: None` regardless of
whether cull has run. NIMA scores silently never appear in XMP sidecars.

**Verified**: present=yes at plan:438; neither constant found in model_bytes.rs.

**Remediation**: Replace both occurrences with `photohelper_ai::MODEL_SLUG`.

---

## Theme E — `DevelopRow` missing `photo_id`; plan re-derives PhotoId unnecessarily [HIGH]

**Agents**: feature-dev:code-architect, pr-review-toolkit:type-design-analyzer, pr-review-toolkit:code-simplifier

`DevelopRow` as specified: `{ source_path: PathBuf, nima_score: Option<f32> }` — no `photo_id`.

Plan line 444 then says "Populate `settings.photohelper_id` from PhotoId derived from path" —
`PhotoId::derive(&path)` reads up to 128KB of each photo file plus BLAKE3 hashing. For 370 photos
this is ~47 MB of I/O. This contradicts the plan's own "I/O-bound, not CPU-bound" rationale for
sequential execution.

Larger problem: `PhotoId::derive` can fail (file deleted, zero-length, I/O error). The plan
specifies no error handling for this path (see Theme H). The catalog already has `p.id` — add it
to the SELECT.

**Verified**: present=yes at plan:398 (`DevelopRow` lacks `photo_id`), plan:444 (re-derive from path).

**Remediation**: Add `photo_id: PhotoId` to `DevelopRow`. Extend query to `SELECT p.id, p.source_path,
cs.aesthetic_score`. Populate `settings.photohelper_id` from `row.photo_id().to_string()`.
This eliminates all per-photo disk I/O for the PhotoId, making develop truly I/O-bound only on
sidecar writes.

---

## Theme F — `SidecarSettings` public fields bypass validation; `WhiteBalance::Custom` unguarded [HIGH]

**Agents**: pr-review-toolkit:type-design-analyzer (primary), pr-review-toolkit:code-reviewer

Two sub-issues in D3a:

**F1 — Public fields + undefined validation timing**: All `SidecarSettings` fields are `pub`.
Validation rules are listed below the struct but no constructor or builder is specified. Any caller
can write `settings.temperature = Some(99_999)` and bypass validation. The project's invariant
pattern (private fields + fallible constructor, per `Photo`, `ImageEmbedding`, `NimaScore`) is broken.

**F2 — `WhiteBalance::Custom` co-requirement**: When `crs:WhiteBalance = "Custom"`, Lightroom
respects `crs:Temperature` + `crs:Tint`. For any other `WhiteBalance` variant, Lightroom ignores
explicit Temperature/Tint values. The plan permits `temperature: Some(5500)` with `white_balance:
Some(AsShot)` — the user sets `--temp 5500` and Lightroom silently ignores it. The plan also
permits `white_balance: Some(Custom)` with `temperature: None` — Lightroom cannot honor Custom WB.
Neither invalid state is validated.

Also: `WhiteBalance` enum variants must map to Lightroom's exact string values. `AsShot` → `"As Shot"`
(with space); the plan doesn't specify a `Display` impl.

**Verified**: present=yes at plan:255 (pub fields), plan:279 (Custom variant), plan:284-287 (validation list but no constructor).

**Remediation**: Make fields private. Add `SidecarSettings::builder()` or `new()` → `Result<Self, Error>`.
Add cross-field validation: if `white_balance == Some(Custom)`, require `temperature.is_some()
&& tint.is_some()`; if `temperature.is_some() || tint.is_some()`, require `white_balance ==
Some(Custom)`. Specify `impl Display for WhiteBalance` mapping to Lightroom strings.

---

## Theme G — `WriteOutcome`↔`DevelopStats` counter mapping undefined; `ForcedOverwrite` unaccounted [HIGH]

**Agents**: feature-dev:code-reviewer, pr-review-toolkit:type-design-analyzer

`WriteOutcome { Written, ConflictPreserved, ForcedOverwrite }` — 3 variants.
`DevelopStats` counters: `written`, `updated`, `conflict_preserved`, `errored` — 4 buckets.
Neither the plan nor any code sketch specifies:
- Does `Written` map to `written` (new file) or `updated` (overwrite when we're newer)?
- `ForcedOverwrite` maps to which counter? It doesn't appear in any counter list.
- The summary line (plan:460) has no `force-overwritten` field.

Without this spec, the implementor must guess. Wrong guess produces misleading summary output and
invisible force-overwrite counts.

**Verified**: present=yes at plan:331 (`WriteOutcome`), plan:451 (`DevelopStats`).

**Remediation**: Expand `WriteOutcome` to 4 variants mapping 1:1 to stats counters:
`Created` → `written`, `Overwritten` → `updated`, `ConflictPreserved` → `conflict_preserved`,
`ForcedOverwrite` → dedicate a `force_overwritten: AtomicU64` counter or map to `updated`.
Add `force_overwritten` to the summary line. Document the mapping explicitly in the plan.

---

## Theme H — D4b per-photo error handling gaps; missing `derive_failed` + `file_missing` [HIGH]

**Agents**: feature-dev:code-reviewer, pr-review-toolkit:silent-failure-hunter

D4b step 6b: "Populate `settings.photohelper_id` from PhotoId derived from path" — no failure spec.
D4b step 6d: `merge_and_write(&sidecar_path, &settings, args.force)` — called even if source file was deleted.

The cull pipeline has: `file_missing` check (step 1), then `derive_failed` (step 2), then decode, then infer.
The dedup pipeline has: `file_missing` check, `derive_failed`, `decode_failed`, `infer_failed`.
The develop plan has: none of these pre-checks, and a single `errored` catch-all.

Specific gaps:
1. No existence pre-check on `source_path` (photo may have been deleted since ingest).
2. `PhotoId::derive` failure routing: should increment `derive_failed`, not generic `errored`.
3. `merge_and_write` `Err` routing: plan says "Count outcome in `DevelopStats`" — ambiguous.
4. No specification that the loop continues on per-photo error (vs aborting the batch).

**Verified**: present=yes at plan:451 (DevelopStats lacks `derive_failed`, `file_missing`).

**Remediation**: Add pre-steps to D4b loop matching cull/dedup pattern:
- Step 6a-pre: `if !source_path.exists() { warn!; stats.file_missing++; continue; }`
- Step 6a: `PhotoId::derive` → on `Err`: `warn!; stats.derive_failed++; continue`
- Step 6d error: `merge_and_write` `Err` → `warn!; stats.errored++; continue`
- Add `file_missing: AtomicU64`, `derive_failed: AtomicU64` to `DevelopStats`.
- Explicitly state: "per-photo errors never abort the batch; only fatal setup failures propagate."

---

## Theme I — `/tmp/preview.jpg` clobber in parallel CI (TD-009 sanitize-check) [HIGH]

**Agents**: pr-review-toolkit:silent-failure-hunter

D2a pseudocode (plan:192-196):
```bash
exiftool -b -PreviewImage "$fixture" > /tmp/preview.jpg 2>/dev/null || true
if [ -s /tmp/preview.jpg ]; then ...
```

Two parallel CI jobs on the same host share `/tmp/preview.jpg`. Race: job A writes fixture-A's
preview, job B overwrites with fixture-B's preview, job A checks fixture-B's preview instead.
**Silent false negative**: PII in fixture-A's embedded preview bypasses the sanitization gate
because job A read a different fixture's clean preview. GPS coordinates, owner name, serial numbers
could ship to the public repo.

**Verified**: present=yes at plan:192 (`/tmp/preview.jpg` hardcoded).

**Remediation**: Use `mktemp`: `preview_tmp=$(mktemp /tmp/ph-sanitize-XXXXXX.jpg)`; use
`"$preview_tmp"` throughout; `rm -f "$preview_tmp"` at end or via `trap`.

---

## Theme J — XMP reader behavior on malformed field values unspecified [HIGH]

**Agents**: pr-review-toolkit:silent-failure-hunter (primary)

Plan D3c says "Unknown fields are silently ignored (forward-compatibility)." This handles unknown
*field names*. It says nothing about **known fields with malformed values**:
- `crs:Temperature="not-a-number"` — known field, unparseable integer
- `ph:NimaScore="NaN"` — known field, non-finite float
- `xmp:MetadataDate="invalid-date"` — known field, unparseable ISO 8601

If the reader returns `Err(XmlParse)` on any malformed field, the entire sidecar read fails,
`merge_and_write` propagates the error, the photo is counted as `errored`, and the user's
existing Lightroom sidecar is preserved untouched but no develop settings are written.

If the reader silently returns `None` for malformed fields without logging, the conflict resolution
logic sees `xmp:MetadataDate = None` and potentially silently overwrites the user's edits (Theme C).

The plan must choose explicitly.

**Remediation**: Specify lenient read: known fields with malformed values → log `tracing::warn!`
per field + treat as `None`. Entire read succeeds. Add test `read_malformed_temperature_warns_and_returns_none`.

---

## Theme K — Critical test gaps: stub removal, boundary validation, content verification [HIGH]

**Agents**: pr-review-toolkit:pr-test-analyzer (primary)

Seven test gaps in D3/D4:

1. **Stub test breakage** (HIGH): `cli.rs:326` lists `"develop"` in `stub_subcommands_exit_69`.
   When D4b lands real `develop`, this test breaks. Plan never mentions updating it. This will
   cause `just test` to fail immediately after D4b lands.

2. **Only 2 of 7 validated fields have boundary tests**: `temperature_out_of_range` and
   `exposure_out_of_range` tested. `tint ∈ [-150,150]`, `contrast/highlights/shadows/clarity/
   vibrance/saturation ∈ [-100,100]` — none tested.

3. **No write I/O error test**: `Error::Io` is declared but never exercised. No test verifies
   that `write_xmp` returns `Err(Io)` on a permissions-denied path.

4. **CLI flags not verified in sidecar content**: `develop_writes_nima_score_when_culled` checks
   `ph:NimaScore` but no test verifies that `--temp 5500`, `--exposure 1.5` actually appear in the
   written `crs:Temperature` / `crs:Exposure2012` fields.

5. **`conflict_preserved` counter not tested**: No test exercises a path where
   `conflict_preserved > 0` appears in the summary line.

6. **`develop_strict_exits_nonzero_on_error` description contradicts its name**: Plan says "If
   catalog is empty, not an error (exit 0)" — but the test is named for non-zero exit. These are
   two different scenarios. Both need dedicated tests.

7. **Model slug mismatch LEFT JOIN untested**: No test where `cull_scores` has a row with the
   wrong `model_slug` — the JOIN filter `AND cs.model_slug = ?1` is untested.

**Remediation**: Add to D4b test list: update-stub-test note, `develop_cli_flags_written_to_sidecar`,
`develop_conflict_preserved_appears_in_summary`, corrected `develop_strict_exits_nonzero_on_error`.
Add to D3 test list: `tint_boundary_rejected`, `int_field_boundary_rejected`, `write_xmp_to_readonly_dir_returns_io_error`, `all_photos_wrong_model_slug_returns_none_score`.

---

## Theme L — D1 ordering risk; high-variance D1 should follow low-risk D2 [HIGH]

**Agents**: pr-review-toolkit:code-simplifier

Current plan ordering: D0 → D1 → D2 → D3 → D4 → D5.

D1 (session-02 8-agent review) is context-intensive: fires 8 agents, R1 + remediation + R2
against the full LibRaw FFI diff. If R1 surfaces CRITICAL findings in `photohelper-raw`, the
remediation commits could consume significant context budget before D2-D5 begin.

D2 contains 5 independent, low-risk, quickly-committable tasks (TD closures, version bump,
shell script fix). Completing D2 first guarantees 7 TDs addressed even if session context
runs out before D1 completes.

**Remediation**: Reorder to D0 → D2 → D1 → D3 → D4 → D5. D2's sub-tasks are independent
of each other and of D1. Front-loading them banks concrete progress.

---

## Theme M — D2 planned as single commit; bundles 5 unrelated TD closures [MEDIUM]

**Agents**: feature-dev:code-reviewer, pr-review-toolkit:comment-analyzer

Plan line 186: `fix(session-06): D2 — TD-004/TD-005/TD-009/TD-014/TD-020 closure`

Five unrelated changes in one commit violates CLAUDE.md "one logical change per commit." Also `fix:`
is wrong for most items (TD-004/TD-009 are additions, TD-005 is bookkeeping, TD-014 is a dep bump).

**Remediation**: Split into:
- `fix(scripts): D2a — TD-009 sanitize-check.sh stage-2 embedded-preview check`
- `chore(ci): D2b — TD-004 osv-scanner libraw CVE monitoring`
- `chore(session-06): D2c — TD-005 formal closure (env-var panic removed in session 05 D4)`
- `chore(deps): D2d — TD-014 ort stable version check` (or bump if released)
- `fix(ai): D2e — TD-020 CLIP bicubic center-crop preprocessing`

---

## Theme N — Unused `photohelper-core` dependency; will trigger lint [MEDIUM]

**Agents**: general-purpose (consistency), pr-review-toolkit:comment-analyzer

Plan D3 adds `photohelper-core` to `photohelper-sidecar/Cargo.toml`. The current Cargo.toml
comment explicitly says it was "dropped in D4 to satisfy the new `unused_crate_dependencies`
workspace lint." None of the proposed `SidecarSettings`, reader, writer, or error types use any
type from `photohelper-core` (all fields are primitives: `String`, `i32`, `f32`, `i64`).

Adding an unused dep will trigger the same lint that caused its removal.

**Remediation**: Remove `photohelper-core` from the D3 dependency list. If `ph:PhotohelperId`
needs `PhotoId`'s base64url format, do the conversion in `develop.rs` (the CLI command) before
passing the already-rendered `String` to `SidecarSettings`.

---

## Theme O — Dead `SidecarSettings` fields with no CLI surface [MEDIUM]

**Agents**: pr-review-toolkit:code-simplifier, pr-review-toolkit:comment-analyzer

`SidecarSettings` declares `clarity`, `vibrance`, `saturation` (3 fields), and `white_balance`
(`WhiteBalance` enum with 9 variants) — but `DevelopArgs` exposes no `--clarity`,
`--vibrance`, `--saturation`, or `--white-balance` flags. These fields will always be `None`
throughout the develop pipeline in v0.1.

`process_version` is auto-set by the writer ("11.0" when any `crs:` field is written) and should
not be a user-facing `pub` field.

This contradicts the plan's own § Scope justification citing "Do not design for hypothetical
future requirements."

**Remediation**: Remove `clarity`, `vibrance`, `saturation`, and `white_balance` from
`SidecarSettings` for v0.1. Remove `process_version` from the user-facing struct (hardcode "11.0"
in the writer). Document removed fields in a DN entry for future sessions. The XMP reader should
still silently parse these fields from existing sidecars (forward-compat).

---

## Theme P — Trigger count and characterization errors [MEDIUM]

**Agents**: pr-review-toolkit:comment-analyzer

Two count errors in the plan:
1. Scope justification says "8 TDs whose binding triggers have NOT fired" but lists 9 items:
   TD-002, TD-006, TD-007, TD-012, TD-013, TD-015, TD-017, TD-018, TD-019.
2. "7 TDs whose binding triggers have fired" — TD-001, TD-004, TD-005 triggers have NOT
   technically fired by their own definitions (no Release tag cut; no external PR). These are
   **proactive closures** (closing before the trigger fires), not fired-trigger closures. The
   distinction matters for audit purposes.

**Remediation**: Fix deferred count to "9 TDs". For TD-001/TD-004/TD-005 characterization,
change to "7 TDs addressed this session (4 with fired triggers: TD-009, TD-011, TD-014, TD-020;
3 proactive closures: TD-001, TD-004, TD-005)."

---

## Theme Q — `bilinear_resize` should be demoted to `fn` after TD-020 [MEDIUM]

**Agents**: general-purpose (consistency)

Plan line 229: "`bilinear_resize` remains `pub(crate)` for NIMA preprocessing."

After D2e adds `clip_preprocess` to `mobileclip.rs`, `bilinear_resize` in `nima.rs` has exactly
one caller: `nima.rs::Nima::score()` (internal, same file). A function with only file-internal
callers does not need `pub(crate)`. The current `pub(crate)` was added with comment
"so mobileclip.rs can reuse for CLIP preprocessing (TD-020)" — that reason is gone after D2e.

**Remediation**: D2e commit should demote `bilinear_resize` from `pub(crate)` to `fn` and
remove the TD-020 comment from `nima.rs:255`. Update plan line 229 to say "demoted to `fn`."

---

## Theme R — Design decisions section has wrong threshold for TD-020 [MEDIUM]

**Agents**: pr-review-toolkit:comment-analyzer

Plan line 140-141 (Design decisions for D2): "the cosine_sim golden test band should tighten
from ≥0.98 (bilinear) to ≥0.99 (bicubic, closer to Python OpenCLIP reference)."

The actual test in `crates/photohelper-raw/tests/integration_clip.rs` uses `>= 0.80` (not 0.98).
The D2e implementation section (plan:232) correctly says "tighten from >= 0.80 to >= 0.90."
The design decisions section contradicts both the source code and the implementation spec.

**Remediation**: Fix plan line 140-141 to say "tighten from ≥0.80 (bilinear) to ≥0.90 (bicubic)."

---

## Disposition summary

| Theme | Severity | Status |
|---|---|---|
| A — XMP path convention (CR3.xmp → xmp) | CRITICAL | Open |
| B — Atomic XMP write not specified | CRITICAL | Open |
| C — Conflict resolution undefined for missing timestamps | CRITICAL | Open |
| D — Phantom constant MODEL_MANIFEST_NAME_NIMA | HIGH | Open |
| E — DevelopRow missing photo_id; unnecessary disk re-derive | HIGH | Open |
| F — SidecarSettings public fields + WhiteBalance::Custom | HIGH | Open |
| G — WriteOutcome↔DevelopStats mapping undefined | HIGH | Open |
| H — D4b per-photo error handling gaps | HIGH | Open |
| I — /tmp/preview.jpg clobber in parallel CI | HIGH | Open |
| J — XMP reader malformed value behavior unspecified | HIGH | Open |
| K — Critical test gaps (7 sub-items) | HIGH | Open |
| L — D1 ordering risk | HIGH | Open |
| M — D2 single commit bundles 5 changes | MEDIUM | Open |
| N — Unused photohelper-core dep | MEDIUM | Open |
| O — Dead SidecarSettings fields | MEDIUM | Open |
| P — Trigger count errors | MEDIUM | Open |
| Q — bilinear_resize visibility after TD-020 | MEDIUM | Open |
| R — Wrong threshold in design decisions | MEDIUM | Open |

## R2 watch-list (mandatory verification items)

- [ ] R1-A: Sidecar path uses `Path::with_extension("xmp")` (not `photo.CR3.xmp`)
- [ ] R1-B: Atomic write specified (temp file + rename + fsync) in D3b
- [ ] R1-C: Conflict resolution table covers all 4 timestamp-presence cases
- [ ] R1-D: `MODEL_SLUG` (not `MODEL_MANIFEST_NAME_NIMA`) used for catalog query
- [ ] R1-E: `DevelopRow` includes `photo_id`; query SELECTs `p.id`
- [ ] R1-F: `SidecarSettings` fields private; constructor/builder specified
- [ ] R1-G: `WriteOutcome` 4-variant; counter mapping table in plan
- [ ] R1-H: `file_missing` + `derive_failed` in DevelopStats; error handling specified
- [ ] R1-I: `mktemp` used in D2a sanitize pseudocode
- [ ] R1-J: Lenient reader behavior specified for malformed values

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 17
  verified: 16
  drifted: 1
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: A-xmp-path
      line: 377
      present: yes
      retain: yes
      evidence_snippet: "path_written_as_dotxmp_extension | For photo.CR3 → sidecar at photo.CR3.xmp"
    - finding_id: B-atomic-write
      line: null
      present: yes
      retain: yes
      evidence_snippet: "pub fn write_xmp(path: &Path, settings: &SidecarSettings) -> Result<(), Error>"
    - finding_id: C-conflict-missing-timestamps
      line: 90
      present: yes
      retain: yes
      evidence_snippet: "Read `ph:LastProcessedAt` from our prior write (if present)"
    - finding_id: D-phantom-constant
      line: 438
      present: yes
      retain: yes
      evidence_snippet: "catalog.all_photos_with_cull_scores(MODEL_MANIFEST_NAME_NIMA)"
    - finding_id: E-developrow-no-photoid
      line: 398
      present: yes
      retain: yes
      evidence_snippet: "`DevelopRow`: `source_path: PathBuf, nima_score: Option<f32>`"
    - finding_id: F-public-fields
      line: 255
      present: yes
      retain: yes
      evidence_snippet: "pub process_version: Option<String>,"
    - finding_id: G-writeoutcome-mapping
      line: 331
      present: yes
      retain: yes
      evidence_snippet: "pub enum WriteOutcome { Written, ConflictPreserved, ForcedOverwrite }"
    - finding_id: H-error-handling-gaps
      line: 451
      present: yes
      retain: yes
      evidence_snippet: "- `errored` — photo failed (XMP write error, path invalid, etc.)"
    - finding_id: I-tmp-clobber
      line: 192
      present: yes
      retain: yes
      evidence_snippet: "exiftool -b -PreviewImage \"$fixture\" > /tmp/preview.jpg"
    - finding_id: J-reader-malformed
      line: 320
      present: yes
      retain: yes
      evidence_snippet: "Unknown fields are silently ignored (forward-compatibility)."
    - finding_id: K-stub-test
      line: null
      present: yes
      retain: yes
      evidence_snippet: "for name in [\"develop\", \"export\", \"run\", \"models\", \"camera\"]"
    - finding_id: L-d2-commit-bundling
      line: 186
      present: yes
      retain: yes
      evidence_snippet: "fix(session-06): D2 — TD-004/TD-005/TD-009/TD-014/TD-020 closure"
    - finding_id: M-unused-photohelper-core
      line: 247
      present: yes
      retain: yes
      evidence_snippet: "photohelper-core = { path = \"../photohelper-core\" }"
    - finding_id: N-dead-fields
      line: 265
      present: yes
      retain: yes
      evidence_snippet: "pub clarity: Option<i32>,"
    - finding_id: O-trigger-count
      line: 533
      present: yes
      retain: yes
      evidence_snippet: "8 TDs whose binding triggers have NOT fired"
    - finding_id: P-bilinear-visibility
      line: 229
      present: yes
      retain: yes
      evidence_snippet: "`bilinear_resize` remains `pub(crate)` for NIMA preprocessing"
    - finding_id: S-wrong-threshold
      line: 140
      present: drifted
      retain: yes-with-corrected-line
      evidence_snippet: "tighten from ≥0.98 (bilinear) to ≥0.99 (bicubic)"
```
