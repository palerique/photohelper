# Session 04 — Catalog migration D2a+D2b, Review Round 2

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

## R1 Watch-List: All 10 Items CLOSED

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | `CullRow.source_path` → `PathBuf` | **CLOSED** | `row.rs:89` |
| 2 | `insert_cull_score` range guard | **CLOSED** | `catalog.rs:559` |
| 3 | Doc corrected: `nima_score.as_f64()` | **CLOSED** | `catalog.rs:525` |
| 4 | Ordering assertion in test | **CLOSED** | `catalog.rs:974-980` |
| 5 | Cross-model slug isolation test | **CLOSED (partially)** | `catalog.rs:982-987` — see R2-A |
| 6 | `tracing::warn!` in canonical_path fallback | **CLOSED** | `catalog.rs:301-305` |
| 7 | `insert_cull_score` poison test | **CLOSED** | `catalog.rs:873-886` |
| 8 | Score read-back assertion | **CLOSED** | `catalog.rs:928-943` |
| 9 | `// TD-013:` comment in-source | **CLOSED** | `catalog.rs:530-532` |
| 10 | Decision doc 0001:110 annotated | **CLOSED** | `0001-catalog-schema-v1.md:111-113` |

---

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 0 | — |
| HIGH | 2 | R2-A, R2-B |
| MEDIUM | 4 | R2-C, R2-D, R2-E, R2-F |
| LOW | 4 | R2-G, R2-H, R2-I, R2-J |
| **Total** | **10** | |

No CRITICAL items — Round 3 not required per `docs/quality-assurance.md § Double-review protocol`.

---

## Theme R2-A — R1-D cross-model test is vacuous: assertion runs before any scores exist `HIGH`

**Agents flagging**: code-reviewer (HIGH), pr-test-analyzer (HIGH) — 2/8

`crates/photohelper-catalog/src/catalog.rs:981-987`

```rust
// R1-D: scoring for model-A must not exclude from model-B work list.
let rows_other = cat.unsuperseded_unscored_rows("other-model-v1").unwrap();
assert_eq!(
    rows_other.len(),
    3,
    "scoring for one model must not exclude from a different model's work list"
);
```

This assertion runs at line 982. The first `insert_cull_score` call (scoring p1 for "nima-aesthetic-v1") is at line 990 — eight lines later. At line 982 the `cull_scores` table is completely empty. `id NOT IN (SELECT photo_id FROM cull_scores WHERE model_slug = ?1)` returns an empty set for any slug, so `rows_other.len() == 3` is trivially true regardless of whether the `WHERE model_slug = ?1` clause is in the SQL. If someone removed that clause entirely, making the subquery match scores from all models, this assertion would still pass.

The comment says "scoring for model-A must not exclude from model-B" but no model-A scoring has happened yet at this point.

**Remediation**: Move the cross-model assertion after line 990 (the first `insert_cull_score`) and strengthen it:

```rust
// Score p1 under slug = "nima-aesthetic-v1".
cat.insert_cull_score(p1.photo_id(), slug, 5.0, 0).unwrap();
let rows = cat.unsuperseded_unscored_rows(slug).unwrap();
assert_eq!(rows.len(), 2);
assert!(rows.iter().all(|r| r.photo_id != p1.photo_id()));

// R2-A: scoring p1 for model-A must NOT exclude p1 from model-B's work list.
// This assertion only has meaning because p1 is already scored above.
// If WHERE model_slug = ?1 were dropped from the SQL, the NOT IN subquery
// would exclude p1 from model-B's list too, and this assertion would fail.
let rows_other = cat.unsuperseded_unscored_rows("other-model-v1").unwrap();
assert_eq!(
    rows_other.len(),
    3,
    "p1 scored for nima-aesthetic-v1 must still appear in other-model-v1 work list"
);
assert!(
    rows_other.iter().any(|r| r.photo_id == p1.photo_id()),
    "p1 must be present in other-model-v1 results despite being scored for nima-aesthetic-v1"
);
```

---

## Theme R2-B — `rusqlite::Error::InvalidParameterName` misused for domain range validation `HIGH`

