# Session 04 — Catalog migration D2a+D2b, Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6 [1m] (orchestrator); opus (all sub-agents via model: 'opus' pin)"
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
| CRITICAL | 1 | A |
| HIGH | 4 | B-doc, B-sig, C, D |
| MEDIUM | 5 | E, F, H, I, J |
| LOW | 2 | K, M |
| **Total** | **12** | |

---

## Theme A — `AbsPath::canonicalize` inside `unsuperseded_unscored_rows` aborts the entire batch on a single deleted file `CRITICAL`

**Agents flagging**: general-purpose (HIGH), code-architect (HIGH/CRITICAL), code-reviewer (CRITICAL), silent-failure-hunter (HIGH), code-simplifier (MEDIUM) — 5/8 agents

`crates/photohelper-catalog/src/catalog.rs:499`

```rust
let source_path =
    AbsPath::canonicalize(std::path::Path::new(&path_str)).map_err(|e| {
        Error::CatalogOpen { path: self.canonical_path.clone(), ... }
    })?;
```

`AbsPath::canonicalize` calls `std::fs::canonicalize`, which requires the file to exist on disk. If any single catalogued photo has been moved, renamed, deleted, or is on an unmounted volume since ingest, the `?` propagates an error and `unsuperseded_unscored_rows` returns `Err` — yielding **zero work items** for the entire culling run. The D3 `run_cull` pipeline explicitly budgets `file_missing` as a per-photo resilience counter (plan lines 393, 405), but this counter is never reachable because the work-list query itself fails first.

**Remediation**: Change `CullRow.source_path` from `AbsPath` to `PathBuf` (raw, as stored). The per-photo existence check in `run_cull` is the correct enforcement point — it can increment `file_missing` and `continue`. This also removes the filesystem round-trip from the batch-query path:

```rust
pub struct CullRow {
    pub photo_id: PhotoId,
    pub source_path: PathBuf,   // raw from DB; caller validates via !source_path.exists()
}
```

Constructing `CullRow` then becomes infallible for the filesystem check:
```rust
out.push(CullRow { photo_id, source_path: PathBuf::from(&path_str) });
```

---

## Theme B — `insert_cull_score` doc references nonexistent `NimaScore.get()` method; `f64` score has no range enforcement `HIGH`

**Agents flagging**: type-design-analyzer (HIGH), comment-analyzer (HIGH)

**B-doc** — `crates/photohelper-catalog/src/catalog.rs:524`

```rust
/// `nima_score.get() as f64` at the call site.
```

`NimaScore` has `as_f32(self) -> f32` and `as_f64(self) -> f64`. There is no `get()` method. The first D3 caller implementing the cull command will get a compile error and improvise — possibly passing an unchecked raw `f64`.

**Remediation**: Change the doc to reference the actual API:
```
/// pass `nima_score.as_f64()` at the call site.
```

**B-sig** — `crates/photohelper-catalog/src/catalog.rs:529`

```rust
pub fn insert_cull_score(
    &self,
    photo_id: PhotoId,
    model_slug: &str,
    score: f64,               // ← no validation
```

The function body performs zero range or finiteness checks. `NaN`, `Infinity`, and values outside `[1.0, 10.0]` are silently written to `cull_scores.aesthetic_score`. The schema has no `CHECK` constraint. A row with `NaN` stored is permanently corrupt — any downstream code constructing a `NimaScore` from catalog data will hit `ScoreOutOfRange`.

**Remediation**: Add a guard at the top of `insert_cull_score`:
```rust
if !score.is_finite() || !(1.0_f64..=10.0_f64).contains(&score) {
    return Err(Error::CatalogInsert {
        photo_id,
        source: Box::new(rusqlite::Error::InvalidParameterName(
            format!("score {score} not in [1.0, 10.0]"),
        )),
    });
}
```

Or add a SQL CHECK constraint alongside (defense-in-depth):
```sql
aesthetic_score REAL NOT NULL CHECK(aesthetic_score >= 1.0 AND aesthetic_score <= 10.0)
```

---

## Theme C — `unsuperseded_unscored_rows` test has no `ORDER BY` assertion `HIGH`

**Agent flagging**: pr-test-analyzer (HIGH)

`crates/photohelper-catalog/src/catalog.rs:924`

```rust
assert_eq!(rows.len(), 3);
```

The SQL specifies `ORDER BY ingested_at_unix_seconds`. The test inserts p1/p2/p3 at timestamps 1000/2000/3000 but only asserts `rows.len() == 3`. Accidentally removing the `ORDER BY` clause would not be caught. The AI culling pipeline is specified to process oldest-first.

**Remediation**: Add ordering assertions immediately after the count check:
```rust
assert_eq!(rows.len(), 3);
assert_eq!(rows[0].photo_id, p1.photo_id(), "oldest ingest first");
assert_eq!(rows[1].photo_id, p2.photo_id());
assert_eq!(rows[2].photo_id, p3.photo_id());
```

