# Session 15 — Session-end final review (post-R2 changes), Round 3

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

**Scope**: Post-R2 code changes (commits `c05526cb..HEAD`). Primary diffs:
- `crates/photohelper-export/src/lib.rs` (+277/-37): `fit_equal_margin`, `MARK_MIN_READABLE_PX`, Lanczos3 badge scaling in `blit_badge_at`, single-pass export via `render_marks`/`render_shadow` in `ExportOptions`
- `crates/photohelper-cli/src/commands/export.rs` (+55/-2): `--mark1-png`, `--mark2-png`, `--with-shadow` flags
- `crates/photohelper-cli/src/commands/watermark.rs` (+10): JXL unsupported-format warning
- `crates/photohelper-raw/build.rs` (+10/-5): `sh ./configure` for Windows/MSYS2
- `scripts/photohelper-produce.sh` (+253): new all-in-one pipeline script

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 1 | R3-A |
| HIGH | 5 | R3-B, R3-C, R3-D, R3-E, R3-G |
| MEDIUM | 3 | R3-F, R3-H, R3-I |
| LOW | 4 | R3-J, R3-K, R3-L, R3-M |

9th-agent verification: 8/8 verified, 0 hallucinated, 1 drifted (line number corrected), discard_rate: 0.00.

---

## Theme R3-A — No integration test for single-pass export path (CRITICAL)

**Severity: CRITICAL**

The `render_to_jpeg` non-trivial code path (resize + shadow + mark compositing at lines 956–968 of lib.rs) is tested by exactly one unit test — `test_render_to_jpeg_default_no_shadow_no_marks` — which explicitly takes the fast-path (`if opts.marks.is_empty() && opts.shadow.is_none()`) and never exercises resize, shadow, or mark compositing. The single-pass export path (via `--mark1-png`/`--mark2-png`/`--with-shadow` in the CLI and `render_marks`/`render_shadow` in `ExportOptions`) has zero CLI integration tests: `grep -rn "mark1_png\|mark2_png\|with_shadow\|render_marks\|render_shadow"` across `crates/photohelper-cli/tests/` returns no matches.

This means the 54%-performance optimization commit (`4dd5ef0a`) introduced a new code path that has never been automatically verified to produce output, let alone correct output.

- [pr-test-analyzer]: CRITICAL (9/10) — single-pass path has no CLI test; render_to_jpeg non-trivial path entirely untested
- [general-purpose]: CRITICAL (cross-cutting) — confirmed via grep

**Verified**: present=yes. `grep -rn "mark1_png\|mark2_png\|with_shadow"` returned zero matches in test files.

**Remediation**: Add (a) a unit test in `lib.rs` for `render_to_jpeg` with a non-empty marks list and shadow, asserting output JPEG decodes to non-empty pixels with a shadow band; (b) a CLI integration test for `export --mark1-png <png> --with-shadow` asserting `written: 1` and non-empty output file.

---

## Theme R3-B — `blit_badge_at`: Lanczos3 operates on demultiplied straight-alpha data (HIGH)

**Severity: HIGH**

`blit_badge_at` demultiplies tiny-skia's premultiplied RGBA to straight RGBA (lines 1295–1311), then passes the straight-alpha buffer to `image::imageops::resize(..., FilterType::Lanczos3)`. The `image` crate's resize documentation states it assumes premultiplied alpha for images with non-constant alpha. Filtering in straight-alpha space with Lanczos3 — which has negative lobes — produces dark-halo artifacts at semi-transparent badge edges: the Lanczos kernel averages nearby color values, but transparent pixels have RGB = 0 in straight space, pulling adjacent opaque colors toward black.

The practical impact depends on the badge PNG: hard-edged (binary alpha) logos show no artifact; anti-aliased logos (typical for high-quality watermarks) will show subtle darkening at edges. At the 15–20× downscale ratios involved (Marca-1.png is 1100×1540, Marca-2.png is 8120×1920), the effect is concentrated in the anti-aliased transition band.

The compositing loop (lines 1340–1354) correctly implements Porter-Duff straight-over-premultiplied blend (`dst = src_straight × alpha + dst_premult × (1 − alpha)`), but this correctness is undermined by the upstream resize having operated in straight space.

