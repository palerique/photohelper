# Session 05 — dedup-mobileclip, Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent session); verification agent pinned to opus"
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

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 0 |
| LOW | 0 |
| **Total** | **0** |

**Round 2 is CLEAN.**

---

## R1 Watch-List Verification

### R1-A1: `insert_embedding` guards `dim*4 == bytes.len()` — CLOSED

`catalog.rs:675` — `if embedding_bytes.len() != dim * 4 { return Err(Error::CatalogInsert{...}) }`.
Uses `embedding_bytes.len()` (correct parameter name). Also validates `dim` range at line 666.

### R1-A2: `dedup.rs` uses `dim` (not `_dim`) in filter_map and validates byte-length — CLOSED

`dedup.rs:277` — `.filter_map(|(pid, bytes, dim)| {` — `dim` destructured and used.
Line 278: `if bytes.len() != dim * 4 {` — byte-length validation present with `tracing::error!`
and `deserialize_failed.fetch_add`.

### R1-A3: `all_embeddings_for_model` docstring corrected — CLOSED

`catalog.rs:722` — docstring now says `insert_embedding enforces dim*4 == bytes.len() at write
time, so the returned triples are byte-length consistent.` No longer claims "caller validates."

### R1-B: `#[cfg(test)] mod threshold_cluster_tests` with ≥5 unit tests — CLOSED

`dedup.rs:451-548` — 6 tests present:
- `empty_input_returns_zero_clusters` — n=0 → 0 clusters, 0 singletons
- `single_element_is_one_singleton` — n=1 → 1 singleton
- `two_orthogonal_vectors_are_two_singletons` — orthogonal → 2 singletons at threshold 0.95
- `two_identical_vectors_at_threshold_1_0_form_one_cluster` — identical → 1 cluster
- `two_orthogonal_vectors_at_threshold_1_0_are_singletons` — orthogonal → 2 singletons at 1.0
- `three_elements_partial_cluster` — pair+singleton → 2 clusters

Tests verify meaningful invariants; `tempfile` lifetime is correct (derive completes before
`TempDir` drops).

### R1-C: Single-instance guard in `MobileClip::new` — CLOSED

`mobileclip.rs:42` — `static INSTANCE_EXISTS: AtomicBool`. Lines 95-109: `compare_exchange`
in `new()` with `tracing::warn!`. `Drop` at line 82-84 resets to `false` (`Ordering::Release`).
Guard cannot get stuck; `Drop` always fires even on panic unwind.

### R1-D: `if let Ok(sim)` → match with `tracing::error!` on Err — CLOSED

`dedup.rs:400-413` — full match with `Ok(sim) if sim >= threshold`, `Ok(_)`, and `Err(e)`
arms. Err arm logs `tracing::error!` with both photo IDs and the error.

### R1-E1: Log when Phase 2 embedding set is empty — CLOSED

`dedup.rs:262-267` — `if all_embeddings.is_empty() { tracing::info!(model = CLIP_MODEL_SLUG,
"no embeddings found for model; skipping clustering phase"); }`.

### R1-E2: `deserialize_failed` counter in DedupeStats — CLOSED

`dedup.rs:68` — field declared. Incremented at lines 283 and 290 (both failure paths).
Included in `summary_line`. `all_errors` for `--strict` intentionally excludes
`deserialize_failed` (catalog corruption is logged but is not a per-photo embed-phase error
that `--strict` is designed to surface — reasonable design).

---

## Disposition summary

| Theme | R1 severity | R2 status |
|---|---|---|
| A — dim consistency gap | CRITICAL | CLOSED |
| B — threshold_cluster zero tests | CRITICAL | CLOSED |
| C — SESS module-scoped guard | HIGH | CLOSED |
| D — cosine_similarity silent swallow | HIGH | CLOSED |
| E — Phase 2 empty/corrupt silent | HIGH | CLOSED |
| F — catalog_inconsistency conflation | MEDIUM | CLOSED |
| G — insert_dup_cluster OR REPLACE | MEDIUM | CLOSED (warn advisory via tracing) |
| H — cluster_write_failed strict | MEDIUM | CLOSED (added to strict check) |
| I — TD-017/TD-015 stale | MEDIUM | CLOSED |
| J — filter_map (DISCARDED in R1) | — | N/A |
| K — SESSION-STATE catalog | MEDIUM | CLOSED |
| L — Stale docs | MEDIUM | CLOSED |
| M — superseded photos in clustering | MEDIUM | CLOSED (fixed in R1-A SQL) |
| N — mobileclip naming | MEDIUM | CLOSED (doc note added) |
| O — TD-019 misquote | LOW | CLOSED |

**All 15 retained findings from Round 1 are CLOSED. 0 new findings from Round 2.**

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 8
  verified: 8
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: R1-A1
      file: crates/photohelper-catalog/src/catalog.rs
      line: 675
      present: yes
      retain: yes
      reason: "dim*4==bytes.len() guard confirmed present and uses correct parameter name"
      evidence_snippet: "if embedding_bytes.len() != dim * 4 {"
    - finding_id: R1-A2
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 277
      present: yes
      retain: yes
      reason: "dim destructured (not _dim); byte-length check at line 278 present"
      evidence_snippet: ".filter_map(|(pid, bytes, dim)| {"
    - finding_id: R1-A3
      file: crates/photohelper-catalog/src/catalog.rs
      line: 722
      present: yes
      retain: yes
      reason: "docstring corrected; no longer claims caller validates"
      evidence_snippet: "insert_embedding enforces dim*4 == bytes.len() at write time"
    - finding_id: R1-B
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 451
      present: yes
      retain: yes
      reason: "6 unit tests in #[cfg(test)] mod threshold_cluster_tests"
      evidence_snippet: "mod threshold_cluster_tests {"
    - finding_id: R1-C
      file: crates/photohelper-ai/src/mobileclip.rs
      line: 42
      present: yes
      retain: yes
      reason: "INSTANCE_EXISTS AtomicBool + Drop reset confirmed"
      evidence_snippet: "static INSTANCE_EXISTS: AtomicBool = AtomicBool::new(false);"
    - finding_id: R1-D
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 400
      present: yes
      retain: yes
      reason: "match with three arms including Err(e) → tracing::error!"
      evidence_snippet: "match embeddings[i].1.cosine_similarity(&embeddings[j].1) {"
    - finding_id: R1-E1
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 262
      present: yes
      retain: yes
      reason: "tracing::info! on empty all_embeddings confirmed"
      evidence_snippet: "if all_embeddings.is_empty() {"
    - finding_id: R1-E2
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 68
      present: yes
      retain: yes
      reason: "deserialize_failed field declared; incremented at two sites"
      evidence_snippet: "deserialize_failed: AtomicU64,"
```
