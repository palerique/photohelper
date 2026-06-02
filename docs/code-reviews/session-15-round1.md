# Session 15 — Implementation, Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6[1m] (parent); sub-agents pinned to opus"
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

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 0 | — |
| HIGH | 4 | A, B, C, J |
| MEDIUM | 6 | D, E, F, G, I, K |
| LOW | 1 | H |

Agents that fired: all 8 + 9th verifier. discard_rate: 0.00 (0 of 11 verified findings hallucinated).

---

## Theme A — `ShadowSpec.band_frac` silently ignored (5 agents)

**Severity: HIGH**

`render_to_jpeg` binds the shadow spec as `ref _shadow` (underscore prefix — compiler silences unused warning) and immediately calls `apply_shadow_gradient(&mut pixmap)` without passing the spec at all (`lib.rs:859-861`). `apply_shadow_gradient` accepts no parameter and internally hardcodes `SHADOW_BAND_FRAC` via `shadow_alpha_ramp(pixmap.height())` (`lib.rs:1042-1047`). The `ShadowSpec.band_frac` field is documented as "configurable" but is entirely inert — any caller that passes a different fraction gets silently ignored.

- [general-purpose, code-architect, type-design-analyzer, silent-failure-hunter, comment-analyzer]: HIGH — dead API parameter; API lies about configurability.

**Verified**: present=yes at lib.rs:859, lib.rs:1042.

**Remediation**: Pass `shadow.band_frac` through `apply_shadow_gradient` → `shadow_alpha_ramp`. Change signatures to accept `band_frac: f32`.

---

## Theme B — Margins computed from source dimensions, applied to resized pixmap (2 agents)

**Severity: HIGH**

In `watermark.rs:246-247`, pixel margins are computed from the **source** image dimensions (`w`, `h`) before resize:
```rust
let margin_x = (w as f32 * MARK_MARGIN_FRAC).round().max(1.0);
let margin_y = (h as f32 * MARK_MARGIN_FRAC).round().max(1.0);
```
These are stored in `MarkSpec` and later applied against the **post-resize** pixmap (`pw`, `ph`) in `composite_mark_on_pixmap`. For a 6000×4000 source resized to 2048px, the margin becomes `round(6000 × 0.046) = 276px` — 13.5% of the 2048px wide resized image instead of the intended 4.6%. This may cause spurious `MarkDoesNotFit` errors on images where marks should fit, or displace marks dramatically toward center.

- [general-purpose, code-architect]: HIGH — correctness bug in the common `--max-long-edge` use case.

**Verified**: present=yes at watermark.rs:246.

**Remediation**: Compute margins inside `composite_mark_on_pixmap` from the resized pixmap dimensions (`pw`, `ph`), not from the source. Change `MarkSpec.margin_x/margin_y` from pre-computed pixel values to fractions (store `MARK_MARGIN_FRAC`), and compute pixels at compositing time.

---

## Theme C — `MarkPlacement` is dead code in production; `composite_mark_on_pixmap` duplicates its logic (3 agents)

**Severity: HIGH**

`MarkPlacement::fit` (`lib.rs:282-390`) is a well-designed validated type with private fields, `checked_sub` overflow guards, bounds checking, and a `debug_assert!` on `height_frac`. However, ALL calls to `MarkPlacement::fit` occur inside `#[cfg(test)]` blocks (`lib.rs:1590, 1615, 1633, 1658`). The production compositing function `composite_mark_on_pixmap` (`lib.rs:1066-1139`) reimplements the same geometry (dimension calculation, margin computation, slot-based position via `checked_sub`) without routing through `MarkPlacement`. This means:
1. The `debug_assert!` on `height_frac` never fires in production.
2. Two copies of the geometry algorithm must be kept in sync.
3. The invariant ("mark fits") guaranteed by `MarkPlacement` does not protect the production path.

- [code-architect, type-design-analyzer, code-simplifier]: HIGH — validated type is dead; production code has unguarded duplicated logic.

**Verified**: present=yes at lib.rs:1066 (composite_mark_on_pixmap does not call MarkPlacement::fit) and lib.rs:1590 (MarkPlacement::fit only in tests).

