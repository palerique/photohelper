# Session 05 — Duplicate-detection pipeline, Plan Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6 [1m] (orchestrator); opus (2 sub-agents + R1 verifier)"
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
  agents_requested: [general-purpose, feature-dev:code-architect]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## R1 Watch-List: All 16 Items CLOSED

| # | R1 Theme | R1 Severity | R2 Status | Evidence |
|---|---|---|---|---|
| T-A | Stop-gap TD filing timing | CRITICAL | CLOSED | Lines 559-564: "IN THE SAME COMMIT"; D5 lines 527-529: "Verify…were filed at their introducing commits. Do NOT file them here." |
| T-B | Migration runner snippet incorrect | CRITICAL | CLOSED | Lines 240-257: correct 5-arm pattern (0,1,2,v if v==SCHEMA_VERSION,other); `&mut conn, catalog_path` signatures. |
| T-C | NaN norm passes validation | CRITICAL | CLOSED | Lines 107-109: `is_finite()` fires first; line 185: `EmbeddingZeroVector` for zero-vector before division. |
| T-D | InsertClusterOutcome dead code | HIGH | CLOSED | Lines 272-276: `Result<(), Error>`; "AlreadyAssigned would be dead code" explicit. |
| T-E | Test count +15/158 | HIGH | CLOSED | Line 30: ≥167; test table: 25 new → 168; buffer acknowledged. |
| T-F | Transitivity undocumented | HIGH | CLOSED | Lines 406-411: "Clustering transitivity design decision" paragraph with A/B/C example + TD-017 + decisions/0003. |
| T-G | Phase 2 errors swallowed | HIGH | CLOSED | Lines 349, 398-400: `cluster_write_failed` counter + match block with WARN. |
| T-H | similarity_threshold no range validation | HIGH | CLOSED | Lines 338-344: `value_parser = parse_similarity_threshold`; fn spec rejects NaN/Inf/out-of-range. |
| T-I | verify-model-sha256.sh hardcoded | HIGH | CLOSED | Lines 135-136: D1b sub-task to iterate all manifest sections (~25 LoC). |
| T-J | already_embedded counter wrong | HIGH | CLOSED | Lines 353-357: removed; AlreadyEmbedded always → catalog_inconsistency + WARN. |
| T-K | HANDOFF checkpoint 13 collision | HIGH | CLOSED | Line 523: "Checkpoint 14". |
| T-L | D5 TD-016 conditional | HIGH | CLOSED | Line 525: "unconditional"; D4 header: "mandatory". |
| T-M | Missing derive_failed + skip spec | HIGH | CLOSED | Lines 348-349, 379-380: derive_failed in DedupeStats; explicit pseudocode with continue. |
| T-N | D1a tests insufficient (4 for 6 behaviors) | HIGH | CLOSED | Line 546: 6 tests named; norm+NaN+Inf rejects + cosine_similarity dim-mismatch + round-trip. |
| T-O | Golden-band undefined | HIGH | CLOSED | Line 547: cosine_sim ≥0.999 arm64, ≥0.98 x86_64 vs committed vector. |
| TD-016 binding trigger | HIGH | CLOSED | D4 header: "mandatory"; Checkpoints table: "unconditional". |

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 3 |
| LOW | 3 |

---

## Theme R2-A — `cluster_count` / `singletons` placement causes Arc write conflict [MEDIUM]

- [general-purpose]: MEDIUM — `DedupeStats` lists 9 `AtomicU64` fields + `cluster_count` as "plain u64, set after Phase 2". If `DedupeStats` is shared via `Arc` (standard pattern from `cull.rs`), writing `stats.cluster_count = ...` through an `Arc` requires `Arc::get_mut` (only valid when refcount==1) or interior mutability. `singletons` is in `ClusteringResult` but appears in the summary line — neither struct unambiguously owns it.
- [feature-dev:code-architect]: MEDIUM — the field count claims "10 total" but enumerates 9 AtomicU64 + 1 plain u64 (= 10), omitting `singletons` (11th summary field). The summary printer must read from two different structs for `clusters-found` vs `singletons`.

**Remediation**: The cleanest fix is to move `cluster_count` and `singletons` OUT of `DedupeStats` — keep them as local `u64` variables in `run_dedup` after Phase 2, passed directly to the summary printer. `DedupeStats` then has exactly 9 `AtomicU64` fields (all concurrent counters). Update the field count claim and the pseudocode accordingly. The `ClusteringResult` already carries both `cluster_count` and `singleton_count` — just use them directly.

---

## Theme R2-B — D2a FK enforcement test needs explicit `PRAGMA foreign_keys = ON` note [MEDIUM]

