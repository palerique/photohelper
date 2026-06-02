# Session 15 — Plan (`watermark-and-rename`), Review Round 3

```yaml
session_config:
  schema_version: 1
  model_claimed: "Gemini 3.5 Flash (High)"
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
  agents_requested: ["general-purpose", "feature-dev:code-architect", "feature-dev:code-reviewer", "pr-review-toolkit:type-design-analyzer", "pr-review-toolkit:silent-failure-hunter", "pr-review-toolkit:comment-analyzer", "pr-review-toolkit:pr-test-analyzer", "pr-review-toolkit:code-simplifier"]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

Scope: `docs/plans/session-15.md` v3 (post-Round-2 remediation). Full 8-agent suite + orchestrator verification. **Round 2's two CRITICALs (RT-A, RT-B) verified genuinely closed by 3 independent lenses.** Severity trajectory across rounds: **7C → 2C → 0C** — this round converges. Every lens independently judged the plan implementation-ready (pending the plan-text one-liners below) and advised against a Round 4.

## Triage summary

<table>
<tr><th>Severity</th><th>Count</th></tr>
<tr><td>CRITICAL</td><td>0</td></tr>
<tr><td>HIGH</td><td>2</td></tr>
<tr><td>MEDIUM</td><td>4</td></tr>
<tr><td>LOW</td><td>5</td></tr>
</table>

All findings are **plan-text patches**; none touches the architecture or the four user decisions. The two HIGH items are a recurrence-class type definition (`MarkSlot`) and one missing integration test row for the session's most novel exit rule.

---

## R3-A — `MarkSlot` named once, never defined (RT-H defect class recurred); discriminant untested (HIGH)

- [type-design-analyzer (HIGH), pr-test-analyzer (MEDIUM)]: v3 fixed RT-H by adding `GeometryError::MarkDoesNotFit { which: MarkSlot, … }` (`session-15.md:258`), but `MarkSlot` itself appears exactly once and is never defined — structurally identical to the Round-2 `GeometryError`-undefined finding, one level down. The geometry test row asserts `MarkDoesNotFit{…}` eliding `which`, so a builder reporting the wrong slot (mark1 vs mark2) passes. 'HIGH'

**Remediation**: Define `enum MarkSlot { Mark1, Mark2 }` (derive `Debug, Clone, Copy, PartialEq, Eq`) in D1b alongside `GeometryError`; the geometry test asserts the full variant incl. `which == MarkSlot::Mark2` (and a `Mark1` case).

## R3-B — `mark_doesnt_fit → EX_PARTIAL_FAIL` (the novel partial-fail-without-`--strict` rule) is never exercised end-to-end (HIGH)

- [pr-test-analyzer]: D-Q5's contract (a non-fitting mark → no JPEG, `mark_doesnt_fit++`, `EX_PARTIAL_FAIL` even without `--strict`; fatal under `--strict`) is the session's single most unusual exit rule, but only the **pure** `MarkPlacement::fit` `Err` is tested (`session-15.md:126`). No integration row drives the wiring; a pure-fn test passes even if D2c swallows the `Err` as `errored` or returns exit 0. 'HIGH'

**Remediation**: Add a D2d integration row: a valid raster + a `--mark1` that cannot fit (tiny target) → exit **2**, `stderr` contains `mark-doesnt-fit: 1` + `written: 0`, output JPEG absent; re-run `--strict` → exit **1** (calibrate against `export_strict_cancellation_on_missing_file`, `cli.rs:1944-1946`).

## R3-C — RT-B's `(ingested_at, photo_id)` Rust-sort key is non-implementable; only the SQL `, p.id` branch is sound (MEDIUM)

- [silent-failure-hunter]: v3 offers "sort rows by `(ingested_at, photo_id)` (or add `, p.id` to the query)" (`session-15.md:297`). The first option cannot compile: `PhotoId` derives `Clone, Copy, PartialEq, Eq, Hash` — **no `Ord`** (`model.rs:45-46`, verified); and `ingested_at_unix_seconds` is **not projected** into `DevelopRow` (the SELECT is `p.id, p.source_path, cs.aesthetic_score, dc.cluster_id`; `ingested_at` is only in `ORDER BY` — `catalog.rs:866-881`, verified). An implementer taking that branch stalls or drops the tiebreaker, reopening the RT-B non-determinism. 'MEDIUM'

**Remediation**: Specify only "add `, p.id` as a secondary `ORDER BY` key to `all_photos_with_cull_scores` (total order `(ingested_at_unix_seconds, id)`); do NOT Rust-sort on `photo_id` (no `Ord`) or `ingested_at` (not projected)." Note the **shared-query side effect**: callers `export.rs:259` + `develop.rs:236` re-order too — benign (both build deterministic upfront maps) but state it for the D1.0 regression reviewer.

## R3-D — `render_to_jpeg`/`load_source_image` seam: `RgbImage` has no `into_pixels`, so the (rgb,w,h) handoff risks a per-image ~24MP copy (MEDIUM)

- [feature-dev:code-architect (LOW), type-design-analyzer (MEDIUM)]: RT-D made `load_source_image -> RgbImage` (for the `len==w*h*3` guard) but `render_to_jpeg`/`resize_rgb` take a `(rgb, w, h)` triple; `RgbImage` exposes only `pixels() -> &[u8]` (no `into_pixels`/`into_parts`, `model.rs:693-705`), so the hot-path handoff could land a `.pixels().to_vec()` full-buffer copy, undermining throughput, with no perf test to catch it. (RT-D's `RgbImage` return reverted Round-1's explicit `(Vec,w,h)` decision without re-reconciling the consumer.) 'MEDIUM'

**Remediation**: Pin `render_to_jpeg(rgb: &[u8], w, h, &opts)` (a **borrow**) + the call form `render_to_jpeg(img.pixels(), img.width().get(), img.height().get(), &opts)` — zero-copy, no core-crate change; `export_photo`'s re-point passes `&rgb_pixels`.

## R3-E — `HeightFrac` newtype is over-built (only vetted-const producers; `.max(1)` already floors) (MEDIUM)

- [code-simplifier]: RT-M(T4) and the simplifier disagree by design; the simplifier's case is stronger here. The only producers of the height fraction are the compile-time-valid consts `MARK1_HEIGHT_FRAC=0.14`/`MARK2_HEIGHT_FRAC=0.13` (no user-supplied entry point, unlike `Scale` which backs `--badge scale=`), and `MarkPlacement::fit` already applies `round(H*f).max(1)`. v3 took BOTH halves of the Round-2 "newtype OR consts+floor" disjunction. 'MEDIUM'

**Remediation**: Drop the `HeightFrac` newtype; `BadgeSizeBasis::Height` carries the const `f32`, with a one-line `debug_assert!(f.is_finite() && f > 0.0 && f <= 1.0)` at `fit` entry + the existing `.max(1)` floor. Less code, same safety; record the divergence from `Scale`/`Rating` (those clamp user input; this has no untrusted producer).

## R3-F — happy-path `watermark` `written: N` is never asserted (MEDIUM)

- [pr-test-analyzer]: every `watermark` integration row asserts a failure/skip counter or the empty-source summary; none asserts that N valid mixed inputs → `written: N`, exit 0, JPEGs present. The idempotency row asserts a *stable* count (satisfied by 0==0), so a walker/dispatch regression writing zero files would pass. 'MEDIUM'

**Remediation**: Add a D2d positive row: `--source` with a JPEG + PNG (+ CR3 if LFS present) → exit 0, `written: 2` (or 3), output files exist + non-empty (calibrate against `export_runs_successfully_for_ingested_photos`, `cli.rs:1829-1837`).

## R3-G — session-14 raw severity breakdown drifts from the cited artifact (LOW)

- [comment-analyzer]: v3 says "2 CRITICAL + 9 HIGH + 4 MEDIUM + 2 LOW raw" (`session-15.md:189`), but the artifact's in-body severity tags are **2C + 8H + 3M + 2L = 15** (verified by grep), matching its `verified: 15`. The headline "15 verified findings / discard_rate 0.16" is correct; only the parenthetical breakdown drifts (the session-14 artifact's own triage table says 17, an internal inconsistency in *that* artifact). 'LOW'

**Remediation**: Drop the raw breakdown; keep "15 verified findings (3 hallucinations discarded, discard_rate 0.16)" to avoid propagating session-14's internal table-vs-tags drift.

## R3-H — TD numbering hint is misleading; the ledger is non-contiguous (LOW)

- [comment-analyzer]: v3 says "ledger tail ≈ TD-040 — confirm, don't assume" (`session-15.md:341`). Verified: present ids are TD-001..023, 025, 026, 040 — so **TD-024 and TD-027..TD-039 are free, and TD-040 is taken** (session 11). An implementer filing "TD-040" collides; filing "TD-041" skips 15 free slots. 'LOW'

**Remediation**: "File the new TD at the lowest free id (**TD-024**); the ledger is non-contiguous (TD-024 + TD-027..039 free, TD-040 taken) — confirm against `TECH-DEBT.md` at filing."

## R3-I — `test_watermark_position_calculation` must be migrated, not just added, to the 2-axis signature (LOW)

- [code-simplifier]: v3 adds `test_watermark_position_calculation` (`lib.rs:792-798`) to the D1.0 regression set, but D1d makes `calculate_watermark_position` 2-axis, so that test's 6-arg call won't compile as-is. 'LOW'

**Remediation**: Say "migrate `test_watermark_position_calculation` to the 2-axis signature" (not merely "add").

## R3-J — minor test/citation polish (LOW)

- [pr-test-analyzer]: the shadow test should pin an EXACT mid-band value (e.g. `t=0.5`, base 200 → 100) rather than "partially darkened" (slightly soft); the score formatter path is `commands/util.rs:15` (plan says `util.rs:15`). 'LOW'

**Remediation**: Tighten the shadow assertion to a concrete value; fix the path citation.

## R3-K — `RenameStats.errored`'s named source (containment rejection) is unreachable-by-construction and untested (LOW)

- [general-purpose]: v3 names `errored`'s "one distinct source" as a destination-containment rejection (`session-15.md:113`), but the sanitizer guarantees single-component names, so a lexical-containment rejection can't fire; the counter is then a dead, untested catch-all. 'LOW'

**Remediation**: Keep `errored` as a **defensive catch-all** (it also covers an unexpected `fs::copy` IO error not classified as `sidecar_copy_failed`/`file_missing`) and reword `:113` from "its one distinct source is" → "a defensive catch-all (e.g. an unclassified copy IO error; the containment rejection is unreachable while the sanitizer guarantees single-component names)." No new dead counter.

---

## Disposition summary

<table>
<tr><th>Theme</th><th>Severity</th><th>Action</th></tr>
<tr><td>R3-A MarkSlot undefined</td><td>HIGH</td><td>Define enum + assert `which`</td></tr>
<tr><td>R3-B mark_doesnt_fit exit untested</td><td>HIGH</td><td>D2d integration row (exit 2 / 1)</td></tr>
<tr><td>R3-C non-compilable sort key</td><td>MEDIUM</td><td>SQL `, p.id` only; note shared-query effect</td></tr>
<tr><td>R3-D RgbImage→(rgb,w,h) copy</td><td>MEDIUM</td><td>`render_to_jpeg(rgb: &[u8], …)` borrow</td></tr>
<tr><td>R3-E HeightFrac over-built</td><td>MEDIUM</td><td>Drop newtype; debug_assert + .max(1)</td></tr>
<tr><td>R3-F written:N untested</td><td>MEDIUM</td><td>D2d positive row</td></tr>
<tr><td>R3-G session-14 breakdown drift</td><td>LOW</td><td>Drop raw breakdown</td></tr>
<tr><td>R3-H TD numbering</td><td>LOW</td><td>Lowest free TD-024</td></tr>
<tr><td>R3-I test migration</td><td>LOW</td><td>"migrate" not "add"</td></tr>
<tr><td>R3-J test/citation polish</td><td>LOW</td><td>Exact shadow value; commands/util.rs</td></tr>
<tr><td>R3-K RenameStats.errored</td><td>LOW</td><td>Reword as defensive catch-all</td></tr>
</table>

Remediated to **v4** inline. **Round 3 converges (0 CRITICAL); per `docs/quality-assurance.md § Double-review protocol`, no further round is warranted** — the double-review (R1+R2) plus the CRITICAL-triggered R3 are complete, and the suite unanimously judged the plan implementation-ready after these plan-text patches.

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 4           # new Round-3 driving citations verified by the orchestrator
  verified: 4
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  note: "Round-2 CRITICALs RT-A/RT-B independently re-verified closed by 3 lenses; Round-3 findings are plan-text patches."
  details:
    - {finding_id: R3-TD, file: TECH-DEBT.md, line: 0, present: yes, retain: yes, reason: "present ids TD-001..023,025,026,040 -> lowest free is TD-024; TD-040 taken (ledger non-contiguous)"}
    - {finding_id: R3-S14, file: docs/code-reviews/session-14-implementation-round3.md, line: 27, present: yes, retain: yes, reason: "in-body tags 2C+8H+3M+2L=15 (==verified:15); v3 parenthetical 9H+4M drifts"}
    - {finding_id: R3-ORD, file: crates/photohelper-core/src/model.rs, line: 45, present: yes, retain: yes, reason: "PhotoId derives Clone,Copy,PartialEq,Eq,Hash — no Ord; Rust-sort on photo_id won't compile"}
    - {finding_id: R3-PROJ, file: crates/photohelper-catalog/src/catalog.rs, line: 866, present: yes, retain: yes, reason: "SELECT projects id/source_path/aesthetic_score/cluster_id; ingested_at only in ORDER BY (not on DevelopRow)"}
```
