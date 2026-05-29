# Session 05 — Duplicate-detection pipeline, Plan Review Round 1

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

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 3 |
| HIGH | 13 |
| MEDIUM | 8 |
| LOW | 5 |

---

## Theme A — Stop-gap TD filing deferred to D5, violating CLAUDE.md policy [CRITICAL]

- [general-purpose]: CRITICAL — plan header (line 455) correctly states "All stop-gaps require companion TDs filed at the commit that introduces the stop-gap" but D5 (lines 427-429) lists TD-017/018/019 as D5 deliverables. D5 is after D3.
- [feature-dev:code-reviewer]: CRITICAL — CLAUDE.md "No Acceptable Trade-offs Policy": "every stop-gap commit MUST file a TD entry in TECH-DEBT.md." Stop-gaps S1 and S3 land in D3 (`dedup.rs`), S2 lands in D2b (`catalog.rs`). Filing in D5 means those commits are stop-gap-without-TD — a CRITICAL finding per `docs/quality-assurance.md`.

**Verification (F1)**: `present: yes` — CLAUDE.md line 124 verbatim: "every stop-gap commit MUST file a TD entry in TECH-DEBT.md"

**Remediation**: Move TD-017 filing to the D3 commit that introduces `threshold_cluster`. Move TD-018 filing to the D2b commit that introduces `insert_embedding`. Move TD-019 filing to the D3 commit that introduces `run_dedup` + `dup_clusters` schema. Each stop-gap commit MUST include: (1) TD entry in TECH-DEBT.md with binding trigger, (2) in-source `// TD-NNN: <description>` comment at the stop-gap site. D5's TECH-DEBT.md bullet should say "Verify TD-017/018/019 were filed at their introducing commits" — not file them.

---

## Theme B — Migration runner snippet is incorrect [CRITICAL]

- [general-purpose]: CRITICAL — plan D2a (lines 225-228) shows:
  ```rust
  1 => { apply_v1_to_v2(&conn)?; apply_v2_to_v3(&conn)?; }
  2 => { apply_v2_to_v3(&conn)?; }
  3 => {} // SCHEMA_VERSION
  ```
  Three errors: (1) `0 =>` arm is missing entirely; (2) function signature uses `&conn` but existing `apply_v1_to_v2` takes `&mut conn, catalog_path`; (3) SCHEMA_VERSION match uses a literal (`3 =>`) instead of the guard pattern used everywhere else.

**Verification (F2)**: `present: yes` — catalog.rs has four arms (0, 1, `v if v == SCHEMA_VERSION`, other); function signature is `apply_v1_to_v2(conn: &mut Connection, path: &Path)`.

**Remediation**: Replace the plan's snippet with the correct four-arm pattern:
```rust
0 => {
    // Fresh DB: INIT_SQL already ran (in transaction above);
    // chain both migrations for a fresh v3 baseline.
    apply_v1_to_v2(&mut conn, catalog_path)?;
    apply_v2_to_v3(&mut conn, catalog_path)?;
}
1 => { apply_v1_to_v2(&mut conn, catalog_path)?; apply_v2_to_v3(&mut conn, catalog_path)?; }
2 => { apply_v2_to_v3(&mut conn, catalog_path)?; }
v if v == SCHEMA_VERSION => {}
other => { return Err(Error::CatalogSchemaTooNew { found: other, expected: SCHEMA_VERSION }); }
```
Note: `apply_v2_to_v3` must accept `(&mut Connection, &Path)` matching the existing convention.

---

## Theme C — NaN norm passes `from_raw` validation check [CRITICAL]

- [pr-review-toolkit:silent-failure-hunter]: CRITICAL — plan D1a specifies norm check as "Rejects L2-norm < 0.99 or > 1.01" (line 107). IEEE 754 semantics: `NaN < 0.99` is `false`, `NaN > 1.01` is `false`. So `!(false || false)` = the condition is NOT triggered. A NaN-norm embedding passes `from_raw` and is stored in the catalog. During `threshold_cluster`, `cosine_similarity` computes a dot product involving NaN → returns NaN. `NaN >= 0.95` is `false` → the photo silently never clusters with anything.

**Verification (F5)**: `present: yes` — plan line 107 says "Rejects L2-norm < 0.99 or > 1.01" with no is_finite() guard.

