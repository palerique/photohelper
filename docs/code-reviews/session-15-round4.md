# Session 15 — Session-end final review (R3 remediation verification), Round 4

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

**Scope**: R3 remediation commit `bda7c38d`. Verified that all 13 R3 themes are correctly closed, and identified residual issues.

**R3 themes confirmed CLEAN**: R3-B (premultiplied blend), R3-C (margin docs), R3-G (flag name in warning), R3-I (render_shadow doc), R3-J (Arc removed), R3-K (mark_h_preview removed), R3-L (banner), R3-M (fit_equal_margin doc). R3-D and R3-E (produce.sh WM_EXIT guards) partially remediated — see R4-A.

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 0 | — |
| HIGH | 3 | R4-A, R4-B, R4-C |
| MEDIUM | 2 | R4-D, R4-E |
| LOW | 4 | R4-F, R4-G, R4-H, R4-I |

9th-agent verification: 5/5 verified, 0 hallucinated, 0 drifted, discard_rate: 0.00.

---

## Theme R4-A — `export` subcommand in produce.sh has no partial-failure guard (HIGH)

**Severity: HIGH**

The RAW pipeline's `$BINARY export` call (produce.sh line ~168) has no `|| WM_EXIT=$?` guard. Under `set -euo pipefail`, any non-zero exit — including exit 2 (`EX_PARTIAL_FAIL`, triggered when a mark does not fit a narrow image) — immediately kills the script with no error message, no summary, no output path. This directly contradicts the fix in R3-D/R3-E which gracefully handles exit 2 at the two `$BINARY watermark` call sites, but leaves the `export` call (which uses the same `--mark1-png`/`--mark2-png` single-pass mark fitting that can also produce a geometry-error partial failure) unguarded.

- [silent-failure-hunter]: HIGH — exit 2 from export kills script silently

**Verified**: present=yes. produce.sh line 168 `"$BINARY" export \` with no `|| WM_EXIT=$?`.

**Remediation**: Wrap the export call with `WM_EXIT=0; ... || WM_EXIT=$?`, then add the same exit-2-tolerance guard with a user-facing warning, and abort on other fatal codes.

---

## Theme R4-B — produce.sh line 19 still describes "Export → Watermark" as separate stages (HIGH)

**Severity: HIGH**

produce.sh line 19 reads:
```
#   Ingest → Cull (AI) → Cluster (dedup) → Develop (XMP) → Export → Watermark
```
The RAW pipeline now performs single-pass export+watermark in one `export --mark1-png --with-shadow` call. This header comment gives every reader a wrong mental model of the architecture (two binary invocations where there is one). The banner on line 3 correctly says "export+watermark (single-pass)" and the runtime step label correctly says "Export+Watermark … single-pass", making line 19 directly contradictory.

- [general-purpose]: LOW (considered still correct for raster-only secondary step)
- [comment-analyzer]: HIGH (primary RAW path is misrepresented)

**Verified**: present=yes. Line 19 contains "Export → Watermark" as separate entries.

**Remediation**: Change line 19 to:
```
#   Ingest → Cull (AI) → Cluster (dedup) → Develop (XMP) → Export+Watermark (single-pass)
#   (co-located JPEG/PNG files are watermarked separately via the watermark subcommand)
```

---

## Theme R4-C — `test_render_to_jpeg_with_shadow_and_mark` verifies shadow but not mark compositing (HIGH)

**Severity: HIGH**

The test (lib.rs ~line 2408) declares it verifies "shadow + height-based mark are composited" but only asserts `bot_lum < top_lum` (shadow darkens bottom rows). The mark (4×4 white badge at Mark1 top-right) would be placed at approximately pixel (23, 1) in a 32×32 canvas. Neither the checked pixels (0, 0) and (0, 31) overlap the mark region. If `composite_mark_on_pixmap` were silently no-op'd — by a future bug or swallowed error — the test would remain green because the shadow assertion alone passes.

This test claims to be the primary unit-level verification for R3-A (no integration test for single-pass path), making its gap directly consequential.

- [pr-test-analyzer]: HIGH (8/10) — mark compositing unverified

**Verified**: present=yes. Test only checks `bot_lum < top_lum`, no top-right corner pixel assertion.

**Remediation**: Add an assertion checking a pixel known to be inside the mark region (e.g., pixel (25, 4) for the 32×32 canvas), asserting it is brighter than the original gray background (160 per channel).

---

## Theme R4-D — Missing in-source `// TD-041` label at `margin_y` stop-gap site (MEDIUM)

**Severity: MEDIUM**

CLAUDE.md requires: "The stop-gap MUST be labeled in-source. A comment at the stop-gap site cites the `TD-N` identifier so the next reader sees the obligation without grepping." The `margin_y` field at lib.rs line 247 has no adjacent `// TD-041` comment. TD-028 at lib.rs line 1233 demonstrates the required pattern. Without the in-source label, a future maintainer touching `margin_y` has no indication that a remediation is pending.

- [type-design-analyzer]: MEDIUM — explicit CLAUDE.md policy violation

**Verified**: present=yes. `grep -n "TD-041" crates/photohelper-export/src/lib.rs` returns zero matches.

**Remediation**: Add `// TD-041: dead in Height path — see TECH-DEBT.md` as a trailing comment on the `pub margin_y: f32,` line (or the line immediately after the doc block).