**Agents flagging**: code-reviewer (MEDIUM), type-design-analyzer (HIGH), silent-failure-hunter (HIGH), code-simplifier (MEDIUM) — 4/8

`crates/photohelper-catalog/src/catalog.rs:560-565`

```rust
return Err(insert_error(
    photo_id,
    rusqlite::Error::InvalidParameterName(format!(
        "aesthetic_score {score} is not finite or not in [1.0, 10.0]"
    )),
));
```

`rusqlite::Error::InvalidParameterName` means "a SQL `:named` parameter was not found in a prepared statement." Using it to carry an application-level range-validation message causes two problems:

1. The Display impl prepends `"Invalid parameter name: "`, so the full operator-facing message reads: `"could not insert photo XYZ: Invalid parameter name: aesthetic_score 7.5 is not finite…"`. This actively misdirects debugging toward SQL binding code when the problem is a caller-provided value.

2. If rusqlite itself ever returns `InvalidParameterName` from a genuine binding failure in this method (hypothetically, in a future refactor), downstream match arms cannot distinguish the application-originated error from the rusqlite-originated one.

**Remediation**: Use a standard `std::io::Error` boxed into `Error::CatalogInsert` directly, bypassing the misleading rusqlite variant:

```rust
if !score.is_finite() || !(1.0_f64..=10.0_f64).contains(&score) {
    return Err(Error::CatalogInsert {
        photo_id,
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("aesthetic_score {score} is outside the valid range [1.0, 10.0]"),
        )),
    });
}
```

This produces an unambiguous operator message: `"could not insert photo XYZ: aesthetic_score 0.5 is outside the valid range [1.0, 10.0]"`.

---

## Theme R2-C — `insert_cull_score` range guard has zero test coverage `MEDIUM`

**Agents flagging**: general-purpose (MEDIUM), code-architect (MEDIUM), silent-failure-hunter (MEDIUM), pr-test-analyzer (MEDIUM) — 4/8

`crates/photohelper-catalog/src/catalog.rs:559`

The range guard `!score.is_finite() || !(1.0_f64..=10.0_f64).contains(&score)` was added by R1-B-sig but no test exercises it. All test call sites pass valid scores (5.0, 6.0, 7.0). If the guard condition were accidentally inverted or the range changed, no test would fail.

**Remediation**:
```rust
#[test]
fn insert_cull_score_rejects_out_of_range_values() {
    let dir = tempfile::tempdir().unwrap();
    let cat = Catalog::open(dir.path().join("c.db"), 1).unwrap();
    let photo = make_test_photo(dir.path(), 1);
    cat.upsert(&photo, 0).unwrap();
    let pid = photo.photo_id();
    // Rejection cases.
    for bad_score in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, 0.999, -1.0, 10.001] {
        assert!(
            cat.insert_cull_score(pid, "nima-v1", bad_score, 0).is_err(),
            "score {bad_score} must be rejected"
        );
    }
    // Boundary values that MUST succeed.
    assert_eq!(
        cat.insert_cull_score(pid, "boundary-min", 1.0, 0).unwrap(),
        InsertScoreOutcome::Inserted
    );
    assert_eq!(
        cat.insert_cull_score(pid, "boundary-max", 10.0, 0).unwrap(),
        InsertScoreOutcome::Inserted
    );
}
```

---

## Theme R2-D — "R1-A" comment cross-reference doesn't match artifact's "Theme A" naming `MEDIUM`

**Agent flagging**: comment-analyzer (HIGH)

`crates/photohelper-catalog/src/row.rs:81` and `crates/photohelper-catalog/src/catalog.rs:505`

```rust
// row.rs:81
(R1-A fix: see `docs/code-reviews/session-04-catalog-migration-round1.md`).

// catalog.rs:505
// R1-A: store the raw path without calling std::fs::canonicalize.
```

The review artifact `session-04-catalog-migration-round1.md` uses **Theme** letters (Theme A, Theme B, …), not the `R1-A` pattern. Searching the artifact for "R1-A" returns zero matches. A future developer following the cross-reference will not find the cited finding.