**Remediation**: Add a finite-check BEFORE the range check in the `from_raw` spec:
```
- Rejects if norm is not finite (NaN or Inf) → Error::EmbeddingNotNormalized { norm }
- Rejects if norm < 0.99 or > 1.01 → Error::EmbeddingNotNormalized { norm }
```
The `l2_norm` call in `MobileClip::embed` should also pre-check for a zero vector (norm < f32::EPSILON) and return `Error::EmbeddingZeroVector` before attempting division, which would produce NaN.

---

## Theme D — `InsertClusterOutcome::AlreadyAssigned` is dead code [HIGH]

- [feature-dev:code-reviewer]: HIGH — `INSERT OR REPLACE INTO dup_clusters` (line 273) always deletes conflicting row + inserts fresh. `changes()` is always 1. `AlreadyAssigned` can never be returned.
- [pr-review-toolkit:type-design-analyzer]: HIGH — compares with `InsertScoreOutcome` which correctly uses `INSERT OR IGNORE` + `changes()`. The plan conflates two incompatible SQL strategies.
- [pr-review-toolkit:pr-test-analyzer]: HIGH — no test for `AlreadyAssigned` because it can never be constructed.
- [pr-review-toolkit:code-simplifier]: HIGH — dead enum variant accretes tech debt from day one.

**Verification (F3)**: `present: yes` — plan line 273 verbatim says `INSERT OR REPLACE INTO dup_clusters`.

**Remediation**: Change `insert_dup_cluster` return type to `Result<(), Error>`. Remove `InsertClusterOutcome` entirely. The plan already states "re-clustering a photo replaces the old assignment" (line 274) — unconditional overwrite, no discrimination needed. Update D2b and the D2b test table accordingly.

---

## Theme E — Test count arithmetic inconsistency (+15/158 vs +20/163) [HIGH]

- [general-purpose]: HIGH — line 30 says "target ≥ 158 (+15 minimum)"; test plan table (lines 441-448) sums to 4+3+3+5+3+2+0 = 20, and line 449 says "≥ 163 total (143 baseline + 20)".
- [pr-review-toolkit:comment-analyzer]: CRITICAL (downgraded to HIGH since arithmetic-only) — one count is provably wrong.
- [pr-review-toolkit:pr-test-analyzer]: CRITICAL (downgraded) — the test plan table is the authoritative calculation; line 30 is stale.

**Verification (F8)**: `present: yes` — SESSION-STATE.md: "just ci GREEN (143 tests — no code changes from main)".

**Remediation**: Change plan line 30 from "target ≥ 158 (+15 minimum)" to "target ≥ 163 (+20 minimum)". The test plan table is the authoritative source.

---

## Theme F — Clustering transitivity semantics not documented [HIGH]

- [feature-dev:code-architect]: HIGH — union-find produces connected components via transitive closure. If sim(A,B) ≥ 0.95 and sim(B,C) ≥ 0.95 but sim(A,C) = 0.70, A/B/C are in the same cluster despite A and C being visually dissimilar. This is a design decision (transitivity vs. clique-only clustering) that the plan treats as an implementation detail.

**Remediation**: Add a paragraph to D3 and to `docs/decisions/0003-catalog-schema-v3.md`: "Union-find clusters are connected components: A~B and B~C → {A,B,C} in one cluster, even if sim(A,C) < threshold. This is the v0.1 choice — near-duplicate chains are transitive by design. TD-017's DBSCAN upgrade addresses this if transitive chaining produces user-confusing clusters in practice."

---

## Theme G — Phase 2 `insert_dup_cluster` errors silently swallowed [HIGH]

- [pr-review-toolkit:silent-failure-hunter]: HIGH — Phase 2 pseudocode (lines 341-342) shows `for each assignment: Catalog::insert_dup_cluster(...)` with no error handling. Contrast with Phase 1 where every Result is explicitly matched. A FK violation or I/O error in Phase 2 loses the cluster assignment silently.

**Remediation**: Add a `cluster_write_failed: AtomicU64` counter to `DedupeStats`. Phase 2 pseudocode must show:
```
for each (photo_id, cluster_id) in assignments:
    match Catalog::insert_dup_cluster(...):
        Ok(()) => {}
        Err(e) => warn!(...); stats.cluster_write_failed.fetch_add(1, ...)
```
Include `cluster_write_failed` in the summary line and in `--strict` exit logic.

---

## Theme H — `similarity_threshold` f32 has no range validation [HIGH]

