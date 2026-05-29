# Session 04 — plan review, Round 2

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
  agents_requested: [general-purpose, feature-dev:code-reviewer, pr-review-toolkit:pr-test-analyzer]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## R1 watch-list verification (32/32 PASS)

All 32 R1 items verified fully remediated. No regressions from R1 edits.
See detailed per-item verification in the general-purpose agent output (archived).

---

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 3 |
| MEDIUM | 5 |
| LOW | 2 |

---

## R2-H1 — `file_missing` dispatch not implementable: `photohelper-raw::Error` has no `Io` variant (HIGH)

**Agents**: general-purpose, feature-dev:code-reviewer
**File**: `docs/plans/session-04.md:D3 pipeline step 2`

D3 says: "`read_raw_rgb returns Err(Io { NotFound }) → file_missing++`". But `photohelper_raw::Error` has no `Io` variant (verified: the error enum has `RawExifUnavailable`, `RawDecodeFailed`, `RawPath`, etc. — all LibRaw-specific). When a file is missing, LibRaw's `libraw_open_file` returns `LIBRAW_IO_ERROR` (code -2), which maps to `Error::RawDecodeFailed { cause: LibRawCallFailed { libraw_code: -2, ... } }` — indistinguishable from permission denied or corruption.

**Fix**: Add an explicit file-existence pre-check in D3's pipeline BEFORE calling `read_raw_rgb`:
```
if !source_path.as_ref().exists() { file_missing++; continue }
```
This check is cheap and is the correct place to distinguish "file deleted since ingest" from "file corrupt/unreadable". D1e does NOT need a new error variant.

**Remediation**: D3 pipeline step 2 should read: "Pre-check: if `source_path` does not exist on disk → `file_missing++`; `continue`. Then call `read_raw_rgb` — any error at this point is `decode_failed` (corrupt, wrong format, permission denied, etc.)."

---

## R2-H2 — `insert_cull_score` FK violation error path cannot discriminate without leaking storage backend (HIGH)

**Agents**: feature-dev:code-reviewer
**File**: `docs/plans/session-04.md:D2b, D3 line 398`

D3 says "`Err (FK violation) → catalog_inconsistency++`". But the plan does not specify how `run_cull` discriminates an FK violation from other `insert_cull_score` errors (disk full, WAL corruption, etc.). `insert_cull_score` returns `Err(Error::CatalogInsert { source: BoxedSourceError })` — discriminating FK violations via SQLITE_CONSTRAINT_FOREIGNKEY (extended code 787) requires downcasting through `BoxedSourceError`, leaking the storage backend through the abstraction — a CLAUDE.md violation.

**Fix**: For v0.1, **all `insert_cull_score` errors map to `catalog_inconsistency`** without FK-vs-other discrimination. The FK race (photo deleted between SELECT and INSERT) is the dominant cause; other errors are pathological and rarer. The WARN log should include `source_path` for diagnostics regardless of error type.

**Remediation**: D3 pipeline step 4 should read: "Any `insert_cull_score` error → `catalog_inconsistency++`; `tracing::warn!(path = %source_path.display(), err = %e, \"catalog write failed after inference\")`. Do NOT attempt to discriminate FK violations from other insert errors (would require downcasting through BoxedSourceError, violating CLAUDE.md's no-storage-leak convention)."

---

## R2-H3 — CI band bounds for `nima_scores_cc0_r8_cr3_fixture` unknowable at plan-write time; `nima-regenerate-golden` recipe missing (HIGH)

**Agents**: pr-review-toolkit:pr-test-analyzer
**File**: `docs/plans/session-04.md:test plan row D1c`

The test row says "Linux x86_64 CI: `score ∈ [band_low, band_high]` per D0' measurements and DN-025". But D0' hasn't run yet — the CI band bounds cannot be known until D0' executes and prints actual fixture scores. The plan has no bridge from "D0' prints scores" to "test file contains golden vector and CI band is calibrated." The `nima-regenerate-golden` recipe (specified in session-03 plan line 232) is absent from session-04 deliverables.

**Remediation**: Add to D1c deliverables: "After D0' records fixture scores: (a) store 10-class softmax output as `crates/photohelper-ai/tests/fixtures/nima/golden_{fixture}.npy` (or raw f32); (b) CI band = `[score - 2.0, score + 2.0]` clamped to `[1.0, 10.0]` (generous for cross-arch FMA drift; tight enough to catch model-loading failures). Add `just nima-regenerate-golden` recipe to `justfile` per session-03 plan: runs NIMA on CC0 fixtures, writes golden files. Update CI job to compare against this band."

---

## R2-M1 — `from_catalog_f64` precision-loss formula is a tautology (MEDIUM)

**Agents**: general-purpose, feature-dev:code-reviewer (both caught)
**File**: `docs/plans/session-04.md:D1c line ~186`

Plan says: `WARN if (v as f32 - v as f32).abs() > 1e-6`. The expression `v as f32 - v as f32` is always `0.0` (same cast twice, subtracted from itself). The WARN never fires.

**Remediation**: Fix formula to: `((v as f32) as f64 - v).abs() > 1e-6` (round-trip comparison: cast to f32 then back to f64, compare to original f64 value).