**Remediation**: Generalize `MarkPlacement::fit` to accept `BadgeSizeBasis` (or add an overload for the `LongEdge` case), then rewrite `composite_mark_on_pixmap` to delegate geometry computation to `MarkPlacement::fit` and use its accessor methods (`x()`, `y()`, `w()`, `h()`) for the blit.

---

## Theme J — `unreachable!()` in production path violates CLAUDE.md no-panic rule (1 agent)

**Severity: HIGH**

`rename.rs:479` contains `unreachable!()` inside the `Some(Err(()))` match arm of `run_rename`. This macro expands to a `panic!()` and violates CLAUDE.md's "No panics / unchecked failures on production paths" policy. While the path is structurally unreachable (the early return at line 432 guards it), the production binary still carries a panic site. `clippy::panic` does not catch `unreachable!()` (it only catches `panic!()` directly), so CI passed — but the policy violation stands.

- [code-reviewer]: HIGH — CLAUDE.md violation; production panic site.

**Verified**: present=yes (drifted to line 479).

**Remediation**: Replace `unreachable!()` with a defensive non-panic fallback:
```rust
Some(Err(())) => {
    // Structurally unreachable: sidecar_copy_failed early-return at line 432 guards this arm.
    // Defensive (in case of future refactor): count as errored rather than panic.
    stats.errored.fetch_add(1, Ordering::Relaxed);
    if args.strict { cancelled.store(true, Ordering::Relaxed); }
}
```

---

## Theme D — `TempFileGuard.commit()` called before rename, defeating RAII cleanup (4 agents)

**Severity: MEDIUM**

In `watermark.rs:311` and `rename.rs:435`, `guard.commit()` is called BEFORE `fs::rename`, disabling the guard's `Drop` cleanup before the rename attempt. If rename fails, the manual `let _ = std::fs::remove_file(...)` is the only cleanup, and it silently discards any removal error. This subverts the RAII pattern: the guard was designed to auto-clean on failure, but it is pre-disarmed.

- [code-architect, type-design-analyzer, silent-failure-hunter, general-purpose]: MEDIUM — RAII contract violated; temp file leaked on rename failure + remove failure.

**Verified**: present=yes at watermark.rs:311; drifted to rename.rs:435 (cited as 434).

**Remediation**: Move `guard.commit()` to after the successful rename, inside the `else` branch. Remove the manual `remove_file` call — the guard's `Drop` handles that case.

---

## Theme E — Decision doc 0005 falsely claims shared formatter; rename uses inline format (2 agents)

**Severity: MEDIUM**

`docs/decisions/0005-rename-filename-scheme.md:28-29` states: "both subcommands route through the shared `format_nima_score_label` helper." Neither `rename.rs` nor `export.rs` calls `format_nima_score_label`. `rename.rs:136` inlines `format!("Cull-{s:05.2}")` directly.

- [general-purpose, comment-analyzer]: MEDIUM — documentation inaccuracy; drift risk if format_nima_score_label changes.

**Verified**: present=yes at decision doc:28 and rename.rs:136.

