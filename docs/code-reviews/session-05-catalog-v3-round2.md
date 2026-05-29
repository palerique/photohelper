# Session 05 — Catalog v3 API (D2a+D2b), Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6 [1m] (orchestrator); opus (1 R2 agent)"
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

## R1 Watch-List: All 9 Items — Disposition

| R1 Theme | Severity | R2 Status |
|---|---|---|
| A — TD-017 + TD-018 filed | CRITICAL | **CLOSED** — both entries in TECH-DEBT.md with all required fields |
| B — dim range guard | HIGH | **CLOSED** — Rust-level guard at catalog.rs:666-674 before INSERT OR IGNORE |
| C — dim in `all_embeddings_for_model` | HIGH | **CLOSED** — SELECT includes dim; return type Vec<(PhotoId, Vec<u8>, usize)> |
| D — FK violation test | MEDIUM | **CLOSED** — `insert_embedding_fk_violation_with_nonexistent_photo` at line 1612; FK DOES propagate despite OR IGNORE (SQLite docs confirmed) |
| E — migration test renamed | MEDIUM | **CLOSED** — `migration_v2_to_v3_reopen_succeeds` |
| F — fresh DB test renamed | MEDIUM | **CLOSED** — `catalog_fresh_db_initializes_to_v3` |
| G — inconsistent poison recovery | LOW | NOT REMEDIATED (deferred; no production impact; extract helper when third write method added) |
| H — dup_clusters FK API test | LOW | **CLOSED** — `insert_dup_cluster_with_missing_embedding_fails` at line 1633 |
| I — replace test incomplete | LOW | **CLOSED** — threshold + clustered_at_unix_seconds asserted in `insert_dup_cluster_happy_path_and_replace` |

## Triage summary (R2 new findings)

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 1 |
| LOW | 2 |

---

## R2-B — `dim_i64 as usize` unguarded cast in `all_embeddings_for_model` [MEDIUM → CLOSED]

**Status**: Remediated inline before Round 2 artifact was written.

`catalog.rs:755` changed `dim_i64 as usize` to `usize::try_from(dim_i64).map_err(|e| Error::CatalogOpen {...})?`. If a corrupt DB row has `dim = -1`, the code now returns `Error::CatalogOpen` with `"embeddings.dim value -1 is out of usize range"` instead of silently wrapping to `18446744073709551615`. Mirrors the photo_id blob-length check pattern at lines 743-753.

**Closed inline**.

---

## R2-A — `insert_dup_cluster_happy_path_and_replace` only checked `cluster_id` [LOW → CLOSED]

**Status**: Remediated inline.

`insert_dup_cluster_happy_path_and_replace` now asserts `cluster_id=7`, `similarity_threshold≈0.90`, and `clustered_at_unix_seconds=3000` after the second `INSERT OR REPLACE`. Float tolerance uses `f64::from(0.90_f32)` to match the widening cast in `insert_dup_cluster`. **Closed inline**.

---

## R2-C — dim upper-bound test missing [LOW → CLOSED]

**Status**: Remediated inline.

`insert_embedding_dim_bounds_guard` test added: `dim=65537` must reject (`CatalogInsert`); `dim=65536` must succeed. **Closed inline**.

---

## Regression analysis

All R1 remediations are verified clean:
- The 3-tuple return type change in `all_embeddings_for_model` is updated correctly in the round-trip test (both bytes and dim asserted per photo).
- The FK violation test correctly asserts `Err(CatalogInsert)` — SQLite docs confirm FK violations are NOT suppressed by `INSERT OR IGNORE` (unlike UNIQUE/NOT NULL/CHECK/ROWID).
- No external callers of `all_embeddings_for_model` exist yet; return type change is backward-compatible at this stage.

**No regressions introduced by R1 edits.**

## Disposition

**R2 is clean. Catalog v3 sub-component double-review complete.** D3 (dedup subcommand) may begin.

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 0
  verified: 0
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: "All R1 watch-list items verified CLOSED or deferred. R2 found 1 MEDIUM +
    2 LOW, all remediated inline before this artifact. 178 tests passing (just ci green).
    Additional insight from test discovery: SQLite ON CONFLICT IGNORE does NOT suppress
    FK violations — only UNIQUE, NOT NULL, CHECK, ROWID. FK violations always propagate
    as errors regardless of conflict resolution strategy."
```