**Remediation**: Change both references to match the artifact:
- `row.rs:81`: `(Theme-A fix: see docs/code-reviews/session-04-catalog-migration-round1.md § Theme A).`
- `catalog.rs:505`: `// Theme-A: store the raw path without calling std::fs::canonicalize.`

---

## Theme R2-E — `CullRow` doc says "raw" but path was canonicalized at ingest `MEDIUM`

**Agent flagging**: comment-analyzer (MEDIUM)

`crates/photohelper-catalog/src/row.rs:87-88`

```rust
/// Source path as stored at ingest time (raw, not re-canonicalized).
/// Callers should check existence before opening the file.
pub source_path: PathBuf,
```

At ingest, `photo.source_path()` returns an `AbsPath` (constructed via `AbsPath::canonicalize`) — i.e., `std::fs::canonicalize` has already been called. The stored string is therefore a canonicalized path. Calling it "raw" is misleading: it implies an unchecked, possibly relative user-supplied path, when it is actually a canonical path at the time of ingest.

The Theme-A fix removed the re-canonicalization at query time (to prevent a single missing file from aborting the batch), not the original canonicalization at ingest time.

**Remediation**:
```rust
/// Source path as canonicalized at ingest time, not re-validated at query time.
/// The file may have been moved or deleted since ingest; callers must check
/// existence before opening.
pub source_path: PathBuf,
```

---

## Theme R2-F — `CullRow` pub fields with no constructor; weakest-typed public struct in the codebase `MEDIUM`

**Agent flagging**: type-design-analyzer (MEDIUM)

`crates/photohelper-catalog/src/row.rs:82-90`

Every other public struct in the codebase (`Photo`, `AbsPath`, `NimaScore`, `PhotoId`, `RawExif`, `RawImage`) has private fields with validating constructors. `CullRow` is the outlier: fully public fields, no constructor, no validation. Any downstream crate can construct `CullRow` with arbitrary `photo_id` and garbage `source_path`. While no downstream consumer exists today, the type is `pub` and exported via `lib.rs`.

**Remediation**: Make both fields private with `pub fn` accessors; keep construction `pub(crate)` in `catalog.rs::unsuperseded_unscored_rows`:

```rust
pub struct CullRow {
    photo_id: PhotoId,
    source_path: PathBuf,
}

impl CullRow {
    pub fn photo_id(&self) -> PhotoId { self.photo_id }
    pub fn source_path(&self) -> &Path { &self.source_path }
}
```

This is a backward-compatible change since there are no external consumers yet.

---

## Theme R2-G — `is_finite()` guard redundant: `RangeInclusive::contains` already rejects NaN/Inf `LOW`

**Agent flagging**: code-simplifier (LOW)

`crates/photohelper-catalog/src/catalog.rs:559`

`(1.0_f64..=10.0_f64).contains(&score)` returns `false` for `f64::NAN` (NaN comparisons are always false), `f64::INFINITY` (outside range), and `f64::NEG_INFINITY` (outside range). The `!score.is_finite()` prefix is logically redundant.

**Remediation**: Simplify to `if !(1.0_f64..=10.0_f64).contains(&score)` and update the error message to remove "is not finite or".

---

## Theme R2-H — `PathBuf::from(&path_str)` borrows an owned String; should consume `LOW`

**Agent flagging**: code-simplifier (LOW)

`crates/photohelper-catalog/src/catalog.rs:510`

```rust
source_path: std::path::PathBuf::from(&path_str),
```

`path_str` is a destructured `String` from a tuple; it is not used after this line. `PathBuf::from(&path_str)` borrows and copies — `PathBuf::from(path_str)` would consume the buffer. Additionally, the fully-qualified `std::path::PathBuf` is unnecessary since `PathBuf` is already imported on line 7.

**Remediation**: `source_path: PathBuf::from(path_str),`

---

## Theme R2-I — R1-M (apply_v1_to_v2 closure extraction) not applied — acknowledged `LOW`

**Agents noting**: general-purpose (LOW), code-architect (LOW)

`crates/photohelper-catalog/src/catalog.rs:646-662`

