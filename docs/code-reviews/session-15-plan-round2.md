# Session 15 — Plan (`watermark-and-rename`), Review Round 2

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

Scope: `docs/plans/session-15.md` v2 (post-Round-1 remediation) vs the Round-1 artifact + source. Full 8-agent suite + 9th-agent verification. **All 19 Round-1 themes verified correctly closed.** Round 2 found regressions the v2 edits introduced — most notably, v2 *hardened* the Round-1 "`canonicalize_within`-style" hedge into a literal call that cannot run on a not-yet-created destination.

## Triage summary

<table>
<tr><th>Severity</th><th>Count</th></tr>
<tr><td>CRITICAL</td><td>2</td></tr>
<tr><td>HIGH</td><td>7</td></tr>
<tr><td>MEDIUM</td><td>6</td></tr>
<tr><td>LOW</td><td>3</td></tr>
</table>

All findings are **plan-text patches** (no code exists yet); none changes the architecture or the four user decisions. The comment-analyzer (prose) and code-simplifier (over-build) lenses came back essentially clean — the residue concentrates in the rename path-safety spec and the D1.0 extraction contract.

---

## RT-A — `canonicalize_within` cannot validate a not-yet-existent destination (CRITICAL · v2 regression)

- [code-reviewer, silent-failure-hunter, general-purpose]: v2's Theme-D remediation says "validate every destination with `canonicalize_within` containment" (`session-15.md:338`). But `AbsPath::canonicalize` wraps `std::fs::canonicalize` (`model.rs:236-256`), which **errors on non-existent paths** (test at `model.rs:1037-1047`); `canonicalize_within` = `canonicalize(path)?` then `starts_with(root)` (`model.rs:264`). A rename/watermark *destination* does not exist yet, so every call returns `Io(NotFound)` before the containment check — the guard fails every legitimate row (or gets coded around, reopening the escape). Round 1 said "-style" (a hedge); v2 hardened it into the infeasible literal. 'CRITICAL'

**Remediation**: Canonicalize `--output` ONCE at setup (it exists); then validate each destination by a **pure lexical** check — `output_canonical.join(&sanitized_name)` whose `.parent() == output_canonical` and whose name is a single path component. Since the stem sanitizer already rejects separators/`..`, lexical containment is sufficient AND stronger. Either add a filesystem-free `AbsPath::join_within(root, name)` helper (unit-tested with a non-existent leaf) or specify the inline lexical check. Do NOT canonicalize the leaf.

## RT-B — Sanitized-stem + `NAME_MAX` truncation can silently clobber two distinct sources (CRITICAL · v2 gap)

- [code-reviewer]: D3b layers stem sanitization + length-capping + `resolve_collisions` without pinning their order (`session-15.md:333-338`). Two distinct stems that sanitize/truncate to the same bytes collide; worse, capping the **whole composed name** to `NAME_MAX` can truncate the `_N` collision suffix back off, re-colliding after de-dup → the second copy overwrites the first, `renamed` increments twice. No `NAME_MAX`/sanitizer exists in the tree today (net-new). Compounded by RT-determinism below. 'CRITICAL'

**Remediation**: Pin the exact pipeline in D3b: (1) sanitize stem → (2) compose name → (3) truncate the **stem** (not the whole name) so prefix + `_N` suffix + extension always survive the `NAME_MAX` budget → (4) feed the final bytes to `resolve_collisions` (key on post-truncation bytes). Add an adversarial test: two distinct sources that sanitize-and-truncate identically produce two distinct outputs; assert `output_file_count == input_row_count` (no clobber). Make the catalog `ORDER BY` total (`+ , p.id`) so suffix assignment is deterministic (currently `ORDER BY p.ingested_at_unix_seconds` has no tie-breaker — `catalog.rs:881`).

## RT-C — `render_to_jpeg` extraction must reproduce export's THREE pixel paths or export output byte-drifts under weak tests (HIGH)

- [feature-dev:code-architect]: D1.0 describes `render_to_jpeg` as one pipeline (`session-15.md:255`), but `export_photo` has three: a **fast-path bypass** that encodes directly without a pixmap round-trip when `long_edge.is_none() && watermarks.is_empty()` (`lib.rs:222-232`), a resize branch (`lib.rs:256-284`), and a no-resize-watermarked branch (`lib.rs:285-296`). If `render_to_jpeg` always builds a pixmap + demultiplies, export's bypass output can byte-drift — and the existing export integration test asserts only `exists()+len>0` (`cli.rs:1892`), so it passes while bytes change. 'HIGH'