- [feature-dev:code-reviewer]: HIGH — `DedupeArgs.similarity_threshold: f32` (line 301) has `default_value_t = 0.95_f32` but no `value_parser`. Valid cosine similarity is in `(-1.0, 1.0]`; meaningful dedup threshold is `(0.0, 1.0]`. Values > 1.0 produce all singletons silently. Values ≤ 0.0 produce one cluster. NaN produces all singletons silently. The existing `--threads` arg uses `clap::value_parser!(u32).range(1..=1024)` as the established pattern.

**Remediation**: Add to `DedupeArgs`:
```rust
#[arg(long, default_value_t = 0.95_f32, value_parser = parse_similarity_threshold)]
similarity_threshold: f32,
```
with `fn parse_similarity_threshold(s: &str) -> Result<f32, String>` that rejects NaN, infinity, and values outside `(0.0, 1.0]`. Spec the error message.

---

## Theme I — `verify-model-sha256.sh` hardcoded for NIMA; plan claims it covers both models [HIGH]

- [feature-dev:code-reviewer]: HIGH — plan line 126 says "just verify-model-sha256 CI gate covers BOTH models (NIMA + MobileCLIP)". The script is hardcoded: `MODEL="crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx"` and greps for `[nima_mobilenet_aesthetic]` section only. No loop over manifest sections.

**Verification (F4)**: `present: yes` — `MODEL="crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx"` is hardcoded.

**Remediation**: Add a D1b sub-task: "Extend `scripts/verify-model-sha256.sh` to iterate over all `[section_name]` headers in `manifest.toml`, extract `filename` and `sha256` from each section, and verify each. Replace the current single-model hardcode with a loop." Specify ~20-30 LoC in the script. The plan must not claim the existing gate covers MobileCLIP until this extension is specified and implemented.

---

## Theme J — `already_embedded` counter semantics incorrect [HIGH]

- [feature-dev:code-reviewer]: HIGH — plan lines 335-336 say "AlreadyEmbedded: increment already_embedded (catalog_inconsistency if this was in the unembedded_rows list)". Within a single dedup process, `unembedded_rows` returns unique rows (photos.id is PK). No two rayon workers can race on the same photo. If `insert_embedding` returns `AlreadyEmbedded` for a photo in the list, it is ALWAYS an inter-process race (another dedup process ran concurrently) → always `catalog_inconsistency`, never `already_embedded`.
- [pr-review-toolkit:silent-failure-hunter]: HIGH — same finding; the "data race between workers" framing is incorrect — intra-process races cannot occur.

**Verification (F10)**: `present: yes` — plan lines 335-336 verbatim confirmed.

**Remediation**: Remove `already_embedded` from `DedupeStats`. Change the `AlreadyEmbedded` handler to always increment `catalog_inconsistency` with `WARN!("insert returned AlreadyEmbedded for a photo in the work list — inter-process race?")`. If a "photos skipped because already embedded at start" count is desired for UX, compute it as `total_photos - unembedded_rows.len()` as a local before the walk, not as a per-photo counter.

---

## Theme K — HANDOFF_REPORT.md Checkpoint 13 collision [HIGH]

- [general-purpose]: HIGH — D5 (line 424) says "HANDOFF_REPORT.md Checkpoint 13". Checkpoint 13 was already written during the context-refresh pause (`## Checkpoint 13 — session 05 PAUSED for context refresh`).
- [pr-review-toolkit:comment-analyzer]: HIGH — append-only audit trail would have two Checkpoint 13 blocks.

**Verification (F6)**: `present: yes` — "## Checkpoint 13 — session 05 PAUSED for context refresh" exists at HANDOFF_REPORT.md line 924.

**Remediation**: Change D5 line 424 to "HANDOFF_REPORT.md Checkpoint 14".

---

## Theme L — D5 TD-016 closure conditional inconsistency [HIGH]

- [pr-review-toolkit:comment-analyzer]: HIGH — D5 (line 425) says "TD-016 → Closed (if D4 fires)" with a conditional, but the rest of the plan is unambiguous: "D3 adds heartbeat as third consumer → D4 MUST land this session" (D3 line 370, D4 line 390 body). The conditional in D5 contradicts the unconditional language everywhere else and could leave TD-016 open if someone reads D5 as a checklist.

