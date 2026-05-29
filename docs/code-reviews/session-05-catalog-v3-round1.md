# Session 05 — Catalog v3 API (D2a+D2b), Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6 [1m] (orchestrator); opus (all 8 sub-agents + 9th verifier)"
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
  agents_requested: [general-purpose, feature-dev:code-architect, feature-dev:code-reviewer,
    pr-review-toolkit:type-design-analyzer, pr-review-toolkit:silent-failure-hunter,
    pr-review-toolkit:comment-analyzer, pr-review-toolkit:pr-test-analyzer,
    pr-review-toolkit:code-simplifier]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

**Scope**: `crates/photohelper-catalog/src/{catalog,row,schema,lib}.rs` (D2a schema v3 + D2b embedding API)

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 1 |
| HIGH | 2 |
| MEDIUM | 3 |
| LOW | 3 |

---

## Theme A — TD-018 not filed in TECH-DEBT.md [CRITICAL]

- [general-purpose]: CRITICAL — `catalog.rs:653-654` contains `// TD-018: embedding stored as raw f32 LE bytes; quantization='f32' hardcoded. // See TECH-DEBT.md § TD-018 for the int8/f16 quantization upgrade plan.` but no TD-018 entry exists in TECH-DEBT.md (ledger ends at TD-019 with TD-017 and TD-018 absent).
- [pr-review-toolkit:comment-analyzer]: CRITICAL (same finding) — `docs/decisions/0003-catalog-schema-v3.md` line 59 also references TD-018.

**Verification (F1)**: `present: yes` — `catalog.rs:653` verbatim: `"// TD-018: embedding stored as raw f32 LE bytes..."`. Zero matches in TECH-DEBT.md.

Per CLAUDE.md § No Acceptable Trade-offs Policy: "Stop-gap commits without companion TDs violate this policy."

**Remediation**: Add TD-018 to TECH-DEBT.md with all required fields:
- **Stop-gap location**: `schema.rs::MIGRATE_V2_TO_V3_SQL` (`quantization TEXT NOT NULL DEFAULT 'f32'`) + `catalog.rs::insert_embedding` (hardcodes `'f32'` literal) @ session 05 D2b commit
- **Fundamental fix**: support int8/f16 quantization — add `quantization` dispatch in `all_embeddings_for_model`; extend `insert_embedding` to accept a quantization parameter; adapt callers
- **Binding trigger**: first user request for int8/f16 quantization or storage-size complaint
- **Scope estimate**: ~30 LoC / low risk
- **Consequence of inaction**: all embeddings stored as f32 (4 bytes/dim); 512 dims × 85.3 MB model weight = acceptable at v0.1; becomes a storage concern at scale

---

## Theme B — `INSERT OR IGNORE` swallows CHECK violations in `insert_embedding` [HIGH]

- [feature-dev:code-reviewer]: HIGH — `catalog.rs:673` uses `INSERT OR IGNORE INTO embeddings`. SQLite's `OR IGNORE` conflict algorithm silently suppresses not only UNIQUE violations but also CHECK constraint violations. If `dim == 0` (or `dim > 65536`) is passed, the `CHECK(dim > 0 AND dim <= 65536)` fires but `OR IGNORE` swallows it. `tx.changes()` returns 0, and the method returns `Ok(InsertEmbeddingOutcome::AlreadyEmbedded)` — a false result indicating the photo was already embedded when it was never stored.

**Verification (F2)**: `present: yes-flag-for-human-triage` — `INSERT OR IGNORE` at `catalog.rs:673` confirmed. Note: 9th agent incorrectly stated CHECK violations are not suppressed by OR IGNORE; SQLite documentation confirms they are (same as UNIQUE, NOT NULL, ROWID). Finding is retained.

SQLite docs: "IGNORE: the current SQL statement does not abort. Instead, it continues processing subsequent rows as if nothing went wrong. No error is returned when the IGNORE conflict resolution algorithm is used." This applies to UNIQUE, NOT NULL, CHECK, and ROWID constraints.