**Remediation**: D1.0 specifies `render_to_jpeg` preserves the bypass condition (direct `compress_jpeg` when no resize/shadow/marks). Add a regression assertion stronger than `exists()+len>0` for the bypass case (decode output, assert dims + a sample pixel vs pre-refactor).

## RT-D — Channel-count guard dropped; `Srgb8` has no `colors==3` assert and the bare `(Vec,w,h)` return discards `RgbImage::new`'s check (HIGH)

- [feature-dev:code-architect, silent-failure-hunter]: the FFI stores `channels: colors as u8` with **no** `colors==3` assertion (`ffi.rs:758-764`); only `RgbImage::new`'s `len==w*h*3` (`model.rs:670-689`) catches a non-3-channel decode today. D1a returns a bare `(Vec<u8>,w,h)` and the D-Q6 sanity guard checks **dims only** (`session-15.md:279`), so a 4-channel/monochrome decode mis-strides into garbage silently. 'HIGH'

**Remediation**: D1a's loader asserts `channels==3` (e.g. route through `RgbImage::new` or check `len==w*h*3`), returning `decode_failed` otherwise. Add a test that a non-3-channel decode is counted, not rendered. (See RT-K for the residual color-corruption acknowledgement.)

## RT-E — `BadgeSizeBasis` margin must be per-axis; export's single long-edge padding must be preserved (HIGH)

- [feature-dev:code-architect]: `calculate_watermark_position` takes a **single scalar `padding`** (`lib.rs:349-364`), and export computes `padding = (long_edge*0.015).round().max(8.0)` (`lib.rs:500`); the watermark feature needs per-axis `margin_x=round(W*0.046)`, `margin_y=round(H*0.046)`. The unified placement signature must carry `(margin_x, margin_y)`; export's re-point must pass equal values (its existing formula) to avoid sub-pixel placement drift. 'HIGH'

**Remediation**: D1d specifies a 2-axis margin parameter; export passes `margin_x==margin_y==(long_edge*0.015).round().max(8.0)`. Add `test_watermark_position_calculation` (`lib.rs:792-798`) to the D1.0 regression set.

## RT-F — Exit-code prose conflates `skipped_*` with failure counters (HIGH)

- [general-purpose, silent-failure-hunter]: `session-15.md:149-150` ("`skipped_*` never fail unless `--strict` AND the skip is a decode/fit/copy failure") is a category error — a decode/fit/copy failure is never a `skipped_*` (those are `skipped_unsupported`/`skipped_existing`). Read literally, an implementer could wire `decode_failed`/`mark_doesnt_fit` to exit 0 under non-strict, violating D-Q5/D-Q2. 'HIGH'

**Remediation**: Reword: "`skipped_unsupported`/`skipped_existing` never contribute to the exit code, even under `--strict`. The failure counters (`decode_failed`/`mark_doesnt_fit`/`sidecar_copy_failed`/`file_missing`/`errored`) drive `EX_PARTIAL_FAIL`; under `--strict` the first such failure is fatal (`EX_STRICT_FAIL`)."

## RT-G — `--output` overlap via a symlink inside `--source` defeats the lexical prune → re-ingest / non-idempotent (HIGH)

- [code-reviewer]: the overlap defense (reject `--output` nested in `--source` + walker prune, `session-15.md:80-82,316`) is lexical; the precedent `run.rs:140` uses `canonical_output.starts_with(&canonical_input)`. A symlink inside `--source` pointing into `--output` (or a cycle) is yielded by `walkdir` at its lexical path and not pruned → the second run re-processes its own outputs (idempotency invariant silently violated). 'HIGH'

**Remediation**: The walker prune compares each entry's **canonical** path (or `same_file::is_same_file` against canonical `--output`); confirm `follow_links(false)`. Add a test: a symlink in `--source` targeting `--output` does not cause re-processing.

## RT-H — `GeometryError` named once, never defined; conflated with its variant (HIGH)