- [code-architect]: CRITICAL — image crate requires premultiplied input; dark-halo artifacts at semi-transparent edges
- [comment-analyzer]: confirmed blend formula correct, did not address resize-in-straight-space issue

**Verified**: present=yes. Lines 1295–1311 confirm demultiply before `image::imageops::resize`.

**Remediation**: Remove the demultiply loop (lines 1295–1311). Pass `badge.data().to_vec()` directly to `image::RgbaImage::from_raw` (tiny-skia stores premultiplied RGBA; the image crate treats it as premultiplied during Lanczos3). After resize, `resized_raw` is premultiplied; update the blend to premultiplied-over-premultiplied: `dst[di] = (resized_raw[si] as f32 + dst[di] as f32 × inv_a)` — removing the `× alpha` factor that was premultiplying an already-straight value. Update the in-code comment from "Premultiply straight source on the fly" to "Premultiplied source blended over premultiplied dst".

---

## Theme R3-C — `MarkSpec.margin_y` dead in `Height` path; `margin_x` docstring stale (HIGH)

**Severity: HIGH**

`MarkSpec` declares two margin fields:
```
pub margin_x: f32,  // "fraction of the post-resize image width (0.0–1.0)"
pub margin_y: f32,  // "fraction of the post-resize image height (0.0–1.0)"
```

In `composite_mark_on_pixmap`'s `BadgeSizeBasis::Height` branch (line 1205), only `margin_x` is used — applied to `pw.min(ph)` (the short edge), not the image width: `let margin_px = (pw.min(ph) as f32 * mark.margin_x).round() as u32`. The field `mark.margin_y` is never read. Every construction site sets both fields to `MARK_MARGIN_FRAC`, masking the dead field. A future maintainer who sets `margin_y` to a different value expecting per-axis control will observe no change — silent logic error.

The `margin_x` docstring says "fraction of image width" but is applied to `min(W, H)` in the Height path. The doc is accurate only for the `LongEdge` branch.

- [general-purpose]: MEDIUM — margin_y dead; margin_x doc wrong for Height path
- [type-design-analyzer]: HIGH — API hazard, future callers will be silently wrong
- [comment-analyzer]: CRITICAL — doc says "fraction of image width" but code uses min(W,H)

**Verified**: present=drifted → corrected. margin_y unused at line 1205 (only margin_x read). margin_x doc at line 236 says "image width" but applied to `pw.min(ph)`.

**Remediation**: (a) Update `margin_x` doc to state it is applied to `min(W, H)` in the `Height` path and to image width in the `LongEdge` path. (b) Update `margin_y` doc to note it is unused in the `Height` path (only active for `LongEdge` marks). (c) File a TD for the structural fix: move margin fields into `BadgeSizeBasis` variants so the dead-field state is structurally unrepresentable.

---

## Theme R3-D — produce.sh RAW raster sub-step swallows fatal exit codes (HIGH)

**Severity: HIGH**

The RAW pipeline's raster watermark step (lines 197–211) uses `|| WM_EXIT=$?` to capture all non-zero exit codes, then only checks `if [[ $WM_EXIT -eq 2 ]]`. Exit codes 1 (strict-fail), 64 (usage), 74 (I/O error), 75 (tempfail), and 77 (permission denied) fall through unhandled and are silently treated as success: the script prints "Done" and continues to the summary.

Concrete scenario: disk full mid-watermark (exit 74) — the script reports success, continues to print the final summary with `$WATERMARKED` count, and exits 0. The user has no indication that their raster files were not processed.

- [silent-failure-hunter]: HIGH — exits 74/77 silently ignored; summary reports success on fatal I/O error

**Verified**: present=yes. Lines 197–211 confirm: `|| WM_EXIT=$?` on line 204, check only for `WM_EXIT -eq 2` on line 208.

**Remediation**: After line 204, add: `if [[ $WM_EXIT -ne 0 && $WM_EXIT -ne 2 ]]; then echo "Raster watermark failed (exit $WM_EXIT). Aborting." >&2; exit $WM_EXIT; fi`

---

## Theme R3-E — produce.sh raster-only pipeline aborts on exit 2 (mark-doesnt-fit) (HIGH)

**Severity: HIGH**