**Remediation**: Change D5 line 425 to "TD-016 → Closed (unconditional — D3 requires a heartbeat; D4 is mandatory)." Also: the D4 header label "CONDITIONAL on D3 trigger" (line 390) should become "Expected: D3 adds heartbeat (third consumer); D4 is mandatory" to match.

---

## Theme M — Missing `derive_failed` counter + `content_changed` skip behavior unspecified [HIGH]

- [pr-review-toolkit:silent-failure-hunter]: HIGH — plan's `DedupeStats` (line 307-309) omits `derive_failed`. The `cull.rs` pipeline has this counter (cull.rs line 44). If `PhotoId::derive(path)` fails (I/O error reading the file for hashing), the plan has no counter and no specified behavior. The `content_changed` step (line 331) also doesn't specify the `return`/skip behavior — `cull.rs` pattern (lines 149-167) shows an explicit `return` on content-changed detection.

**Remediation**: (a) Add `derive_failed: AtomicU64` to `DedupeStats`. (b) In Phase 1 pseudocode, make explicit:
```
2. PhotoId::derive(path) → if Err: derive_failed++, continue
   → if Ok but differs from stored: content_changed++, continue
   → if Ok and matches: proceed to step 3
```

---

## Theme N — D1a tests (4) insufficient for 6 defined behaviors [HIGH]

- [pr-review-toolkit:pr-test-analyzer]: HIGH — D1a defines 6 behaviors: from_raw happy, from_raw empty rejection, from_raw norm rejection, cosine_similarity happy, cosine_similarity dim-mismatch error, from_f32_le_bytes round-trip. Test table allocates 4. `from_f32_le_bytes` round-trip is the catalog's deserialization path — a bug here corrupts every read embedding. The dim-mismatch path guards against cross-model clustering.

**Remediation**: Allocate 6 D1a tests, explicitly naming: (a) `from_f32_le_bytes_round_trip` — serialize + deserialize + assert `cosine_similarity == 1.0`; (b) `cosine_similarity_dim_mismatch_returns_error`; (c) add `from_f32_le_bytes_with_corrupt_bytes` (byte slice length not multiple of 4). Update test table to 6 + adjust total to ≥ 165.

---

## Theme O — "Golden-band check" undefined for a 512-dim vector [HIGH]

- [pr-review-toolkit:pr-test-analyzer]: HIGH — D1c test table (line 442) says "golden-band check" without defining what is checked. NIMA golden-band is a scalar range (score ∈ [X, Y]). MobileCLIP output is a 512-dim vector — a scalar "band" concept does not apply.

**Remediation**: Specify the golden check concretely following DN-025's precedent:
- On apple-silicon (CI and developer machines): commit a golden embedding vector for each CC0 CR3 fixture; assert `cosine_similarity(computed, golden) ≥ 1.0 - 1e-3`.
- On Linux x86_64 CI: assert `cosine_similarity(computed, golden) ≥ 0.98` (wider band for cross-arch f32 differences).
- File DN-027 (MobileCLIP cross-platform embedding tolerance for clustering) in D5 (committed, not optional).

---

## Theme P — Union-find path compression missing; O(n²) claim is wrong [MEDIUM]

- [feature-dev:code-architect]: MEDIUM — `threshold_cluster` spec (lines 354-363) says "Algorithm (union-find)" and claims "O(n²) cosine similarity comparisons." Without path compression + union-by-rank, each `union` call in the O(n²) pair loop calls `find` twice, where `find` is O(n) worst-case → total O(n³).

**Remediation**: Add to the `threshold_cluster` spec: "Union-find uses path compression in `find` and union-by-rank in `union` (standard optimizations; ~5 LoC). This ensures `find` is amortized O(α(n)) per call, keeping total complexity at O(n²) as claimed."

---

## Theme Q — Memory budget for `all_embeddings_for_model` undocumented [MEDIUM]

- [feature-dev:code-architect]: MEDIUM — plan warns at n > 5K for compute but not memory. At n=100K × 512 dims × 4 bytes ≈ 200 MB loaded into a single Vec.

**Remediation**: Extend the `threshold_cluster` warning to include memory: "dedup: large corpus ({n} photos, ~{n × dim × 4 / 1_048_576} MiB embedding memory); clustering may take >60s." Add to TD-017 description: "O(n²) time + O(n × dim) memory; at n=100K × 512 dims, requires ~200 MiB RAM."

---

## Theme R — Missing `photohelper-dedup.sh` wrapper script [MEDIUM]