- [feature-dev:code-architect]: MEDIUM — the D2a test "FK enforcement (dup_clusters with nonexistent embedding rejects)" assumes `PRAGMA foreign_keys = ON` is active. SQLite FKs are disabled by default; `Catalog::open` does set the pragma (catalog.rs line 223 confirmed), but the test spec doesn't say "assert FK-violation error specifically" vs "any error." If someone writes this test incorrectly (testing that an insert fails for any reason), they could get a false positive.

**Remediation**: Add to D2a test spec: "FK enforcement test: after `apply_v2_to_v3`, attempt `INSERT INTO dup_clusters ... WHERE (photo_id, model_slug) not in embeddings`; assert the error is a FK violation (`rusqlite::Error::SqliteFailure` with code `SqliteError { code: ErrorCode::ConstraintViolation }`). Requires `PRAGMA foreign_keys = ON` — verify `Catalog::open` already sets this before the migration runs."

---

## Theme R2-C — `cluster_write_failed` `--strict` exclusion undocumented at point of use [MEDIUM]

- [general-purpose]: MEDIUM — exit code 0 note says "clustering may be incomplete if cluster_write_failed > 0 but that is not a per-photo error." This is a design decision that users running `--strict` may find surprising (they expect ALL errors to cause non-zero exit). The exclusion is intentional but only explained inline at exit code 0.

**Remediation**: Add a note at exit code 1 (EX_STRICT_FAIL): "Note: `cluster_write_failed > 0` does NOT trigger `--strict` failure — cluster-insert errors are Phase-2 write failures, not per-photo embedding errors. The `--strict` predicate covers only Phase-1 per-photo counters." This makes the design choice explicit at the predicate definition.

---

## Theme R2-D — Sequencing diagram retains stale "conditional" label [LOW]

- [general-purpose]: LOW — the sequencing diagram at the end of the plan still says "D4 (heartbeat.rs extraction + TD-010 close; conditional on D3 trigger, expected to fire)". D4 is now labeled "mandatory" everywhere else. The parenthetical is stale.

**Remediation**: Change to "D4 (heartbeat.rs extraction + TD-010 close — mandatory)".

---

## Theme R2-E — `from_f32_le_bytes` corrupt-input path not in D1a test list [LOW]

- [feature-dev:code-architect]: LOW — R1 Theme N remediation explicitly requested a test for `from_f32_le_bytes` with byte-slice length not a multiple of 4 (corrupt BLOB). The plan v2 D1a test list names 6 tests but this error path is absent.

**Remediation**: Either add a 7th D1a test (`from_f32_le_bytes_rejects_non_aligned_bytes`) and bump the total to 26/169, OR add a one-line note: "from_f32_le_bytes: bytes.len() % 4 != 0 → Error::EmbeddingEmpty (or a new Error::EmbeddingByteLengthInvalid)." The latter is preferred for conciseness.

---

## Theme R2-F — `ClusteringResult` type file location implicit [LOW]

- [feature-dev:code-architect]: LOW — `ClusteringResult` is defined inline in the `threshold_cluster` algorithm spec but never assigned to a file. It is implicitly a private struct in `dedup.rs`.

**Remediation**: Add one line to D3 before the algorithm spec: "`ClusteringResult` is a `pub(crate)` struct in `crates/photohelper-cli/src/commands/dedup.rs`."

---

## Disposition summary

| Theme | Severity | Fix required before implementation |
|---|---|---|
| R2-A — cluster_count/singletons placement | MEDIUM | Yes — remove from DedupeStats; use locals |
| R2-B — FK test pragma note | MEDIUM | Yes — add FK-violation error spec |
| R2-C — cluster_write_failed strict exclusion | MEDIUM | Yes — add note at exit code 1 |
| R2-D — sequencing diagram stale "conditional" | LOW | Trivial edit |
| R2-E — corrupt-bytes test absent | LOW | Add note or 7th test |
| R2-F — ClusteringResult location | LOW | One-line addition |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 6
  verified: 6
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: "All findings verified by agents reading the plan v2 text directly. R2-A and R2-C
    are design-consistency findings not tied to specific line citations; R2-B, R2-D, R2-E,
    R2-F are corroborated by cross-reading existing code (catalog.rs:223, cull.rs patterns).
    No CRITICAL or HIGH findings. Plan requires 3 MEDIUM remediations before implementation."
```

## Gate assessment

**CONDITIONAL PASS**: No CRITICAL or HIGH findings. The 3 MEDIUM items (R2-A, R2-B, R2-C) require small targeted amendments before implementation begins. All amendments are < 10 LoC each in the plan document. No architectural changes needed. After amendments → plan v3 is ready for implementation.