The raster-only pipeline (lines 229–235) runs the watermark command bare under `set -euo pipefail` (line 24) with no `|| WM_EXIT=$?` guard. When any image is too narrow for the marks, `watermark` exits with code 2 (`EX_PARTIAL_FAIL`), and `set -e` immediately aborts the script. The user sees no summary, no output path, no file count — the script dies mid-run. This contradicts the fix in `bb57d24b` which applied partial-failure tolerance only to the RAW pipeline's raster sub-step.

- [general-purpose]: MEDIUM — inconsistent handling between RAW and raster-only paths
- [silent-failure-hunter]: CRITICAL — raster-only path aborts while RAW path gracefully continues

**Verified**: present=yes. Lines 229–235 confirm bare watermark call with no WM_EXIT guard.

**Remediation**: Apply the same `WM_EXIT=0; ... || WM_EXIT=$?` + `if [[ $WM_EXIT -ne 0 && $WM_EXIT -ne 2 ]]; then die; fi` pattern to the raster-only pipeline. Mirror the exit-2 warning from lines 208–210.

---

## Theme R3-G — Readability warning references `--max-long-edge`; export subcommand uses `--long-edge` (HIGH)

**Severity: HIGH**

The `composite_mark_on_pixmap` readability warning (line 1197) says:
> "Consider a larger --max-long-edge."

This function is called by both `watermark` (which does use `--max-long-edge`) and `export` (which uses `--long-edge`, declared at export.rs line 115). When a user running `export` triggers this warning, the actionable advice references a flag that does not exist for the `export` subcommand — following the advice would produce "unrecognized argument: --max-long-edge".

- [general-purpose]: MEDIUM — flag name mismatch, wrong advice for export users

**Verified**: present=yes. Warning at line 1197 says `--max-long-edge`; grep confirms `export.rs` uses `long_edge` (no "max-").