- [type-design-analyzer]: `MarkPlacement::fit -> Result<_, GeometryError>` but the next clause "returns `Err(MarkDoesNotFit{…})`" (`session-15.md:285-291`); `GeometryError` appears exactly once and is otherwise undefined. Risk: implementer satisfies the prose with a stringly error, losing the `{which, mark_dims, target_dims}` fields the D-Q5 counter/log + the geometry test assert on. 'HIGH'

**Remediation**: Pin the type: `enum GeometryError { MarkDoesNotFit { which: MarkSlot, mark_dims: (u32,u32), target_dims: (u32,u32) } }` (or return `Result<_, MarkDoesNotFit>` and drop the `GeometryError` name). Carry the three context fields.

## RT-I — Test gaps for v2-introduced counters/types (HIGH)

- [pr-review-toolkit:pr-test-analyzer]: counters `errored`, `sidecar_absent`, `skipped_existing` have no asserted test row; the full **summary-line format** is untested for both subcommands (export pins its literal at `cli.rs:1779-1781`); **`RenderOptions::default()`** has no dimension/pixel guard — it rides on the weak `exists()+len>0` export tests, the exact anti-pattern Theme P condemned, so a default flip (`downscale_only`/shadow) would silently regress export; **mark2-within-shadow-band** is unasserted though the compositing-order invariant rests on it. 'HIGH'

**Remediation**: Add rows: assert `skipped-existing: N` on the idempotency re-run; `sidecar-absent: 1` + exit 0 for a no-`.xmp` row; `errored: 0` present on a happy path; full summary-line on empty-source/empty-catalog runs; a `render_to_jpeg(.., &RenderOptions::default())` decode-back asserting no upscale + no shadow band (backstops the extraction); `mark2_y >= H - shadow_band_height` + a mid-band sentinel under mark2 shows the mark color.

## RT-J — Orientation-apply failure has no taxonomy outcome → silent wrong-corner (MEDIUM)

- [silent-failure-hunter]: D1a "apply EXIF orientation" (`session-15.md:276`) has no fallible contract/counter; a malformed/unsupported orientation tag could silently no-op → mark1 at the visual bottom-right (Theme J relapse), and the aspect-only zero-distortion test passes anyway. 'MEDIUM'

**Remediation**: Pin orientation handling in `load_source_image`'s `Result`: malformed tag → `decode_failed`, or a documented default-to-Identity with `tracing::warn!`. Add a "malformed orientation tag → documented outcome" test.

## RT-K — Residual silent-wrong-COLOR on untested RAW unacknowledged (MEDIUM)

- [silent-failure-hunter]: the D-Q6 guard checks **dims only** (`session-15.md:279`); a LibRaw mis-decode with correct dims but wrong color (channel swap, garbage-plausible pixels) still ships. Hard to guard at runtime; the correct action is acknowledgement. 'MEDIUM'

**Remediation**: Add one sentence to D-Q2/D-Q6 and the new untested-RAW DN: "the sanity guard bounds geometry only; silent color corruption on untested RAW remains possible — the reason these formats stay `--allow-untested-raw`-gated and decode-unverified."

## RT-L — `RenderOptions` partially duplicates `ExportOptions` (MEDIUM)

- [code-simplifier]: `ExportOptions` (`lib.rs:128-136`) already carries `quality`/`long_edge`/`watermarks`; an unspecified `RenderOptions` risks a parallel struct + field-by-field translation layer. 'MEDIUM'

**Remediation**: Specify `RenderOptions` as a **decode-output-only** subset (the post-decode render knobs: `long_edge`, `downscale_only`, `quality`, `shadow`, the generalized mark list), explicitly excluding `output_path`/`force` (caller concerns) — OR fold the new axes onto `ExportOptions` and drop `RenderOptions`.

## RT-M — Type-design polish (MEDIUM)

- [type-design-analyzer]: (T2) `MarkPlacement` should have **private** `u32` fields + `checked_sub` in `fit` so "validated then trusted" is a type guarantee (lets the blit drop its bounds clip). (T3) `RenamedFilename` should be a `Result`-returning constructor that **owns** sanitization + composed-length cap (cap the composed name, not just the stem), like `SidecarPath::new` — not a "builder + checklist near the call site." (T4) `BadgeSizeBasis::Height(f32)` admits `NaN`/`0.0`; use a validated fraction newtype or rely on the vetted consts + the `.max(1)` floor. 'MEDIUM'