- [feature-dev:code-reviewer]: MEDIUM — established pattern (from HANDOFF_REPORT.md Checkpoint 12): every implemented subcommand has a shell wrapper (`photohelper-ingest.sh`, `photohelper-cull.sh`). `dedup` needs `PHOTOHELPER_MODEL_DIR` for both NIMA and MobileCLIP. Without the wrapper, users must manually construct the env-var + invocation.

**Remediation**: Add to D3 (or D5 docs): "Add `scripts/photohelper-dedup.sh` following the `photohelper-cull.sh` pattern: export `PHOTOHELPER_MODEL_DIR`, accept `--catalog` + `--similarity-threshold` passthrough, forward to `cargo run --release`. Add `just dedup` recipe."

---

## Theme S — Exit code label wrong + missing EX_USAGE case [MEDIUM]

- [feature-dev:code-reviewer]: MEDIUM — plan line 375: "1 (EX_NOPERM): --strict + any per-photo error counter > 0". In `main.rs`, exit code 1 is `EX_STRICT_FAIL`, not `EX_NOPERM` (`EX_NOPERM = 77`). Also: `cull.rs` has an `EX_USAGE` (64) case for "walked > 0 but nothing useful happened." The dedup plan specifies no equivalent.

**Verification (F7)**: main.rs confirms `EX_STRICT_FAIL = 1` and `EX_NOPERM = 77`. Plan line 375 mislabels exit code 1.

**Remediation**: Fix line 375 to "1 (EX_STRICT_FAIL)". Add EX_USAGE (64) for the case where `unembedded_rows` returned rows but `embedded == 0 && all_per_photo_errors == 0` (likely wrong catalog path), matching the cull pattern.

---

## Theme T — TD-010 heartbeat-death test seam unspecified after D4 extraction [MEDIUM]

- [pr-review-toolkit:pr-test-analyzer]: MEDIUM — D4 (lines 411-415) says TD-010's 2 remaining tests "MUST also land in D4" but does not specify how `HeartbeatDeathTrigger` (from `photohelper-test-helpers`) interfaces with the new `heartbeat.rs` module. After extraction, the test seam location and API are undefined.

**Remediation**: Specify in D4: "heartbeat.rs exposes `#[cfg(test)] pub(crate) fn spawn_death_trigger_handle(stop: Arc<HeartbeatStop>) -> JoinHandle<()>` that spawns a thread programmed to panic after one tick — this is the test seam `HeartbeatDeathTrigger` uses. Alternatively, `run_ingest` accepts an injectable `heartbeat_factory: impl Fn(...) -> HeartbeatHandle` in test builds."

---

## Theme U — Missing integration tests (threshold boundary, empty catalog) [MEDIUM]

- [pr-review-toolkit:pr-test-analyzer]: MEDIUM — D3 test table (lines 379-386) lists 3 tests. Missing: (a) threshold=1.0 → all singletons (validates clustering algorithm boundary); (b) empty catalog → `walked: 0`, exit 0 (validates early-return path); (c) summary-line format assertion for `clusters-found` and `singletons`.

**Remediation**: Add 2 mandatory integration tests: `dedup_threshold_one_produces_all_singletons` and `dedup_empty_catalog_exits_zero_walked_zero`. Bump D3 test count to 5. Add summary-line assertion to the e2e test for `clusters-found` and `singletons`.

---

## Theme V — `MOBILECLIP_MODEL_MANIFEST_KEY` breaks established naming convention [MEDIUM]

- [pr-review-toolkit:code-simplifier]: MEDIUM — NIMA uses `MODEL_MANIFEST_NAME` (e.g., `"nima_mobilenet_aesthetic"`). Plan uses `MOBILECLIP_MODEL_MANIFEST_KEY` — the word "KEY" implies a sub-key within a shared section, not a section name. Does MobileCLIP share the NIMA manifest.toml section or get its own?

**Remediation**: Rename to `MOBILECLIP_MODEL_MANIFEST_NAME` to match the NIMA convention. MobileCLIP gets its own section `[mobileclip_s1_v1]` in `manifest.toml` (section name = filename stem). Clarify this explicitly in D1b.

---

## Theme W — Missing heartbeat `is_finished()` guard in dedup pipeline [MEDIUM]

- [pr-review-toolkit:silent-failure-hunter]: MEDIUM — plan D3 pipeline (line 337): "heartbeat.signal(); heartbeat_handle.join();". The existing `cull.rs` pattern (lines 210-216): `if heartbeat_handle.is_finished() { tracing::warn!(...); } stop.signal(); let _ = heartbeat_handle.join();`.

