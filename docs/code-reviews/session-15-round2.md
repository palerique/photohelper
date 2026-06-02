# Session 15 — Implementation, Review Round 2

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
| HIGH | 2 | R2-A, R2-B |
| MEDIUM | 4 | R2-C, R2-D, R2-E, R2-F |
| LOW | 1 | R2-H |

R1 themes verified CLOSED: A, B, C, D (partial — watermark.rs + rename.rs RAW guard), E, F, G, I, J, K.
New findings introduced by R1 remediation: R2-A, R2-B, R2-E.
Residual R1 findings: R2-C (export.rs not fixed in R1), R2-H (SAFETY: comment still present).

9th-agent verification: 7/7 present=yes. discard_rate: 0.00.

---

## Theme R2-A — `MarkPlacement::fit` receives only `margin_x`, silently ignores `margin_y` in Height path (4 agents)

**Severity: HIGH**

`composite_mark_on_pixmap` at `lib.rs:1088` calls `MarkPlacement::fit(... mark.margin_x, mark.slot)` — passing only `mark.margin_x` as the single `margin_frac` parameter. `MarkPlacement::fit` internally computes `margin_x = round(W * margin_frac)` AND `margin_y = round(H * margin_frac)` from the same fraction. `mark.margin_y` is silently discarded. The `LongEdge` path at `lib.rs:1111-1112` correctly uses both `mark.margin_x` and `mark.margin_y` independently. Current callers set `margin_x == margin_y == MARK_MARGIN_FRAC`, so there is no visible runtime bug today — but `MarkSpec` documents per-axis margin control that the Height path cannot deliver.

- [general-purpose, code-architect, type-design-analyzer, comment-analyzer]: HIGH — API contract violation introduced by R1-Theme-C fix.

**Verified**: present=yes at lib.rs:1088.

**Remediation**: Change `MarkPlacement::fit` to accept `margin_x_frac: f32, margin_y_frac: f32` as separate parameters, computing `margin_x = round(W * margin_x_frac)` and `margin_y = round(H * margin_y_frac)` independently. Update the call site in `composite_mark_on_pixmap` to pass `mark.margin_x, mark.margin_y`. Update all test call sites accordingly.

---

## Theme R2-B — `xmp_guard` committed and dropped before XMP rename; stale comment (3 agents)

**Severity: HIGH**

Phase 2 of the rename pipeline (rename.rs:403-430) creates `xmp_guard` inside the `if src_xmp.exists() { ... }` block, commits it at line 415 on copy-success, and drops it when the block ends (scope ~line 427-430). By the time Phase 3 attempts `fs::rename(&xmp_tmp, &final_xmp_path)` at line 461, `xmp_guard` no longer exists. The comment at line 470 ("xmp_guard cleanup handled by Drop") is factually wrong — the guard has already been committed and dropped. When the XMP rename fails, `xmp_tmp` is leaked on disk.

- [general-purpose, code-reviewer, silent-failure-hunter]: HIGH — temp file leak on XMP rename failure; misleading comment.

**Verified**: present=yes at rename.rs:415 (early commit), rename.rs:470 (stale comment).

**Remediation**: Move `xmp_guard.commit()` to AFTER the XMP rename succeeds (inside the `else` branch at line 475). This requires the guard's lifetime to extend past the `if` block — either by hoisting it above the `sidecar_result` computation or by using a different ownership pattern.

---

## Theme R2-C — `export.rs` `guard.commit()` before rename (incomplete R1 fix) (2 agents)

**Severity: MEDIUM**

R1 Theme D fixed `guard.commit()` ordering in `watermark.rs` and `rename.rs` (RAW guard), but `export.rs:425-427` retains the pre-remediation pattern: `guard.commit()` is called before `fs::rename`. If the rename fails, the guard is already disarmed and the manual `let _ = std::fs::remove_file` handles cleanup (discarding the remove error silently).

- [code-architect, general-purpose]: MEDIUM — R1 fix was incomplete; same RAII-subversion issue persists in export.rs.

**Verified**: present=yes at export.rs:425-427.

**Remediation**: Move `guard.commit()` inside the success branch (`else { guard.commit(); stats.written... }`), remove the manual `remove_file` call.

---

## Theme R2-D — `shadow_alpha_ramp` accepts any `f32` band_frac; negative/large values cause pathological behavior (1 agent)

**Severity: MEDIUM**

`shadow_alpha_ramp(image_h, band_frac)` at `lib.rs:812` has no validation. For `band_frac ≤ 0`, `(image_h as f32 * band_frac).round() as usize` is 0 (return empty, benign) or wraps to `usize::MAX` (OOM). For `band_frac > 1.0`, `band_h > image_h` causing `ph - ramp.len()` underflow in `apply_shadow_gradient`. The function is `pub`, so external callers can trigger this.

- [type-design-analyzer]: MEDIUM — public function with unbounded float parameter; pathological on invalid inputs.

**Verified**: present=yes at lib.rs:812.

**Remediation**: Add an early-return guard: `if !band_frac.is_finite() || band_frac <= 0.0 || band_frac > 1.0 { return vec![]; }`. Also add `debug_assert!` in `MarkPlacement::fit` for `margin_frac` to match the existing `height_frac` guard.

---

## Theme R2-E — `renamed` counter not incremented when XMP rename fails but RAW was renamed (1 agent)

**Severity: MEDIUM**

When XMP rename fails at `rename.rs:461`, the RAW file has already been successfully renamed (lines 442, 456). The code increments `sidecar_copy_failed` but NOT `renamed`. The `None` arm and `Some(Ok(()))` arm both increment `renamed`; only the XMP-rename-failure path does not. The summary line shows `renamed: N` which undercounts the actual RAWs placed in the output directory.