**Remediation**: Add the one-line type pins to D1b/D3b/D1d.

## RT-N — Sidecar copy: permission bits + commit ordering (MEDIUM)

- [code-reviewer]: `std::fs::copy` copies mode bits — a read-only source RAW yields a read-only temp → a `--force` re-run can `EACCES` (the sidecar crate hit this class). The "drop RAW temp on sidecar failure" guarantee only holds if **both temps are created before either rename**; the prose doesn't forbid copy-RAW→rename-RAW→copy-sidecar interleaving. 'MEDIUM'

**Remediation**: D3c numbers the sequence: (1) copy RAW→`raw.tmp` under `--output`; (2) copy sidecar→`xmp.tmp`; (3) only after both temps exist, rename both; (4) `TempFileGuard` on both until commit. State the temp-mode policy (normalize temp to writable, or `set_permissions` first on `--force`). Assert "no final RAW exists when the sidecar copy fails."

## RT-O — Misc clarifications (LOW)

- [pr-test]: negative/≥1000 cluster id must be tested on the **pure `RenamedFilename` builder** (the DB `CHECK(cluster_id>=0)` blocks seeding negatives); add `--max-long-edge < 16` rejection test; add non-PNG/unreadable mark → fatal-up-front + zero-outputs test; add the walker self-exclusion (sibling `--output`) assertion to the idempotency test.
- [general-purpose, comment-analyzer]: state the next-free DN id explicitly after the duplicate reconciliation; note DN-036's trigger *fired* this session (D1.0 touches `export_photo`) — reconcile-note should say so; `export_photo` starts at `lib.rs:178` (`:177` is the `#[tracing::instrument]` attribute — carried from R1, defensible).
- [code-simplifier]: add a one-line "why an enum not a 3-arm match" to `SourceKind` (the 3-way `--allow-untested-raw` gate); name the distinct failure `RenameStats.errored` covers (e.g. containment rejection) or drop it; the `docs/decisions/` note should record that `develop.rs`'s collision key uses NFC+lowercase (`develop.rs:240-264`) while the shared `resolve_collisions` is lowercase-only (`export.rs:300-303`) — intentional, `develop` is out of scope.
- [silent-failure]: caution in D3b that `rename` must NOT reuse `run_export`'s per-photo body wholesale (it calls `read_xmp` at `export.rs:374-401`, which would drag in the session-14 sidecar reader, violating D-Q7).

---

## Disposition summary

<table>
<tr><th>Theme</th><th>Severity</th><th>Action</th></tr>
<tr><td>RT-A canonicalize_within infeasible</td><td>CRITICAL</td><td>Lexical containment on canonical --output + single-component name</td></tr>
<tr><td>RT-B stem-clobber/NAME_MAX</td><td>CRITICAL</td><td>Pin pipeline order; total ORDER BY; count==rows test</td></tr>
<tr><td>RT-C 3-branch render_to_jpeg</td><td>HIGH</td><td>Preserve fast-path bypass + decode-output regression test</td></tr>
<tr><td>RT-D channel-count guard</td><td>HIGH</td><td>Assert channels==3 (reuse RgbImage::new)</td></tr>
<tr><td>RT-E per-axis margin</td><td>HIGH</td><td>(margin_x,margin_y); export preserves its padding</td></tr>
<tr><td>RT-F exit-code prose</td><td>HIGH</td><td>Reword skipped vs failure</td></tr>
<tr><td>RT-G symlink overlap</td><td>HIGH</td><td>Canonical per-entry prune + test</td></tr>
<tr><td>RT-H GeometryError undefined</td><td>HIGH</td><td>Define enum/variant with context fields</td></tr>
<tr><td>RT-I test gaps</td><td>HIGH</td><td>Add counter/summary/RenderOptions-default/mark2-band rows</td></tr>
<tr><td>RT-J orientation failure</td><td>MEDIUM</td><td>Taxonomy outcome + test</td></tr>
<tr><td>RT-K residual color risk</td><td>MEDIUM</td><td>Acknowledge in DN/D-Q2</td></tr>
<tr><td>RT-L RenderOptions dup</td><td>MEDIUM</td><td>Decode-output-only subset</td></tr>
<tr><td>RT-M type-design polish</td><td>MEDIUM</td><td>Private fields/constructor/validated frac</td></tr>
<tr><td>RT-N sidecar perms+ordering</td><td>MEDIUM</td><td>Numbered sequence + mode policy</td></tr>
<tr><td>RT-O misc</td><td>LOW</td><td>Plan clarifications</td></tr>
</table>