---

## Theme D — No cross-model slug isolation test in `unsuperseded_unscored_rows` `HIGH`

**Agent flagging**: pr-test-analyzer (HIGH)

`crates/photohelper-catalog/src/catalog.rs:909`

The `unsuperseded_unscored_rows` test uses a single `model_slug = "nima-aesthetic-v1"` throughout. If the `WHERE model_slug = ?1` predicate were accidentally dropped from the `NOT IN` subquery, all scored photos would disappear from every model's work list — but the existing test would still pass because it only ever queries with one slug.

**Remediation**: After scoring p1 for `"nima-aesthetic-v1"`, query for a different slug and assert p1 still appears:
```rust
let rows_other = cat.unsuperseded_unscored_rows("other-model-v1").unwrap();
assert_eq!(
    rows_other.len(), 3,
    "scoring for model-A must not exclude from model-B's work list"
);
```

---

## Theme E — Wrong error variant: `CatalogOpen` used for per-row path-canonicalize failure `MEDIUM`

**Agents flagging**: silent-failure-hunter (HIGH), code-simplifier (MEDIUM)

`crates/photohelper-catalog/src/catalog.rs:499-506`

```rust
AbsPath::canonicalize(...).map_err(|e| {
    Error::CatalogOpen {
        path: self.canonical_path.clone(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("stored path is not canonicalisable: {path_str}: {e}"),
        )),
    }
})?;
```

`Error::CatalogOpen`'s `Display` is `"could not open catalog at {path}: ..."`. An operator seeing this will investigate SQLite file corruption when the actual problem is a deleted/moved photo. Additionally, `AbsPath::canonicalize` already returns `Error::Io { path, op: "canonicalize", source }` with the correct path and `io::ErrorKind` — the re-wrapping into `InvalidData` destroys that structured information.

This finding merges with Theme A's remediation: if `CullRow.source_path` is changed to `PathBuf`, the canonicalization call is removed entirely and this error mapping disappears.

---

## Theme F — `canonical_path` fallback in `Catalog::open` silently drops canonicalize error `MEDIUM`

**Agent flagging**: silent-failure-hunter (MEDIUM)

`crates/photohelper-catalog/src/catalog.rs:299-302`

```rust
let canonical_path = AbsPath::canonicalize(catalog_path).map_or_else(
    |_| catalog_path.to_path_buf(),     // ← error silently dropped
    |p| p.as_path().to_path_buf(),
);
```

No log line fires when canonicalization fails. All subsequent error messages that include `self.canonical_path` would then print a potentially relative or symlink-laden path, adding debugging friction. The failure is rare (catalog just opened successfully) but not impossible (FUSE mount, race-delete).

**Remediation**:
```rust
let canonical_path = AbsPath::canonicalize(catalog_path).map_or_else(
    |e| {
        tracing::warn!(error = %e, "could not canonicalize catalog path; using raw path");
        catalog_path.to_path_buf()
    },
    |p| p.as_path().to_path_buf(),
);
```

---

## Theme H — `insert_cull_score` poison path is untested `MEDIUM`

**Agent flagging**: pr-test-analyzer (MEDIUM)

`crates/photohelper-catalog/src/catalog.rs:536-554`

The three existing poison tests (`poison_propagates_as_catalog_poisoned_error`, `poison_rollback_discards_panicked_workers_partial_insert`, `poison_recovery_admits_subsequent_inserts`) only exercise the `upsert` path. The poison-recovery block in `insert_cull_score` (lines 536-554) is structurally identical but entirely untested. A future refactor of `insert_cull_score` that breaks the `extended_code == 1` check would go undetected.

**Remediation**:
```rust
#[test]
fn insert_cull_score_poison_returns_catalog_poisoned() {
    let dir = tempfile::tempdir().unwrap();
    let cat = Arc::new(Catalog::open(dir.path().join("c.db"), 1).unwrap());
    let photo = make_test_photo(dir.path(), 1);
    cat.upsert(&photo, 0).unwrap();
    cat.poison_for_testing();
    let err = cat.insert_cull_score(photo.photo_id(), "nima-v1", 5.0, 0).unwrap_err();
    assert!(matches!(err, Error::CatalogPoisoned { .. }));
}
```

---

## Theme I — No test verifies first writer's score is preserved on `AlreadyScored` `MEDIUM`

**Agent flagging**: pr-test-analyzer (MEDIUM)

`crates/photohelper-catalog/src/catalog.rs:891-895`