---

## Theme R4-E — No `trap` handler in produce.sh for temp dir cleanup on SIGINT (MEDIUM)

**Severity: MEDIUM**

produce.sh creates `$RASTER_TEMP` (line ~188) but has no `trap` handler. SIGINT during any non-WM_EXIT-guarded step leaves the temp dir orphaned and prints no user-facing message. By contrast, the two WM_EXIT-guarded `watermark` call sites correctly clean up `$RASTER_TEMP` before the guard (line ~205) because `rm -rf "$RASTER_TEMP"` executes unconditionally — but this only works for those specific paths. Every other step (ingest, cull, export, cluster) leaves `$RASTER_TEMP` orphaned on SIGINT.

- [silent-failure-hunter]: MEDIUM

**Verified**: present=yes. `grep -n "trap " scripts/photohelper-produce.sh` returns zero matches.

**Remediation**: Add near the top of the script:
```bash
cleanup() { [[ -n "${RASTER_TEMP:-}" ]] && rm -rf "$RASTER_TEMP"; }
trap cleanup EXIT
```

---

## Theme R4-F — TD-041 missing commit SHA (LOW)

**Severity: LOW**

TECH-DEBT.md TD-041 "Stop-gap commit" field reads "session-15 R3 remediation" without a commit SHA. CLAUDE.md requires "file path + line + commit SHA" at the stop-gap location.

- [type-design-analyzer]: LOW

**Remediation**: Add the R3 remediation commit SHA (`bda7c38d`) to the stop-gap line.

---

## Theme R4-G — `make_1x1_pixmap` test helper doesn't enforce premultiplied invariant (LOW)

**Severity: LOW**

The helper at lib.rs ~line 2343 writes raw bytes to a Pixmap's `data_mut()` without asserting `r <= a && g <= a && b <= a`. Invalid premultiplied pixels (e.g., `make_1x1_pixmap(255, 0, 0, 128)`) would pass to `blit_badge_at` and produce undefined behavior from the compositing formula. Current callers are correct, but the helper is a latent hazard.

- [code-architect]: LOW

**Remediation**: Add `debug_assert!(r <= a && g <= a && b <= a, "violates premultiplied invariant")` inside the helper.

---

## Theme R4-H — Zero-output success (exit 0, 0 files) looks identical to real success (LOW)

**Severity: LOW**

Both pipelines print "Done" with a file count even when 0 files were produced (all inputs unsupported or empty dir). A user who passes the wrong directory gets an exit-0 success message with "0 watermarked JPEG(s)" and no further indication of the problem.

- [silent-failure-hunter]: LOW

**Remediation**: Add a zero-output warning after the final count in both pipeline branches.

---

## Theme R4-I — `export_single_pass_mark1_png_writes_jpeg` doesn't verify mark presence in output (LOW)

**Severity: LOW**

The CLI integration test verifies exit 0, "written: 1", and non-empty file, but not that the mark was composited. Since unit-level coverage exists (partly addressed by R4-C remediation), this is LOW.

- [pr-test-analyzer]: MEDIUM → demoted to LOW (unit coverage is the primary gate)

**Remediation**: Optional pixel check in the decoded output JPEG, or defer to the unit test.

---

## Disposition summary

| Theme | Severity | Action |
|---|---|---|
| R4-A | HIGH | Add WM_EXIT guard to export call in produce.sh |
| R4-B | HIGH | Fix line 19 header comment |
| R4-C | HIGH | Add mark-pixel assertion to render_to_jpeg test |
| R4-D | MEDIUM | Add `// TD-041` comment at margin_y field |
| R4-E | MEDIUM | Add cleanup trap to produce.sh |
| R4-F | LOW | Add commit SHA to TD-041 |
| R4-G | LOW | Add premultiplied invariant assert to make_1x1_pixmap |
| R4-H | LOW | Add zero-output warning |
| R4-I | LOW | Defer to unit test coverage |

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 5
  verified: 5
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: R4-A
      file: scripts/photohelper-produce.sh
      line: 168
      present: yes
      retain: yes
      reason: export call has no || WM_EXIT=$? guard
      evidence_snippet: '"$BINARY" export \'
    - finding_id: R4-B
      file: scripts/photohelper-produce.sh
      line: 19
      present: yes
      retain: yes
      reason: "Ingest → ... → Export → Watermark as separate stages, contradicts single-pass"
      evidence_snippet: "Ingest → Cull (AI) → Cluster (dedup) → Develop (XMP) → Export → Watermark"
    - finding_id: R4-C
      file: crates/photohelper-export/src/lib.rs
      line: 2411
      present: yes
      retain: yes
      reason: only bot_lum < top_lum asserted; no pixel in mark region
      evidence_snippet: "bot_lum < top_lum,"
    - finding_id: R4-D
      file: crates/photohelper-export/src/lib.rs
      line: 247
      present: yes
      retain: yes
      reason: no TD-041 comment adjacent to margin_y field
      evidence_snippet: "pub margin_y: f32,"
    - finding_id: R4-E
      file: scripts/photohelper-produce.sh
      line: 0
      present: yes
      retain: yes
      reason: grep for "trap " returns zero matches
      evidence_snippet: "set -euo pipefail"
```
