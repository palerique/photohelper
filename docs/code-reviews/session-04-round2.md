# Session 04 — AI culling pipeline, Session-End Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6 [1m] (orchestrator); opus (Round-2 verification agent)"
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
  agents_requested: [general-purpose]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## R1 Watch-List: All 13 Items CLOSED

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Steps 1/2 swapped; file_missing reachable + warn! | **CLOSED** | `cull.rs:139-168` |
| 2 | Cull error arm calls exit_code_for_error | **CLOSED** | `main.rs:159` |
| 3 | ffi.rs comment corrected: user_qual=-1 / quality-3-internal | **CLOSED** | `ffi.rs:617-620` |
| 4 | Cull idempotency test (second run: walked:0, scored:0) | **CLOSED** | `cli.rs:891-902` |
| 5 | catalog_written field removed | **CLOSED** | 0 grep matches in cull.rs |
| 6 | MODEL_MANIFEST_NAME constant + main.rs uses it | **CLOSED** | `model_bytes.rs:21`, `main.rs:28,152,154` |
| 7 | TD-012 in-source comment in decode.rs | **CLOSED** | `decode.rs:154-156` |
| 8 | EX_USAGE guarded by all_per_photo_errors == 0 | **CLOSED** | `cull.rs:237-243` |
| 9 | RgbConversionFailed not reused for buffer mismatch | **CLOSED** | `ffi.rs:731-737` |
| 10 | TD-016 TECH-DEBT.md updated | **CLOSED** | `TECH-DEBT.md:272` |
| 11 | main.rs:141 comment corrected | **CLOSED** | `main.rs:140-142` |
| 12 | Zero-rows early return uses CullStats::new().summary_line() | **CLOSED** | `cull.rs:114-115` |
| 13 | heartbeat_handle.join() has justifying comment | **CLOSED** | `cull.rs:214-216` |

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 0 |
| LOW | 0 |

**No new findings. No Round 3 required.**

## Regression analysis (adversarial pass on R1 remediations)

**Step-swap TOCTOU**: Swapping Step 1/2 creates the same class of TOCTOU as before (file can vanish between exists() and derive()), but the narrow race window is benign — the photo increments derive_failed, which is functionally correct.

**catalog_written removal**: Strictly dead code; removing it has zero behavioral impact. summary_line() and exit-code logic are unaffected.

**EX_USAGE all_per_photo_errors**: The implementation correctly includes all 6 per-photo error counters (derive_failed, decode_failed, infer_failed, file_missing, content_changed, catalog_inconsistency). The catalog_inconsistency inclusion improves on the R1 suggestion.

**MODEL_MANIFEST_NAME propagation**: Import chain verified clean through model_bytes → lib.rs → main.rs. Zero bare literals of "nima_mobilenet_aesthetic" remain.

**Idempotency test**: Second run asserts walked:0 (not already-scored:2) because unsuperseded_unscored_rows SQL excludes scored photos before the walk — this is the intended design and a stronger verification of the SQL filter.

## Disposition

R2 is clean. Session-end double-review complete. Proceed to ledger updates and PR.