The `migration_v1_to_v2_upgrades_and_enforces_fk` test inserts score 5.0, then score 6.0 (returns `AlreadyScored`), but never reads back the stored value. If `INSERT OR IGNORE` were changed to `INSERT OR REPLACE`, the test would still pass — it only checks the outcome enum variant. `INSERT OR REPLACE` would silently overwrite the first writer's score with the second's, corrupting the per-model-slug deduplification guarantee.

**Remediation**: Add a read-back assertion in the test:
```rust
{
    let conn = cat.conn.lock().unwrap();
    let stored: f64 = conn
        .query_row(
            "SELECT aesthetic_score FROM cull_scores WHERE photo_id = ?1 AND model_slug = ?2",
            rusqlite::params![&pid.as_bytes().to_vec(), "nima-aesthetic-v1"],
            |r| r.get(0),
        )
        .unwrap();
    assert!((stored - 5.0).abs() < f64::EPSILON, "INSERT OR IGNORE must preserve first writer's score");
}
```

---

## Theme J — TD-013 in-source comment missing from `insert_cull_score` (CLAUDE.md policy violation) `MEDIUM`

**Agent flagging**: general-purpose (MEDIUM)

`TECH-DEBT.md:235` claims:
> In-source: `// TD-013: per-cull-run audit trail absent`

`grep -n "TD-013"` in `catalog.rs` returns 0 matches. Per CLAUDE.md §No Acceptable Trade-offs Policy: "The stop-gap MUST be labeled in-source. A comment at the stop-gap site cites the `TD-N` identifier so the next reader sees the obligation without grepping." The obligation is clear; the comment is absent.