**Remediation**: Add a Rust-level range guard before the INSERT (mirroring `insert_cull_score`'s score-range guard at `catalog.rs:570-578`):
```rust
if dim == 0 || dim > 65536 {
    return Err(Error::CatalogInsert {
        photo_id,
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("dim {dim} is outside valid range [1, 65536]"),
        )),
    });
}
```
This surfaces the error at the Rust layer before the INSERT. The schema `CHECK` remains as a belt-and-suspenders DB-layer guard.

---

## Theme C — `all_embeddings_for_model` drops the stored `dim` column [HIGH]

- [pr-review-toolkit:type-design-analyzer]: HIGH — `catalog.rs:710` SELECT is `"SELECT photo_id, embedding FROM embeddings WHERE model_slug = ?1"` — the `dim` column is not fetched. The schema stores `dim` explicitly for efficient lookup without deserializing the BLOB. Two consequences:
  1. **Corruption silent**: If a row's BLOB is truncated (disk corruption), the caller silently constructs a wrong-dimension `ImageEmbedding` since `dim = bytes.len() / 4` may compute incorrectly.
  2. **Quantization break**: When TD-018 lands (int8/f16), `bytes.len() / sizeof(element)` requires knowing the quantization. Baking the f32 assumption into the API breaks the quantization upgrade path.

**Verification (F3)**: `present: yes` — `catalog.rs:710`: `"SELECT photo_id, embedding FROM embeddings WHERE model_slug = ?1"` — dim absent.

**Remediation**: Extend the SELECT and return type:
```sql
SELECT photo_id, embedding, dim FROM embeddings WHERE model_slug = ?1
```
Change the return type to `Vec<(PhotoId, Vec<u8>, usize)>` or introduce a small struct `EmbeddingBlob { photo_id, bytes, dim }`. The caller (CLI clustering pass) validates `dim == bytes.len() / 4` before constructing `ImageEmbedding`. ~15 LoC change in the catalog method + all call sites (currently only tests).

---

## Theme D — `insert_embedding` FK violation path untested [MEDIUM]

- [pr-review-toolkit:pr-test-analyzer]: MEDIUM — `catalog.rs:1428` test only exercises happy-path `Inserted` and `AlreadyEmbedded`. No test calls `cat.insert_embedding(nonexistent_pid, ...)`. The FK `embeddings.photo_id REFERENCES photos(id)` should fire, but the behavior with `INSERT OR IGNORE` (see Theme B) is that FK violations may also be suppressed. This test gap prevents discovery of whether the FK is enforced or silently swallowed.

**Verification (F6)**: `present: yes` — test at `catalog.rs:1428` only tests valid `photo_id`.

**Remediation**: Add a test:
```rust
fn insert_embedding_fk_violation_rejects_nonexistent_photo() {
    let fake_pid = photo_id_from_row_bytes([0xFFu8; 32]);
    let emb = make_unit_embedding_bytes(512);
    let result = cat.insert_embedding(fake_pid, "clip-v1", &emb, 512, 1000);
    // Document the actual behavior: FK violation with INSERT OR IGNORE
    // either returns Err(CatalogInsert) or AlreadyEmbedded (if FK is swallowed).
}
```
The test should empirically determine whether `OR IGNORE` suppresses the FK, and assert the correct contract.

---

## Theme E — `migration_v2_to_v3_is_idempotent` tests re-open, not DDL idempotency [MEDIUM]

- [pr-review-toolkit:pr-test-analyzer]: MEDIUM — `catalog.rs:1185` drops the first open immediately (`drop(Catalog::open(...))`). The second open at line 1187 finds `user_version = 3` and takes the `v if v == SCHEMA_VERSION => {}` no-op arm — `apply_v2_to_v3` is NOT called twice. The `CREATE TABLE IF NOT EXISTS` idempotency guard is never exercised. The test validates "re-opening a v3 DB succeeds" (correct and valuable), but the test name claims "idempotent" (a stronger property not tested).

**Verification (F5)**: `present: yes` — `catalog.rs:1185`: `drop(Catalog::open(&db_path, 1).unwrap());` immediately dropped.

**Remediation**: Rename to `migration_v2_to_v3_reopen_succeeds`. To test actual DDL idempotency (running MIGRATE_V2_TO_V3_SQL twice), add a separate test that manually calls `apply_v2_to_v3` twice on the same connection:
```rust
fn migration_v2_to_v3_ddl_is_idempotent() {
    let mut conn = Connection::open(":memory:").unwrap();
    conn.execute_batch(INIT_SQL).unwrap();
    apply_v1_to_v2(&mut conn, ...).unwrap();
    apply_v2_to_v3(&mut conn, ...).unwrap(); // first run
    apply_v2_to_v3(&mut conn, ...).unwrap(); // second run — must not fail
    // verify user_version == 3 and both tables exist
}
```

---

## Theme F — Test function name `catalog_fresh_db_initializes_to_v2` stale [MEDIUM]

- [general-purpose]: MEDIUM — `catalog.rs:997`: function named `catalog_fresh_db_initializes_to_v2` but `SCHEMA_VERSION = 3` and the body asserts `v == SCHEMA_VERSION` with a comment "Fresh catalog must be at SCHEMA_VERSION = 3."
- [feature-dev:code-architect]: TRIVIAL (same finding, lower severity per lens)
- [pr-review-toolkit:comment-analyzer]: LOW (same finding)

**Verification (F4)**: `present: yes` — `catalog.rs:997`: `fn catalog_fresh_db_initializes_to_v2() {` confirmed.

**Remediation**: Rename to `catalog_fresh_db_initializes_to_v3` or `catalog_fresh_db_initializes_to_current_schema_version`.

---

## Theme G — Inconsistent poison-recovery in write methods [LOW]

- [feature-dev:code-reviewer]: LOW — `upsert` and `insert_cull_score` use the full 15-line recovery: `into_inner()` + `ROLLBACK` + return `CatalogPoisoned`. `insert_embedding` (line 663) and `insert_dup_cluster` (line 766) use the simpler 3-line `map_err(|_| CatalogPoisoned)` without ROLLBACK. For read methods this is correct; for write methods that open IMMEDIATE transactions, skipping ROLLBACK is inconsistent. No immediate production impact (poisoned catalog is terminal), but inconsistency is a future trap.

**Remediation**: Extract a private helper method `fn lock_conn_or_recover(&self) -> Result<MutexGuard<Connection>, Error>` that encapsulates the ROLLBACK + CatalogPoisoned pattern, and apply it to all write methods. File as a TODO comment if not addressing this session.

---

## Theme H — `dup_clusters_fk_violation` test uses raw SQL; no API-level test [LOW]

- [pr-review-toolkit:pr-test-analyzer]: LOW — The FK test at `catalog.rs:1244` bypasses `insert_dup_cluster` and uses raw SQL. This verifies the schema constraint but not that `insert_dup_cluster` wraps the FK error into `Error::CatalogInsert`.

**Remediation**: Add a companion test calling `cat.insert_dup_cluster(pid, "clip-v1", 0, 0.95, 2000)` where `pid` has no embedding for `"clip-v1"`. Assert `Err(Error::CatalogInsert { .. })`.

---

## Theme I — `insert_dup_cluster` replacement test incomplete [LOW]

- [pr-review-toolkit:pr-test-analyzer]: LOW — The `insert_dup_cluster_happy_path_and_replace` test (line 1535) checks that `cluster_id` changes from 0 to 7 but does not verify `similarity_threshold` or `clustered_at_unix_seconds` were also replaced.

**Remediation**: Extend the assertion block to also query and check `similarity_threshold` == 0.90 and `clustered_at_unix_seconds` == 3000 after the second insert.

---

## Disposition summary

| Theme | Severity | Fix |
|---|---|---|
| A — TD-018 not filed | CRITICAL | Add TD-018 to TECH-DEBT.md |
| B — INSERT OR IGNORE swallows CHECK | HIGH | Add Rust-level dim range guard |
| C — all_embeddings_for_model drops dim | HIGH | Add dim to SELECT + return type |
| D — insert_embedding FK untested | MEDIUM | Add FK violation test |
| E — migration test misleading | MEDIUM | Rename + add real idempotency test |
| F — test name v2 stale | MEDIUM | Rename to v3 |
| G — inconsistent poison recovery | LOW | Extract helper (or TODO comment) |
| H — dup_clusters FK raw SQL only | LOW | Add API-level test |
| I — replace test incomplete | LOW | Extend assertion |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 6
  verified: 5
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 1
  discard_rate: 0.00
  notes: "F2 marked compromised: 9th agent incorrectly stated CHECK violations
    are not suppressed by INSERT OR IGNORE. SQLite docs confirm they are.
    Finding retained as HIGH based on SQLite documentation. All other 5
    spot-checks verified present=yes."
  details:
    - finding_id: F1
      file: TECH-DEBT.md / crates/photohelper-catalog/src/catalog.rs
      line: 653
      present: yes
      retain: yes
      reason: "TD-018 referenced at catalog.rs:653; absent from TECH-DEBT.md"
      evidence_snippet: "// TD-018: embedding stored as raw f32 LE bytes; quantization='f32' hardcoded."
    - finding_id: F2
      file: crates/photohelper-catalog/src/catalog.rs
      line: 673
      present: yes
      retain: yes
      reason: "INSERT OR IGNORE present; finding retained per SQLite docs (OR IGNORE suppresses CHECK violations)"
      evidence_snippet: "INSERT OR IGNORE INTO embeddings (photo_id, model_slug, dim, quantization, embedding, embedded_at_unix_seconds) VALUES (?1, ?2, ?3, 'f32', ?4, ?5)"
    - finding_id: F3
      file: crates/photohelper-catalog/src/catalog.rs
      line: 710
      present: yes
      retain: yes
      reason: "SELECT missing dim column"
      evidence_snippet: "\"SELECT photo_id, embedding FROM embeddings WHERE model_slug = ?1\""
    - finding_id: F4
      file: crates/photohelper-catalog/src/catalog.rs
      line: 997
      present: yes
      retain: yes
      reason: "Function named initializes_to_v2 but SCHEMA_VERSION is 3"
      evidence_snippet: "fn catalog_fresh_db_initializes_to_v2() {"
    - finding_id: F5
      file: crates/photohelper-catalog/src/catalog.rs
      line: 1185
      present: yes
      retain: yes
      reason: "First open immediately dropped; second open is no-op; DDL not run twice"
      evidence_snippet: "drop(Catalog::open(&db_path, 1).unwrap());"
    - finding_id: F6
      file: crates/photohelper-catalog/src/catalog.rs
      line: 1428
      present: yes
      retain: yes
      reason: "Test only covers valid photo_id; no FK-violation path tested"
      evidence_snippet: "fn insert_embedding_happy_path_and_already_embedded()"
```