---

## R2-M2 — `model_dir` path resolution in `main.rs` is pseudocode, not a spec (MEDIUM)

**Agents**: general-purpose
**File**: `docs/plans/session-04.md:D3 lines ~427`

`let model_dir = /* default: binary-adjacent models/ or env override */;` is a comment, not a specification. Implementer needs to know: the env var name, the fallback resolution, the error path if directory doesn't exist.

**Remediation**: Replace with: "`model_dir` = `PHOTOHELPER_MODEL_DIR` env var if set, else `current_exe().parent().join(\"models\")`. If `current_exe()` fails, emit `tracing::error!` and exit `EX_IOERR`. No separate directory-existence check — `VerifiedModelBytes::from_manifest` returns `ManifestNotFound` if the directory or file is absent."

---

## R2-M3 — `unwrap()` in `thread_local!` pattern needs `#[allow]` attribute specification (MEDIUM)

**Agents**: feature-dev:code-reviewer
**File**: `docs/plans/session-04.md:D1c`

The plan's D1c shows `let sess = guard.as_mut().unwrap(); // safe: just inserted`. The workspace escalates `unwrap_used` to `warn`, and CI runs `clippy -D warnings`. Without `#[allow(clippy::unwrap_used)]`, `just lint` will fail.

**Remediation**: Add to D1c: "The `guard.as_mut().unwrap()` requires `#[allow(clippy::unwrap_used, reason = \"guard proven Some: if-is_none branch either inserts Some or returns Err\")]`. This satisfies `unwrap_used = warn` + CI `-D warnings`."

---

## R2-M4 — `MODEL_SLUG` constant defined in `cull.rs` is an abstraction leak (MEDIUM)

**Agents**: feature-dev:code-reviewer
**File**: `docs/plans/session-04.md:D3`

`MODEL_SLUG = "nima-aesthetic-v1"` ties the model binary to its database identifier. It belongs in `photohelper-ai` (alongside `Nima`) not in the CLI command layer. When a second scorer lands, the slug must travel with the model definition.

**Remediation**: Define as `pub const MODEL_SLUG: &str = "nima-aesthetic-v1"` in `photohelper-ai` (as a crate constant or associated constant on `Nima`). `cull.rs` imports and uses it. D3 should note this placement.

---

## R2-M5 — `catalog_fresh_db_initializes_to_v2` test uses hardcoded `2` instead of `SCHEMA_VERSION` (MEDIUM)

**Agents**: pr-review-toolkit:pr-test-analyzer
**File**: `docs/plans/session-04.md:test plan row D2a`

The test row says `PRAGMA user_version = 2` — hardcoded. PR1-T19 remediated `open_schema_version_too_new_returns_error` to use `SCHEMA_VERSION + 1`; the new test should follow the same pattern to avoid future maintenance traps.

**Remediation**: Change to `assert_eq!(v, SCHEMA_VERSION)`.

---

## R2-L1 — `catalog_written` absent from summary line (LOW)

**Agents**: general-purpose
**File**: `docs/plans/session-04.md:D3 summary line`

The 10-field `CullStats` has `catalog_written` but the summary line lists only 9 fields (omitting `catalog_written`). `catalog_written` = `scored + already_scored` by definition — omitting it is defensible. But the asymmetry should be acknowledged.

**Remediation**: Add a one-line note: "`catalog_written` omitted from summary line (always equals `scored + already_scored`; redundant for human-readable output)."

---

## R2-L2 — FK setup for `insert_score_outcome_already_scored` test unspecified (LOW)

**Agents**: pr-review-toolkit:pr-test-analyzer
**File**: `docs/plans/session-04.md:test plan row D2b`

The test inserts a `cull_scores` row twice but needs a `photos` row first (FK constraint). The plan doesn't specify the setup mechanism.

**Remediation**: Add note: "Both `catalog_v1_to_v2_migration` and `insert_score_outcome_already_scored` share the FK setup pattern. Use direct SQL `INSERT INTO photos (id, ...) VALUES (X32BYTES, ...)` with a synthetic 32-byte blob to satisfy the FK without the full `Photo`/`Catalog::upsert` construction path."

---

## Disposition summary

| Finding | Severity | Action |
|---|---|---|
| R2-H1 file_missing pre-check | HIGH | Remediate R2 |
| R2-H2 FK violation all→catalog_inconsistency | HIGH | Remediate R2 |
| R2-H3 CI band bounds + golden recipe | HIGH | Remediate R2 |
| R2-M1 from_catalog_f64 tautology | MEDIUM | Remediate R2 |
| R2-M2 model_dir pseudocode | MEDIUM | Remediate R2 |
| R2-M3 unwrap allow attribute | MEDIUM | Remediate R2 |
| R2-M4 MODEL_SLUG location | MEDIUM | Remediate R2 |
| R2-M5 hardcoded 2 in test | MEDIUM | Remediate R2 |
| R2-L1 catalog_written summary | LOW | Acknowledge only |
| R2-L2 FK setup unspecified | LOW | Remediate R2 |

**Gate**: 0 CRITICAL, 3 HIGH — plan NOT ready for implementation until R2 remediations applied.

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 10
  verified: 10
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
```