**Remediation**: Make `rename.rs` call `format_nima_score_label(s)` from `commands::util` (fulfilling the doc's claim), OR correct the doc to state inline duplication. Option A is preferred (D-Q3 mandates the shared formatter).

---

## Theme F — Dead variable `long_e` with false justification comment (2 agents)

**Severity: MEDIUM**

`watermark.rs:243` computes `let long_e = w.max(h) as f32;` and `watermark.rs:274` suppresses the unused warning with `let _ = long_e; // used indirectly via margin computation above`. The comment is false: the margin computation at lines 246-247 uses `w` and `h` directly, not `long_e`.

- [general-purpose, comment-analyzer, code-simplifier]: MEDIUM — dead variable + misleading comment.

**Verified**: present=yes at watermark.rs:274.

**Remediation**: Remove both the binding at line 243 and the suppression at line 274.

---

## Theme G — `collect_source_files` swallows all WalkDir traversal errors silently (1 agent)

**Severity: MEDIUM**

`watermark.rs:369-372` uses `.filter_map(|e| e.ok())` and `canonicalize(...).ok()?` which silently drop permission-denied subdirectories and canonicalization failures from the file list. No counter is incremented, no log is emitted. Under `--strict`, these silently-skipped files never reach the per-file pipeline, so strict mode cannot catch traversal errors.

- [silent-failure-hunter]: MEDIUM — silently incomplete traversal; `--strict` ineffective for traversal errors.

**Remediation**: Replace `.filter_map(|e| e.ok())` with a closure that logs `WalkDir` errors as `tracing::warn!`.

---

## Theme I — `RenamedFilename::build` error swallowed silently in closure (1 agent)

**Severity: MEDIUM**

`rename.rs:276-282`: when `RenamedFilename::build()` returns `Err(_)`, the error is discarded with `Err(_) => { ... }` (no log, no counter). The user never learns which files had problematic stems that triggered the fallback. The fallback to plain filename loses the Cluster/Cull prefix silently.

- [silent-failure-hunter]: MEDIUM — silent fallback; no diagnostic for stem sanitization failures.

**Remediation**: Add `tracing::warn!` with the source path and error, and log the RenameError message.

---

## Theme H — `// SAFETY:` prefix on non-unsafe code (1 agent)

**Severity: LOW**

`lib.rs:923`: `// SAFETY: MozJPEG FFI bindings may panic; catch_unwind contains any panic.` The `SAFETY:` prefix is reserved for documenting `unsafe` blocks per workspace policy. This `catch_unwind` call is safe Rust. False `SAFETY:` markers pollute `unsafe` audits.

- [comment-analyzer]: LOW — documentation convention violation.

**Remediation**: Change to `// MozJPEG FFI bindings may panic; catch_unwind contains any panic.`

---

## Theme K — Test gaps: several plan-promised tests absent (1 agent)

**Severity: HIGH** (plan-required tests are non-negotiable per quality-assurance.md)

The following tests promised by `docs/plans/session-15.md §How each deliverable is tested` are entirely absent:

1. **Portrait JPEG Orientation=6 → mark at visual top-right** — no EXIF orientation test anywhere.
2. **Malformed orientation tag → defined outcome (RT-J)** — `apply_exif_orientation` unknown-orientation path untested.
3. **Truncated JPEG → `decode_failed`** — `decode_raster` error path untested.
4. **Sentinel pixel survival test** — `load_source_image` produces correct pixel values untested.
5. **Sidecar-copy failure → NO final renamed RAW** — atomicity contract of rename.rs untested.
6. **Portrait geometry exact placement** — only landscape covered; plan requires portrait too.
7. **Square geometry test** — only error case (50×50 with 500×20 badge) covered; no success-path square test.
8. **RT-C decode-back assertion weak** — existing test checks JPEG magic bytes only; plan requires pixel decode-back.

- [pr-test-analyzer]: CRITICAL/HIGH — plan-required tests absent.

**Remediation**: Add the missing tests per plan §How each deliverable is tested. Priority order: sidecar-copy-failure atomicity, EXIF orientation=6, truncated JPEG, sentinel pixel, portrait geometry, square geometry, RT-C decode-back.

---

## Disposition summary

| Theme | Severity | Action |
|---|---|---|
| A — shadow band_frac ignored | HIGH | Fix in Round 1 remediation |
| B — margins from source dims | HIGH | Fix in Round 1 remediation |
| C — MarkPlacement dead code | HIGH | Fix in Round 1 remediation |
| J — unreachable!() in production | HIGH | Fix in Round 1 remediation |
| D — commit() before rename | MEDIUM | Fix in Round 1 remediation |
| E — false formatter claim | MEDIUM | Fix in Round 1 remediation |
| F — dead long_e variable | MEDIUM | Fix in Round 1 remediation |
| G — WalkDir errors swallowed | MEDIUM | Fix in Round 1 remediation |
| I — build() error swallowed | MEDIUM | Fix in Round 1 remediation |
| K — test gaps | HIGH | Add missing tests |
| H — SAFETY: comment misuse | LOW | Fix in Round 1 remediation |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 11
  verified: 9
  drifted: 2
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: A-shadow-band-frac
      file: crates/photohelper-export/src/lib.rs
      line: 859
      present: yes
      retain: yes
      reason: "_shadow bound with underscore; apply_shadow_gradient called without ShadowSpec"
      evidence_snippet: "if let Some(ref _shadow) = opts.shadow {\n        apply_shadow_gradient(&mut pixmap);\n    }"
    - finding_id: A-apply-shadow-hardcodes
      file: crates/photohelper-export/src/lib.rs
      line: 1042
      present: yes
      retain: yes
      reason: "shadow_alpha_ramp(pixmap.height()) hardcodes constant; no band_frac param"
      evidence_snippet: "fn apply_shadow_gradient(pixmap: &mut tiny_skia::Pixmap) {\n    let ph = pixmap.height() as usize;\n    let ramp = shadow_alpha_ramp(pixmap.height());"
    - finding_id: B-margins-from-source
      file: crates/photohelper-cli/src/commands/watermark.rs
      line: 246
      present: yes
      retain: yes
      reason: "w and h are source image dimensions, not post-resize"
      evidence_snippet: "let margin_x = (w as f32 * MARK_MARGIN_FRAC).round().max(1.0);\n        let margin_y = (h as f32 * MARK_MARGIN_FRAC).round().max(1.0);"
    - finding_id: C-composite-mark-no-placement
      file: crates/photohelper-export/src/lib.rs
      line: 1066
      present: yes
      retain: yes
      reason: "composite_mark_on_pixmap reimplements geometry inline without MarkPlacement::fit"
      evidence_snippet: "fn composite_mark_on_pixmap(\n    pixmap: &mut tiny_skia::Pixmap,\n    mark: &MarkSpec,\n    pw: u32,\n    ph: u32,"
    - finding_id: C-markplacement-tests-only
      file: crates/photohelper-export/src/lib.rs
      line: 1590
      present: yes
      retain: yes
      reason: "All MarkPlacement::fit calls inside #[cfg(test)]"
      evidence_snippet: "let p = MarkPlacement::fit(\n            (1000, 600),\n            (200, 100),\n            MARK1_HEIGHT_FRAC,"
    - finding_id: D-commit-before-rename-watermark
      file: crates/photohelper-cli/src/commands/watermark.rs
      line: 311
      present: yes
      retain: yes
      reason: "guard.commit() before fs::rename; RAII disarmed before rename attempt"
      evidence_snippet: "guard.commit();\n        if let Err(e) = std::fs::rename(&tmp_path, &out_path) {"
    - finding_id: D-commit-before-rename-rename
      file: crates/photohelper-cli/src/commands/rename.rs
      line: 434
      present: drifted
      retain: yes-with-corrected-line
      reason: "Pattern at line 435 (raw_guard.commit() before rename at line 437)"
      evidence_snippet: "raw_guard.commit();\n\n        if let Err(e) = std::fs::rename(&raw_tmp, final_raw_path) {"
    - finding_id: E-format-nima-false-claim
      file: docs/decisions/0005-rename-filename-scheme.md
      line: 28
      present: yes
      retain: yes
      reason: "Doc claims shared formatter; grep confirms rename.rs does not call format_nima_score_label"
      evidence_snippet: "**Shared formatter**: both subcommands route through the shared\n   `format_nima_score_label` helper"
    - finding_id: E-rename-uses-inline-format
      file: crates/photohelper-cli/src/commands/rename.rs
      line: 136
      present: yes
      retain: yes
      reason: "rename.rs uses format!(\"Cull-{s:05.2}\") inline, not the shared helper"
      evidence_snippet: "Some(s) if s.is_finite() && !s.is_nan() => format!(\"Cull-{s:05.2}\"),"
    - finding_id: F-long-e-unused
      file: crates/photohelper-cli/src/commands/watermark.rs
      line: 274
      present: yes
      retain: yes
      reason: "let _ = long_e with false justification comment; long_e genuinely unused"
      evidence_snippet: "let _ = long_e; // used indirectly via margin computation above"
    - finding_id: J-unreachable-or-fixed
      file: crates/photohelper-cli/src/commands/rename.rs
      line: 475
      present: drifted
      retain: yes-with-corrected-line
      reason: "unreachable!() present at line 479; CLAUDE.md policy violation"
      evidence_snippet: "Some(Err(())) => {\n                // Already handled above (sidecar_copy_failed path returns early).\n                unreachable!();\n            }"
```