**Remediation**: Change the warning message to flag-agnostic language: `"Consider a larger output resolution."` (removes the broken flag name from both callers' context).

---

## Theme R3-F — No test for `blit_badge_at` alpha compositing path (MEDIUM)

**Severity: MEDIUM**

`blit_badge_at` has three distinct alpha branches (a=0, a=255, partial) in the demultiply loop and a complex per-pixel compositing loop. No test exercises this function: `grep -rn "blit_badge_at"` in `crates/` returns only the definition and one call site. If the alpha math regresses, only visual inspection of output images would catch it.

- [pr-test-analyzer]: CRITICAL (8/10) — three alpha branches untested; correctness relies on visual review

**Remediation**: Add a unit test creating a small 4×4 badge pixmap with known premultiplied RGBA (including a=0, a=128, a=255 pixels), call `blit_badge_at` onto a solid-color destination, and assert composited pixel values within tolerance.

---

## Theme R3-H — No test for JXL/HEIC unsupported-format warning (MEDIUM)

**Severity: MEDIUM**

The JXL/HEIC warning path in `watermark.rs` (lines 200–212) emits a `tracing::warn!` and increments `skipped_unsupported`. No integration test verifies that placing a `.jxl` file in the watermark source directory produces `skipped-unsupported: 1` in the summary and exit 0 (graceful skip). If `SourceKind::classify` is extended to recognize JXL, the warning would silently stop firing.

- [pr-test-analyzer]: MEDIUM (6/10) — warning path untested

**Remediation**: Add a CLI integration test placing a `.jxl` dummy file in the watermark source dir, asserting `skipped-unsupported: 1` in stderr and exit 0.

---

## Theme R3-I — `render_shadow` doc contract not enforced; shadow silently dropped in legacy path (MEDIUM)

**Severity: MEDIUM**

`ExportOptions.render_shadow` doc says "Shadow gradient to apply when `render_marks` is non-empty", but no enforcement exists. The CLI guards this correctly (lines 299–306), but the library API allows `render_shadow: Some(..)` with empty `render_marks`. Additionally, if a user combines `--badge` (legacy text/image watermark) with `--mark1-png --with-shadow`, `export_photo` takes the legacy path (line 1583) before reaching `render_to_jpeg`, silently dropping both the mark and the shadow.

- [code-reviewer]: CRITICAL (85%) — shadow dropped in legacy path, user gets no indication
- [type-design-analyzer]: LOW — doc constraint not type-enforced
- [general-purpose]: LOW — library API allows shadow-without-marks

**Remediation**: (a) Update `render_shadow` doc to remove the misleading constraint ("when render_marks is non-empty") and replace with "Optional shadow; callers should set this only alongside render_marks". (b) In `export_photo`, check if `options.render_shadow.is_some() && options.render_marks.is_empty()` and emit `tracing::warn!` that `--with-shadow` has no effect without `--mark1-png`/`--mark2-png`. (c) Consider the same for the legacy-path combination.

---

## Theme R3-J — `Arc<Vec<MarkSpec>>` adds indirection without sharing (LOW)

**Severity: LOW**

At export.rs line 329, `single_pass_marks` is wrapped in `Arc::new()`. Each per-photo iteration does `.as_ref().clone()`, which clones the inner `Vec<MarkSpec>`. The `Arc` shares a pointer to a `Vec` that is immediately deep-copied per iteration — it shares nothing. The `Arc` adds a heap allocation + pointer dereference with no benefit.

- [code-simplifier]: MEDIUM — Arc adds complexity without amortizing clone cost

**Remediation**: Remove `Arc::new()` at line 329. Capture `&single_pass_marks` by reference in the rayon closure (it is `Send + Sync`), or simply clone the `Vec<MarkSpec>` directly: `render_marks: single_pass_marks.clone()`.

---

## Theme R3-K — `mark_h_preview` duplicates `fit_equal_margin`'s internal formula (LOW)

**Severity: LOW**

`composite_mark_on_pixmap` computes `mark_h_preview = ((ph as f32 * frac).round() as u32).max(1)` (line 1188) solely to check against `MARK_MIN_READABLE_PX`. The exact same formula appears inside `fit_equal_margin` at line 419. If `fit_equal_margin` fails (mark-doesnt-fit), the readability warning is moot; if it succeeds, `placement.h()` provides the same value. The pre-computation is redundant and creates a drift risk if the sizing formula changes in one location but not the other.

- [code-simplifier]: MEDIUM — duplicate formula; can use placement.h() post-call

**Remediation**: Remove `mark_h_preview`. Move the readability check to after the `fit_equal_margin` call: `if placement.h() < MARK_MIN_READABLE_PX { tracing::warn!(..., mark_h_px = placement.h(), ...); }`.

---

## Theme R3-L — produce.sh header comment says "export → watermark" as separate steps (LOW)

**Severity: LOW**

The script header (line 4) says `ingest → cull → cluster → develop → export → watermark` as separate pipeline stages. After the single-pass optimization, the RAW pipeline combines export+watermark in one `--mark1-png`/`--with-shadow` invocation. The runtime banner (line 113) also says `export→watermark` for the RAW path. These are now misleading to users who wonder why no separate watermark step appears in the log.

- [comment-analyzer]: MEDIUM — header comment describes superseded two-pass pipeline

**Remediation**: Update header to `ingest → cull → cluster → develop → export+watermark (single-pass)`. Update line 113 banner accordingly.

---

## Theme R3-M — `margin_x` doc says `margin_px` is only a recommendation (LOW)

**Severity: LOW**

The `fit_equal_margin` doc at line 397 says "The recommended value is `round(min(W, H) × MARK_MARGIN_FRAC)`" but all current callers use exactly this formula (no other value is possible). "Recommended" overstates the optionality and may lead a future caller to pass a different value without understanding the implications.

- [comment-analyzer]: LOW — "recommended" vs. "established convention"

**Remediation**: Change "The recommended value is" to "Callers pass" in the doc comment.

---

## Disposition summary

| Theme | Severity | File:Line | Action |
|---|---|---|---|
| R3-A | CRITICAL | cli/tests/, lib.rs:2010 | Add unit + CLI integration tests for single-pass path |
| R3-B | HIGH | lib.rs:1295–1321 | Remove demultiply; resize in premultiplied space; update blend |
| R3-C | HIGH | lib.rs:236–239, 1205 | Fix margin_x/margin_y docs; file TD for structural fix |
| R3-D | HIGH | produce.sh:197–211 | Guard fatal exit codes after WM_EXIT capture |
| R3-E | HIGH | produce.sh:229–235 | Add WM_EXIT guard to raster-only pipeline |
| R3-G | HIGH | lib.rs:1197 | Change `--max-long-edge` to flag-agnostic message |
| R3-F | MEDIUM | lib.rs (blit_badge_at tests) | Add unit test for blit_badge_at alpha compositing |
| R3-H | MEDIUM | watermark.rs:200–212 | Add CLI integration test for JXL unsupported-format path |
| R3-I | MEDIUM | lib.rs:203–204, export_photo | Fix doc; warn when shadow set without marks in legacy path |
| R3-J | LOW | export.rs:329, 466 | Remove Arc wrapper; capture Vec by ref or clone directly |
| R3-K | LOW | lib.rs:1188 | Replace mark_h_preview with placement.h() post-call |
| R3-L | LOW | produce.sh:4, 113 | Update header and banner to reflect single-pass |
| R3-M | LOW | lib.rs:397 | Change "recommended" to "established convention" |

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 8
  verified: 7
  drifted: 1
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: 28856f7ae6dde8cbe38e732fdb1728c26eaf379f
      file: crates/photohelper-cli/tests/cli.rs
      line: 0
      present: yes
      retain: yes
      reason: grep confirmed zero matches for mark1_png/mark2_png/with_shadow in test files
      evidence_snippet: "fn test_render_to_jpeg_default_no_shadow_no_marks() {\n        let opts = RenderOptions::default();"
    - finding_id: 5ce77aba4188158155a09bee7193e9d076db89ef
      file: crates/photohelper-export/src/lib.rs
      line: 1295
      present: yes
      retain: yes
      reason: demultiply-then-resize sequence confirmed at lines 1295-1321
      evidence_snippet: "// Demultiply tiny-skia's premultiplied RGBA → straight RGBA for the image crate.\n    let badge_data = badge.data();\n    let mut straight: Vec<u8> = Vec::with_capacity(badge_data.len());"
    - finding_id: a66511b1f70e1814a4bd529915c9a6a29096aedf
      file: crates/photohelper-export/src/lib.rs
      line: 1205
      present: drifted
      retain: yes-with-corrected-line
      reason: margin_y not read at line 1205 (only margin_x); corrected from line 236 to line 1205 for primary evidence
      evidence_snippet: "let margin_px = (pw.min(ph) as f32 * mark.margin_x).round() as u32;"
    - finding_id: b7a29e3df56475891884b62c8a55edd5e3c86817
      file: scripts/photohelper-produce.sh
      line: 196
      present: yes
      retain: yes
      reason: WM_EXIT check only for code==2; other fatal codes ignored
      evidence_snippet: "WM_EXIT=0\n        \"$BINARY\" watermark \\\n            ... || WM_EXIT=$?\n        ...\n        if [[ $WM_EXIT -eq 2 ]];"
    - finding_id: 757e501c4bac857adba587ee2b9389c450a26770
      file: scripts/photohelper-produce.sh
      line: 229
      present: yes
      retain: yes
      reason: bare watermark call under set -euo pipefail confirmed at lines 229-235
      evidence_snippet: "\"$BINARY\" watermark \\\n        --source \"$SOURCE_DIR\" \\\n        --mark1  \"$MARK1\" \\\n        --mark2  \"$MARK2\" \\\n        --output \"$WATERMARK_DIR\" \\\n        --max-long-edge \"$MAX_LONG_EDGE\" \\\n        ${FORCE}"
    - finding_id: 5db052da6387895f4c7a5b540d37022a8597b70b
      file: crates/photohelper-export/src/lib.rs
      line: 1197
      present: yes
      retain: yes
      reason: warning says --max-long-edge; export subcommand uses --long-edge
      evidence_snippet: "Consider a larger --max-long-edge. The mark will still be \\\n                     composited — check the output and decide."
    - finding_id: 46cb4c00891ff3933d80bbfe7be497d927c06a0c
      file: crates/photohelper-cli/src/commands/export.rs
      line: 329
      present: yes
      retain: yes
      reason: Arc::new() wraps Vec that is immediately deep-cloned per photo via .as_ref().clone()
      evidence_snippet: "let single_pass_marks = Arc::new(single_pass_marks);"
    - finding_id: 7c78cec8c8d4cd439637b576f403da154a373066
      file: crates/photohelper-export/src/lib.rs
      line: 1188
      present: yes
      retain: yes
      reason: mark_h_preview computed with same formula as fit_equal_margin's internal mark_h
      evidence_snippet: "let mark_h_preview = ((ph as f32 * frac).round() as u32).max(1);"
```