- [silent-failure-hunter]: MEDIUM — misleading summary; user cannot reconcile file count with summary.

**Verified**: present=yes at rename.rs:469.

**Remediation**: Add `stats.renamed.fetch_add(1, Ordering::Relaxed)` in the XMP rename failure arm (after `sidecar_copy_failed`), since the RAW was physically placed at its final path.

---

## Theme R2-F — LongEdge path in `composite_mark_on_pixmap` duplicates `MarkPlacement::fit` position logic (1 agent)

**Severity: MEDIUM** (maintainability; no correctness impact)

The `BadgeSizeBasis::LongEdge` arm in `composite_mark_on_pixmap` (lib.rs:1101-1151) replicates ~35 lines of slot-position computation (slot match, `checked_sub`, bounds check) that `MarkPlacement::fit` already encapsulates. This duplication means a future bug fix to position logic must be applied twice. The LongEdge path is only used by `export_photo` (legacy), not by the primary `watermark` subcommand.

- [code-simplifier]: MEDIUM — maintainability debt introduced by partial C-fix in R1.

**Remediation**: File as TD-028 with binding trigger "next session that modifies composite_mark_on_pixmap". The recommended fix is to add a `MarkPlacement::fit_with_dims(target, mark_w, mark_h, margin_x_frac, margin_y_frac, slot)` factory that takes pre-computed mark dims, then rewrite both paths to call it.

---

## Theme R2-H — `// SAFETY:` comment on non-unsafe code (residual from R1) (3 agents)

**Severity: LOW**

`lib.rs:923` still has `// SAFETY: MozJPEG FFI bindings may panic; catch_unwind contains any panic.` The R1 disposition said "Fix in Round 1 remediation" but it was not applied.

**Verified**: present=yes at lib.rs:923.

**Remediation**: Remove `SAFETY: ` prefix → `// MozJPEG FFI bindings may panic; catch_unwind contains any panic.`

---

## R1 Watch-list verification (all closed)

| R1 Theme | Status |
|---|---|
| A — shadow band_frac | CLOSED |
| B — margins from source dims | CLOSED |
| C — MarkPlacement dead code | CLOSED (introduced R2-A) |
| D — commit() before rename (watermark+rename RAW) | CLOSED (export residual → R2-C) |
| E — false formatter claim | CLOSED |
| F — dead long_e | CLOSED |
| G — WalkDir errors swallowed | CLOSED |
| H — SAFETY: comment | NOT CLOSED → R2-H |
| I — RenamedFilename error swallowed | CLOSED |
| J — unreachable!() | CLOSED |
| K — missing tests | CLOSED |

---

## Disposition summary

| Theme | Severity | Action |
|---|---|---|
| R2-A — margin_y ignored | HIGH | Fix inline |
| R2-B — xmp_guard lifetime | HIGH | Fix inline |
| R2-C — export.rs commit order | MEDIUM | Fix inline |
| R2-D — shadow_alpha_ramp validation | MEDIUM | Fix inline |
| R2-E — renamed counter | MEDIUM | Fix inline |
| R2-F — LongEdge duplication | MEDIUM | File TD-028 |
| R2-H — SAFETY: comment | LOW | Fix inline |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 7
  verified: 7
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: R2A-margin-y-ignored
      file: crates/photohelper-export/src/lib.rs
      line: 1088
      present: yes
      retain: yes
      reason: "Only mark.margin_x passed to MarkPlacement::fit; mark.margin_y silently ignored"
      evidence_snippet: "mark.margin_x, // margin_x is a fraction (0..1)"
    - finding_id: R2B-xmp-guard-committed-early
      file: crates/photohelper-cli/src/commands/rename.rs
      line: 415
      present: yes
      retain: yes
      reason: "xmp_guard.commit() inside Phase-2 if-block, disarms guard before XMP rename in Phase 3"
      evidence_snippet: "xmp_guard.commit();"
    - finding_id: R2B-xmp-guard-comment-false
      file: crates/photohelper-cli/src/commands/rename.rs
      line: 470
      present: yes
      retain: yes
      reason: "Comment claims Drop handles cleanup but guard was already committed and dropped"
      evidence_snippet: "// xmp_guard cleanup handled by Drop."
    - finding_id: R2C-export-commit-before-rename
      file: crates/photohelper-cli/src/commands/export.rs
      line: 425
      present: yes
      retain: yes
      reason: "guard.commit() before fs::rename; disarms RAII before rename attempt"
      evidence_snippet: "guard.commit();"
    - finding_id: R2D-shadow-no-validation
      file: crates/photohelper-export/src/lib.rs
      line: 812
      present: yes
      retain: yes
      reason: "band_frac: f32 accepted with no finite/range guard; negative/large values cause pathological behavior"
      evidence_snippet: "pub fn shadow_alpha_ramp(image_h: u32, band_frac: f32) -> Vec<u8> {"
    - finding_id: R2E-renamed-counter-missing
      file: crates/photohelper-cli/src/commands/rename.rs
      line: 469
      present: yes
      retain: yes
      reason: "sidecar_copy_failed incremented but renamed NOT incremented despite successful RAW rename"
      evidence_snippet: "stats.sidecar_copy_failed.fetch_add(1, Ordering::Relaxed);"
    - finding_id: R2H-safety-comment
      file: crates/photohelper-export/src/lib.rs
      line: 923
      present: yes
      retain: yes
      reason: "SAFETY: prefix on non-unsafe catch_unwind code; violates Rust SAFETY comment convention"
      evidence_snippet: "// SAFETY: MozJPEG FFI bindings may panic; catch_unwind contains any panic."
```