**Verification (F9)**: `present: yes` — cull.rs lines 210-216 verbatim confirmed.

**Remediation**: Specify the complete shutdown sequence in D3 (and D4's heartbeat.rs extraction should codify it once): `is_finished()` WARN check → `stop.signal()` → `heartbeat_handle.join()` (discard result — already surfaced by WARN). D4's extracted module should make this the canonical sequence.

---

## Theme X — D0 probe script commitment unclear [LOW]

- [pr-review-toolkit:comment-analyzer]: LOW — D0 sequencing (line 88) says "D0-probe-script → D0-ANL-003-commit" implying a committed artifact. D1b (line 130) says "if conversion script is needed … commit it." These may refer to different scripts (probe vs. conversion) but are not clearly distinguished.

**Remediation**: Clarify in D0: the probe script is session-local (not committed) unless it doubles as the conversion script (like `convert-nima-to-onnx.sh` did in session 04). D1b's "if needed" conditional is about the conversion script specifically.

---

## Theme Y — `EmbeddingRow` vs `CullRow` duplication needs explicit justification [LOW]

- [pr-review-toolkit:type-design-analyzer]: LOW — both `{ source_path: PathBuf, photo_id: PhotoId }` with private fields. Plan says "(Same shape as CullRow)" but doesn't say why separate types are kept.

**Remediation**: Add one sentence in D2b: "Intentionally distinct from CullRow: the types may diverge (EmbeddingRow may gain dim/model_slug; CullRow may gain existing_score). Two identical DTOs is below the three-instance abstraction threshold per CLAUDE.md."

---

## Theme Z — `clusters_found`/`singletons` computed ad-hoc outside `DedupeStats` [LOW]

- [pr-review-toolkit:code-simplifier]: LOW — DedupeStats (8 AtomicU64 fields) excludes `clusters_found` and `singletons` despite printing them in the summary. Breaks the pattern where the stats struct owns all summary output.

**Remediation**: Add `clusters_found: u64` and `singletons: u64` as plain (non-atomic, post-Phase-2) fields on a `DedupeClusterResult` companion struct returned by `threshold_cluster`, or add them to `DedupeStats` as post-Phase-2 fields. Either way, the summary printer should read all output from one place.

---

## Theme AA — Per-row `similarity_threshold` column adds schema weight for no v0.1 consumer [LOW]

- [pr-review-toolkit:code-simplifier]: LOW — storing `similarity_threshold REAL NOT NULL` per-row in `dup_clusters` is redundant when all rows in a run share the same threshold. When TD-019 lands, the column moves to `dedup_runs` anyway, requiring a v3→v4 migration to drop it. The simpler choice is to omit the column from v3.

**Remediation**: Consider dropping `similarity_threshold` from `dup_clusters` schema for v0.1. Threshold is re-readable from the CLI arg or session log until TD-019 lands. Saves one column, one parameter per insert, and one future migration.

---

## Disposition summary

| Theme | Severity | What changes in plan |
|---|---|---|
| A — TD filing timing | CRITICAL | Move TD-017/018/019 filing to introducing commits |
| B — Migration runner snippet | CRITICAL | Correct to 4-arm pattern with proper signatures |
| C — NaN norm validation | CRITICAL | Add `is_finite()` guard + EmbeddingZeroVector |
| D — InsertClusterOutcome dead code | HIGH | Change to `Result<()>`; remove enum |
| E — Test count arithmetic | HIGH | Change line 30 to +20/≥163 |
| F — Transitivity undocumented | HIGH | Add design-decision paragraph to D3 + decisions/0003 |
| G — Phase 2 error swallowing | HIGH | Add cluster_write_failed counter; spec error handling |
| H — similarity_threshold no validation | HIGH | Add value_parser with range guard |
| I — verify-model-sha256.sh hardcoded | HIGH | Add D1b sub-task: extend script to loop over manifest |
| J — already_embedded counter wrong | HIGH | Remove counter; AlreadyEmbedded always = catalog_inconsistency |
| K — Checkpoint 13 collision | HIGH | Change D5 to Checkpoint 14 |
| L — D5 TD-016 conditional | HIGH | Remove "if D4 fires" qualifier |
| M — Missing derive_failed + skip spec | HIGH | Add counter + explicit pseudocode |
| N — D1a tests insufficient | HIGH | 6 tests not 4; add round-trip + dim-mismatch |
| O — Golden-band undefined | HIGH | Specify as cosine_similarity vs golden vector |
| P — Path compression / O(n³) | MEDIUM | Add path compression to spec |
| Q — Memory budget undocumented | MEDIUM | Extend warning + TD-017 |
| R — No dedup.sh wrapper | MEDIUM | Add to D3/D5 |
| S — Exit code label + EX_USAGE | MEDIUM | Fix EX_NOPERM label; add EX_USAGE case |
| T — TD-010 test seam unspecified | MEDIUM | Specify seam design in D4 |
| U — Missing integration tests | MEDIUM | Add 2 tests + summary-line assertion |
| V — MANIFEST_KEY naming | MEDIUM | Rename to MANIFEST_NAME; clarify layout |
| W — Heartbeat is_finished() missing | MEDIUM | Spec complete shutdown sequence |
| X — D0 probe script ambiguity | LOW | Clarify probe vs. conversion distinction |
| Y — EmbeddingRow duplication | LOW | One-sentence justification |
| Z — clusters_found outside stats | LOW | Add to DedupeStats or companion struct |
| AA — similarity_threshold per-row | LOW | Consider dropping from v3 schema |

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
  details:
    - finding_id: F1
      file: CLAUDE.md
      line: 124
      present: yes
      retain: yes
      reason: "CLAUDE.md line 124 verbatim confirms the policy"
      evidence_snippet: "every stop-gap commit MUST file a TD entry in TECH-DEBT.md"
    - finding_id: F2
      file: crates/photohelper-catalog/src/catalog.rs
      line: 241
      present: yes
      retain: yes
      reason: "Confirmed: 4-arm match block; apply_v1_to_v2 takes &mut Connection + path"
      evidence_snippet: "0 => { ... apply_v1_to_v2(&mut conn, catalog_path)?; } 1 => { apply_v1_to_v2(&mut conn, catalog_path)?; } v if v == SCHEMA_VERSION => {} other => {"
    - finding_id: F3
      file: docs/plans/session-05.md
      line: 273
      present: yes
      retain: yes
      reason: "Plan verbatim says INSERT OR REPLACE INTO dup_clusters"
      evidence_snippet: "INSERT OR REPLACE INTO dup_clusters"
    - finding_id: F4
      file: scripts/verify-model-sha256.sh
      line: 11
      present: yes
      retain: yes
      reason: "MODEL hardcoded to nima_mobilenet_aesthetic.onnx; no loop"
      evidence_snippet: "MODEL=\"crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx\""
    - finding_id: F5
      file: docs/plans/session-05.md
      line: 107
      present: yes
      retain: yes
      reason: "No is_finite() guard specified; NaN passes the comparison"
      evidence_snippet: "Rejects L2-norm < 0.99 or > 1.01"
    - finding_id: F6
      file: HANDOFF_REPORT.md
      line: 924
      present: yes
      retain: yes
      reason: "Checkpoint 13 header already exists"
      evidence_snippet: "## Checkpoint 13 — session 05 PAUSED for context refresh"
    - finding_id: F7
      file: docs/plans/session-05.md
      line: 375
      present: yes
      retain: yes
      reason: "Plan mislabels exit code 1 as EX_NOPERM; main.rs has EX_STRICT_FAIL=1"
      evidence_snippet: "1 (EX_NOPERM): `--strict` + any per-photo error counter > 0"
    - finding_id: F8
      file: SESSION-STATE.md
      line: 27
      present: yes
      retain: yes
      reason: "143 test baseline confirmed"
      evidence_snippet: "just ci GREEN (143 tests — no code changes from main)"
    - finding_id: F9
      file: crates/photohelper-cli/src/commands/cull.rs
      line: 210
      present: yes
      retain: yes
      reason: "cull.rs has is_finished() WARN before signal+join; plan omits this"
      evidence_snippet: "if heartbeat_handle.is_finished() { tracing::warn!(\"heartbeat thread died before end-of-cull"
    - finding_id: F10
      file: docs/plans/session-05.md
      line: 335
      present: yes
      retain: yes
      reason: "Plan lines 335-336 confirmed verbatim"
      evidence_snippet: "AlreadyEmbedded: increment already_embedded (catalog_inconsistency if this was in the unembedded_rows list"
```