Round 2 surfaced 2 CRITICAL-class regressions (RT-A, RT-B) → per `docs/quality-assurance.md § Double-review protocol`, remediate to v3 and run **Round 3**.

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 17          # new Round-2 code citations checked by the 9th agent
  verified: 17
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  note: "All 19 Round-1 themes independently re-verified closed by the suite; Round-2 findings are v2-introduced regressions/gaps, all plan-text patches."
  details:
    - {finding_id: R2-1, file: crates/photohelper-core/src/model.rs, line: 236, present: yes, retain: yes, reason: "canonicalize wraps std::fs::canonicalize; rejects-nonexistent test @1037-1047"}
    - {finding_id: R2-2, file: crates/photohelper-core/src/model.rs, line: 264, present: yes, retain: yes, reason: "canonicalize_within = canonicalize(path)? then starts_with(root)"}
    - {finding_id: R2-3, file: crates/photohelper-cli/src/commands/ingest.rs, line: 357, present: yes, retain: yes, reason: "production caller canonicalizes the SOURCE path"}
    - {finding_id: R2-4, file: crates/photohelper-export/src/lib.rs, line: 222, present: yes, retain: yes, reason: "fast-path bypass: direct compress_jpeg, no pixmap"}
    - {finding_id: R2-5, file: crates/photohelper-export/src/lib.rs, line: 256, present: yes, retain: yes, reason: "two-branch pixmap fill distinct from fast path"}
    - {finding_id: R2-6, file: crates/photohelper-raw/src/decode.rs, line: 138, present: yes, retain: yes, reason: "ProcessOptions::Srgb8 + Linear16 exist"}
    - {finding_id: R2-7, file: crates/photohelper-raw/src/ffi.rs, line: 758, present: yes, retain: yes, reason: "channels: colors as u8, no colors==3 assert"}
    - {finding_id: R2-8, file: crates/photohelper-core/src/model.rs, line: 670, present: yes, retain: yes, reason: "RgbImage::new enforces len==w*h*3"}
    - {finding_id: R2-9, file: crates/photohelper-export/src/lib.rs, line: 349, present: yes, retain: yes, reason: "calculate_watermark_position takes single scalar padding"}
    - {finding_id: R2-10, file: crates/photohelper-export/src/lib.rs, line: 500, present: yes, retain: yes, reason: "padding = (long_edge*0.015).round().max(8.0)"}
    - {finding_id: R2-11, file: crates/photohelper-catalog/src/catalog.rs, line: 881, present: yes, retain: yes, reason: "ORDER BY ingested_at_unix_seconds, no tie-breaker"}
    - {finding_id: R2-12, file: crates/photohelper-cli/src/commands/run.rs, line: 140, present: yes, retain: yes, reason: "lexical canonical_output.starts_with(canonical_input)"}
    - {finding_id: R2-13, file: crates/photohelper-export/src/lib.rs, line: 722, present: yes, retain: yes, reason: "test_pixel_demultiplication duplicates demultiply loop w/ hand values"}
    - {finding_id: R2-14, file: crates/photohelper-cli/src/commands/export.rs, line: 320, present: yes, retain: yes, reason: "badge preload fatal with_context+? @326, bail! @329"}
    - {finding_id: R2-15, file: crates/photohelper-export/src/lib.rs, line: 65, present: yes, retain: yes, reason: "Scale newtype clamps 0.001..=100.0"}
    - {finding_id: R2-16, file: crates/photohelper-cli/src/commands/export.rs, line: 374, present: yes, retain: yes, reason: "per-photo body calls read_xmp for rating"}
    - {finding_id: R2-17, file: crates/photohelper-cli/src/commands/develop.rs, line: 240, present: yes, retain: yes, reason: "develop dedup key uses NFC+lowercase, differs from export lowercase-only"}
```