The three identical `|e| Error::CatalogOpen { path: path.to_path_buf(), source: Box::new(e) }` closures from R1-M remain. Extraction was attempted but blocked by clippy's `needless_borrows_for_generic_args` lint. This is a low-impact cosmetic item with no behavioral effect. Explicitly deferring — no TD required at this severity.

---

## Theme R2-J — `PhotoRow.source_path: String` vs `CullRow.source_path: PathBuf` inconsistency `LOW`

**Agent flagging**: type-design-analyzer (LOW)

Both types project the same `source_path TEXT` SQLite column. `PhotoRow` stores it as `String`; `CullRow` as `PathBuf`. Inconsistent representation of the same underlying data.

**Remediation**: Track as a future cleanup; `String` is marginally more honest given the `to_string_lossy()` write path. Not blocking.

---

## Disposition summary

| Theme | Severity | Disposition |
|---|---|---|
| R2-A | HIGH | **MUST fix**: move cross-model assertion after scoring p1; add `any()` assertion |
| R2-B | HIGH | **MUST fix**: replace `InvalidParameterName` with `std::io::Error::new(InvalidInput, …)` |
| R2-C | MEDIUM | Fix: add `insert_cull_score_rejects_out_of_range_values` test |
| R2-D | MEDIUM | Fix: update two "R1-A" references to "Theme-A" |
| R2-E | MEDIUM | Fix: doc comment accuracy — "canonicalized at ingest, not re-validated at query time" |
| R2-F | MEDIUM | Fix: make `CullRow` fields private with accessors |
| R2-G | LOW | Fix inline: remove `is_finite()` guard |
| R2-H | LOW | Fix inline: `PathBuf::from(path_str)` (consume, drop fully-qualified prefix) |
| R2-I | LOW | Accept: defer — no behavioral impact |
| R2-J | LOW | Accept: defer — future cleanup |

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
    - finding_id: R2-A
      file: crates/photohelper-catalog/src/catalog.rs
      line: 981
      present: yes
      retain: yes
      reason: Cross-model assertion at line 982 runs before first insert_cull_score at line 990
      evidence_snippet: "// R1-D: scoring for model-A must not exclude from model-B work list."

    - finding_id: R2-B
      file: crates/photohelper-catalog/src/catalog.rs
      line: 562
      present: yes
      retain: yes
      reason: rusqlite::Error::InvalidParameterName used to carry domain range-validation message
      evidence_snippet: "rusqlite::Error::InvalidParameterName(format!("

    - finding_id: R2-C
      file: crates/photohelper-catalog/src/catalog.rs
      line: 559
      present: yes
      retain: yes
      reason: Range guard exists; no test exercises NaN/Inf/out-of-range values
      evidence_snippet: "if !score.is_finite() || !(1.0_f64..=10.0_f64).contains(&score) {"

    - finding_id: R2-D
      file: crates/photohelper-catalog/src/row.rs
      line: 81
      present: yes
      retain: yes-flag-for-human-triage
      reason: "R1-A fix" references artifact but artifact uses "Theme A" naming; citation broken
      evidence_snippet: "(R1-A fix: see `docs/code-reviews/session-04-catalog-migration-round1.md`)."

    - finding_id: R2-E
      file: crates/photohelper-catalog/src/row.rs
      line: 87
      present: yes
      retain: yes
      reason: "raw, not re-canonicalized" misleading; path was canonicalized at ingest
      evidence_snippet: "/// Source path as stored at ingest time (raw, not re-canonicalized)."

    - finding_id: R2-G
      file: crates/photohelper-catalog/src/catalog.rs
      line: 559
      present: yes
      retain: yes
      reason: is_finite() redundant — RangeInclusive::contains already returns false for NaN/Inf
      evidence_snippet: "if !score.is_finite() || !(1.0_f64..=10.0_f64).contains(&score) {"

    - finding_id: R2-H
      file: crates/photohelper-catalog/src/catalog.rs
      line: 510
      present: yes
      retain: yes
      reason: PathBuf::from(&path_str) borrows owned String unnecessarily
      evidence_snippet: "source_path: std::path::PathBuf::from(&path_str),"
```