**Remediation**: Add to `insert_cull_score` (near the method's doc comment or the INSERT statement):
```rust
// TD-013: per-cull-run audit trail absent; each score row carries only
// `scored_at_unix_seconds`, not a cull_run_id. See TECH-DEBT.md.
```

---

## Theme K — Stale column names in decision doc 0001 §What's deliberately NOT in v1 `LOW`

**Agent flagging**: comment-analyzer (LOW)

`docs/decisions/0001-catalog-schema-v1.md:110`

```markdown
- **Cull score columns** (`quality`, `blur`, `eye_state`, etc.) —
```

Session 04 shipped `cull_scores` with a single `aesthetic_score` column (no `quality`, `blur`, or `eye_state`). The section is not marked superseded. A reader encountering this list would have a false expectation of what D2a delivered.

**Remediation**: Append a parenthetical:
```markdown
- **Cull score columns** (`quality`, `blur`, `eye_state`, etc.) — *(actual v2 uses `cull_scores.aesthetic_score`; see `docs/decisions/0002-catalog-schema-v2.md`)*
```

---

## Theme M — `apply_v1_to_v2` repeats identical `CatalogOpen` closure 3× in 17 lines `LOW`

**Agent flagging**: code-simplifier (LOW)

`crates/photohelper-catalog/src/catalog.rs:634-650`

The identical `|e| Error::CatalogOpen { path: path.to_path_buf(), source: Box::new(e) }` appears on lines 637-639, 642-644, and 646-648. A local closure eliminates the repetition with no behavioral change.

**Remediation**: Extract a local closure:
```rust
fn apply_v1_to_v2(conn: &mut Connection, path: &Path) -> Result<(), Error> {
    let map_err = |e: rusqlite::Error| Error::CatalogOpen {
        path: path.to_path_buf(),
        source: Box::new(e),
    };
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(&map_err)?;
    tx.execute_batch(MIGRATE_V1_TO_V2_SQL).map_err(&map_err)?;
    tx.commit().map_err(&map_err)
}
```

---

## Discarded findings (verified as hallucinated or inaccurate)

| ID | Description | Reason |
|---|---|---|
| L | Decision doc 0002 migration table ">= 3" should say "any other value" | 9th agent verified: the ">= 3" entry is correct integer comparison for the match arm; the finding was wrong |
| G | `catalog_fresh_db_initializes_to_v2` missing "row insertable" assertion | 9th agent verified: the test does check `cull_scores` table existence via `sqlite_master`; finding overstated the gap |

---

## Disposition summary

| Theme | Severity | Disposition | Owner |
|---|---|---|---|
| A | CRITICAL | MUST fix before D3: change `CullRow.source_path` to `PathBuf` | Session 04 remediation |
| B-doc | HIGH | Fix doc comment: `nima_score.as_f64()` | Session 04 remediation |
| B-sig | HIGH | Add range guard in `insert_cull_score` | Session 04 remediation |
| C | HIGH | Add ORDER BY ordering assertion to test | Session 04 remediation |
| D | HIGH | Add cross-model slug isolation test | Session 04 remediation |
| E | MEDIUM | Resolved by Theme A remediation (AbsPath removal) | Session 04 remediation |
| F | MEDIUM | Add `tracing::warn!` in canonical_path fallback | Session 04 remediation |
| H | MEDIUM | Add `insert_cull_score_poison_returns_catalog_poisoned` test | Session 04 remediation |
| I | MEDIUM | Add score read-back assertion in `AlreadyScored` test | Session 04 remediation |
| J | MEDIUM | Add `// TD-013:` comment in-source | Session 04 remediation |
| K | LOW | Append reference to 0002 in 0001:110 | Session 04 remediation |
| M | LOW | Extract local `map_err` closure in `apply_v1_to_v2` | Session 04 remediation |

## R1 watch-list (Round 2 must verify)

1. `CullRow.source_path` changed to `PathBuf` — `AbsPath::canonicalize` no longer called inside `unsuperseded_unscored_rows`.
2. `insert_cull_score` has range guard: `!score.is_finite() || !(1.0..=10.0).contains(&score)` → `Err`.
3. Doc comment corrected to `nima_score.as_f64()`.
4. Test asserts ordering after `unsuperseded_unscored_rows` returns 3 rows.
5. Test asserts cross-model slug isolation.
6. `tracing::warn!` added to canonical_path fallback.
7. `insert_cull_score` poison test added.
8. Score read-back assertion added to `AlreadyScored` test.
9. `// TD-013:` in-source comment present.
10. Decision doc 0001:110 stale column names annotated.

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 12
  verified: 9
  drifted: 1
  hallucinated: 1
  unreadable: 0
  compromised: 0
  discard_rate: 0.17
  details:
    - finding_id: A
      file: crates/photohelper-catalog/src/catalog.rs
      line: 499
      present: yes
      retain: yes
      reason: AbsPath::canonicalize called per-row at line 499, requiring files to exist
      evidence_snippet: "AbsPath::canonicalize(std::path::Path::new(&path_str)).map_err(|e| {"

    - finding_id: B-doc
      file: crates/photohelper-catalog/src/catalog.rs
      line: 524
      present: yes
      retain: yes
      reason: nima_score.get() in doc; NimaScore has no get() method (has as_f32/as_f64)
      evidence_snippet: "/// `nima_score.get() as f64` at the call site."

    - finding_id: B-sig
      file: crates/photohelper-catalog/src/catalog.rs
      line: 529
      present: yes
      retain: yes
      reason: score: f64 with no range or finiteness guard in function body
      evidence_snippet: "score: f64,"

    - finding_id: C
      file: crates/photohelper-catalog/src/catalog.rs
      line: 924
      present: drifted
      retain: yes-with-corrected-line
      reason: assert_eq!(rows.len(), 3) present at line 924 (not 920); no ordering assertion
      evidence_snippet: "assert_eq!(rows.len(), 3);"

    - finding_id: D
      file: crates/photohelper-catalog/src/catalog.rs
      line: 909
      present: yes
      retain: yes
      reason: Test uses single model_slug; no second-slug assertion
      evidence_snippet: "fn unsuperseded_unscored_rows_excludes_scored_and_superseded() {"

    - finding_id: E
      file: crates/photohelper-catalog/src/catalog.rs
      line: 499
      present: yes
      retain: yes
      reason: Error::CatalogOpen used for path-canonicalize failure at same location as A
      evidence_snippet: "Error::CatalogOpen { path: self.canonical_path.clone(),"

    - finding_id: F
      file: crates/photohelper-catalog/src/catalog.rs
      line: 299
      present: yes
      retain: yes
      reason: |_| catalog_path.to_path_buf() silently drops error with no warn
      evidence_snippet: "let canonical_path = AbsPath::canonicalize(catalog_path).map_or_else("

    - finding_id: J
      file: crates/photohelper-catalog/src/catalog.rs
      line: 529
      present: yes
      retain: yes
      reason: grep of catalog.rs returns 0 TD-013 matches; TECH-DEBT.md:235 requires it
      evidence_snippet: "pub fn insert_cull_score("

    - finding_id: K
      file: docs/decisions/0001-catalog-schema-v1.md
      line: 110
      present: yes
      retain: yes
      reason: quality/blur/eye_state column names at line 110; v2 uses aesthetic_score only
      evidence_snippet: "- **Cull score columns** (`quality`, `blur`, `eye_state`, etc.) —"

    - finding_id: L
      file: docs/decisions/0002-catalog-schema-v2.md
      line: 88
      present: no
      retain: no
      reason: ">= 3" is correct integer comparison for the match arm; finding was wrong
      evidence_snippet: "| `≥ 3` | Reject with `Error::CatalogSchemaTooNew` |"

    - finding_id: M
      file: crates/photohelper-catalog/src/catalog.rs
      line: 634
      present: yes
      retain: yes
      reason: Three identical CatalogOpen closures confirmed at lines 637-644-646
      evidence_snippet: "fn apply_v1_to_v2(conn: &mut Connection, path: &Path) -> Result<(), Error> {"
```
