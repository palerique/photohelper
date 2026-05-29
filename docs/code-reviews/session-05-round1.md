# Session 05 — dedup-mobileclip, Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent session); agents pinned to opus"
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

| Severity | Count |
|---|---|
| CRITICAL | 2 |
| HIGH | 3 |
| MEDIUM | 9 |
| LOW | 1 |
| **Total** | **15** |

---

## Theme A — `dim` consistency: insert validates range but not byte-length; caller discards dim entirely [CRITICAL]

**Agents**: code-architect, code-reviewer, type-design-analyzer, silent-failure-hunter, comment-analyzer, pr-test-analyzer

`insert_embedding` (catalog.rs:666) validates that `dim ∈ [1, 65536]` but never checks that
`embedding_bytes.len() == dim * 4`. A caller could pass `dim=512` with a 256-float byte slice and
the row would be stored with an inconsistent `dim` column. The `INSERT OR IGNORE` idiom (correctly
guarded by the Rust-level range check) silently swallows CHECK constraint violations — this was
discovered in D2c — but the byte-length consistency gap remains unfilled.

Compound issue: the docstring on `all_embeddings_for_model` (catalog.rs:706-708) states "the caller
validates `dim == bytes.len() / 4` to catch corruption early." The actual caller in `dedup.rs:257`
destructures as `|(pid, bytes, _dim)|` — `_dim` is discarded with the underscore prefix and no
validation occurs. This means the docstring documents an invariant that does not exist in code,
creating a false sense of safety.

**Verified (9th agent)**: present=yes at catalog.rs:666, dedup.rs:257; drifted at catalog.rs:707
(actual lines 706-708). evidence snippets match source.

**Remediation**:
1. Add to `insert_embedding` after line 672: `if embedding_bytes.len() != dim * 4 { return Err(Error::CatalogInsert { ... "dim*4 != byte length" ... }); }`
2. In `dedup.rs:257`, destructure `dim` (not `_dim`) and add: `if bytes.len() != dim * 4 { tracing::warn!(...); return None; }`
3. Update `all_embeddings_for_model` docstring to match whichever of (1)/(2) is implemented — the writer-side guard in (1) is the stronger fix.

---

## Theme B — `threshold_cluster` has zero unit tests; plan promised 5 [CRITICAL]

**Agents**: pr-test-analyzer (primary), cross-cutting consistency

`threshold_cluster` (dedup.rs:352) is the core of the dedup pipeline — it implements union-find
with path compression + union-by-rank over `n*(n-1)/2` cosine-similarity pairs. There is not a
single direct unit test of this function. The session plan's test-plan-summary table (session-05.md)
specifies "D3 dedup | 5 |" tests; only 3 integration tests were delivered (end-to-end, idempotency,
strict-mode). The plan's threshold-boundary test (threshold=1.0 → all singletons) and
empty-catalog test are missing entirely.

The only coverage is indirect via `dedup_end_to_end_embeds_and_clusters_cc0_fixtures`, which:
- Requires Git LFS fixtures (skipped silently without them)
- Only verifies row counts in `dup_clusters`, not cluster assignment correctness

Unverified edge cases with regression potential: empty input (n=0), single element (n=1),
threshold=1.0 (all singletons at exact-match only), threshold near similarity boundary, all
identical embeddings (one cluster), all orthogonal (all singletons).

**Verified**: present=yes at dedup.rs:352; no `#[cfg(test)]` module in file (409 lines, confirmed).

**Remediation**: Add a `#[cfg(test)] mod tests` block in `dedup.rs` with at minimum:
- `threshold_cluster_empty_input` — n=0 → cluster_count=0, singleton_count=0
- `threshold_cluster_single` — n=1 → 1 singleton
- `threshold_cluster_two_identical_at_threshold_1_0` — two 1.0-sim embeddings at threshold=1.0 → same cluster
- `threshold_cluster_two_orthogonal` — two orthogonal embeddings → two singletons
- `threshold_cluster_boundary_below_threshold` — two embeddings with sim just below threshold → two singletons

Also fix the end-to-end test to assert cluster assignment relationships, not just row counts.

---

## Theme C — Module-scoped `thread_local! SESS` allows cross-contamination if multiple `MobileClip` instances exist [HIGH]

**Agents**: type-design-analyzer (primary)

`mobileclip.rs:39-44` defines `SESS` as a module-level `thread_local!`:
```rust
thread_local! {
    static SESS: RefCell<Option<ort::session::Session>> = const { RefCell::new(None) };
}
```
The Session is not tied to any `MobileClip` instance. If two `MobileClip` instances are created
with different model bytes (e.g., different model versions, or a wrong model passed by mistake),
a thread that already called `embed()` with instance A would reuse A's Session for instance B,
producing embeddings from the wrong model. The `if guard.is_none()` check in `embed()` only
fires on first use — it never re-validates that `self.bytes` matches the Session's model.

Current single-instance usage (one `MobileClip` per `run_dedup` invocation) means this is
unexploitable today. However, nothing in the type system or runtime prevents the bug.

**Verified**: present=yes at mobileclip.rs:39.

**Remediation** (choose one):
1. **Minimal**: Add a `static CLIP_INSTANCE_COUNT: AtomicU8 = AtomicU8::new(0)` in `mod.rs` that asserts `count <= 1` in `MobileClip::new`, preventing multiple instances.
2. **Structural**: Store a session-identity fingerprint (e.g., first 8 bytes of SHA-256 of `self.bytes`) in a second thread-local alongside `SESS`; verify on every `embed()` call and reconstruct Session on fingerprint mismatch.

---

## Theme D — `cosine_similarity` error silently discarded in clustering loop [HIGH]

**Agents**: silent-failure-hunter (primary), code-architect, code-reviewer

`dedup.rs:368-374`:
```rust
// cosine_similarity returns Err only for dim-mismatch (same model → same dim).
if let Ok(sim) = embeddings[i].1.cosine_similarity(&embeddings[j].1) {
    if sim >= threshold {
        uf_union(&mut parent, &mut rank, i, j);
    }
}
```
When `cosine_similarity` returns `Err(EmbeddingDimMismatch)`, the error is silently discarded with
`if let Ok`. No log, no counter increment, no user feedback. The pair is silently treated as
"not similar enough to cluster." A dim mismatch within a single model slug indicates data corruption
(two embeddings stored with different dims under the same model). The comment documents this
shouldn't happen, but if it does, the silent skip means a photo silently becomes a "singleton" with
no diagnostic trace.

**Verified**: present=yes at dedup.rs:369 (direct Read confirms).

**Remediation**: Replace `if let Ok(sim)` with an explicit match:
```rust
match embeddings[i].1.cosine_similarity(&embeddings[j].1) {
    Ok(sim) if sim >= threshold => { uf_union(&mut parent, &mut rank, i, j); }
    Ok(_) => {} // below threshold
    Err(e) => {
        tracing::error!(i = %embeddings[i].0, j = %embeddings[j].0, error = %e,
            "cosine_similarity failed (embedding dim mismatch indicates DB corruption)");
    }
}
```
Optionally also add a pre-loop `debug_assert!` that all embeddings have equal dim, as the
`cosine_similarity` docstring (embedding.rs:61-63) recommends.

---

## Theme E — Phase 2 empty/corrupt embedding set is silent; no counter for deserialization failures [HIGH]

**Agents**: silent-failure-hunter (primary)

Two sub-issues, same affected region:

**E1 — Empty Phase 2 set silent exit**: `dedup.rs:249-251`:
```rust
let (clusters_found, singletons) = if all_embeddings.len() < 2 {
    // 0 or 1 embedding: nothing to cluster.
    (0_usize, all_embeddings.len())
```
When Phase 1 fails for every photo (all `infer_failed`), `all_embeddings_for_model` returns an
empty vec, and Phase 2 exits with `clusters_found=0, singletons=0, exit=0`. The user sees a success
summary that looks identical to "empty catalog" — no log explains that embedding failures caused
clustering to be skipped.

**E2 — Corrupt embeddings filtered without counter**: `dedup.rs:257-261`:
```rust
Err(e) => {
    tracing::warn!(error = %e, "skipping corrupt embedding during clustering");
    None  // no counter incremented
}
```
When `from_f32_le_bytes` fails on a catalog row, the embedding is dropped with a `warn!` but no
`DedupeStats` counter is incremented. The resulting cluster statistics are computed from a reduced
set with no summary-line visibility.

**Verified**: E1 at dedup.rs:249 (present=yes), E2 at dedup.rs:257 (present=yes).

**Remediation**:
- E1: Add `if all_embeddings.is_empty() { tracing::info!("no embeddings available for model {CLIP_MODEL_SLUG}; skipping clustering"); }` before the `if all_embeddings.len() < 2` check.
- E2: Add a `deserialize_failed: AtomicU64` field to `DedupeStats` and increment it in the `Err` arm; include in `summary_line()`.

---

## Theme F — `catalog_inconsistency` counter conflates benign race with real catalog failure [MEDIUM]

**Agents**: type-design-analyzer (primary), silent-failure-hunter

`dedup.rs:219-232` increments `catalog_inconsistency` for both:
- `AlreadyEmbedded` (line 226) — a benign inter-process race (another writer won)
- `Err(e)` from `insert_embedding` (line 231) — a real catalog failure (disk full, FK violation, etc.)

These have different operational meanings. Under `--strict`, both trigger `EX_STRICT_FAIL`, so a
benign race between two concurrent dedup runs would cause a non-zero exit in CI. An operator seeing
`catalog-inconsistency: 5` cannot tell if they have 5 harmless races or 5 real data-loss events.

**Verified**: present=yes at dedup.rs:219.

**Remediation**: Split into two counters:
- `already_embedded: AtomicU64` — benign race; excluded from `all_per_photo_errors` and `--strict` check
- `catalog_insert_failed: AtomicU64` — real error; included in `all_per_photo_errors`

Update `summary_line()` to label both distinctly.

---

## Theme G — `insert_dup_cluster` with `INSERT OR REPLACE` silently overwrites previous cluster assignment [MEDIUM]

**Agents**: silent-failure-hunter (primary), comment-analyzer

`catalog.rs:793` (SQL inside function starting at 777):
```sql
INSERT OR REPLACE INTO dup_clusters
(photo_id, model_slug, cluster_id, similarity_threshold, clustered_at_unix_seconds)
VALUES (?1, ?2, ?3, ?4, ?5)
```
`INSERT OR REPLACE` is semantically `DELETE + INSERT`. When the PRIMARY KEY `(photo_id, model_slug)`
conflicts, the previous cluster assignment is destroyed without any log, outcome enum, or audit
record. Unlike `insert_embedding` (returns `AlreadyEmbedded`) and `insert_cull_score` (returns
`AlreadyScored`), `insert_dup_cluster` gives the caller zero visibility into whether it inserted
new data or overwrote existing data.

A user running `photohelper dedup --similarity-threshold 0.95` followed by `0.80` silently loses
the 0.95 results. The old threshold and timestamp are gone.

**Verified**: present=drifted (function at 777, SQL at 793).

**Remediation**: Return an outcome enum (consistent with sibling methods):
```rust
pub enum InsertClusterOutcome { Inserted, Replaced }
```
And/or add a `tracing::debug!` when `Replaced` occurs. The TD-019 stop-gap declaration
acknowledges "no per-dedup-run audit trail" but doesn't cover the overwrite-without-signal gap
which is a distinct issue.

---

## Theme H — `cluster_write_failed` excluded from `--strict` without documented justification [MEDIUM]

**Agents**: silent-failure-hunter

`dedup.rs:294-303`:
```rust
let all_per_photo_errors = stats.derive_failed.load(...)
    + ...
    + stats.catalog_inconsistency.load(...);
// Note: cluster_write_failed is a Phase-2 error; it does NOT trigger EX_STRICT_FAIL.
if args.strict && all_per_photo_errors > 0 { ... }
```
The comment notes the exclusion but provides no rationale. If 50 of 100 cluster writes fail
(disk full during Phase 2), the process exits 0 under `--strict`. Operators relying on strict mode
for CI pipelines would get false greens.

**Verified**: present=yes at dedup.rs:294.

**Remediation**: Either include `cluster_write_failed` in `all_per_photo_errors`, or document the
rationale in CLAUDE.md or a decision doc. The current inline comment ("Phase-2 error") is not
a justification. File a TD entry with a concrete binding trigger if deferring the fix.

---

## Theme I — `TECH-DEBT.md` TD-017 entry is stale: says "prospective — dedup.rs not yet created" [MEDIUM]

**Agents**: general-purpose (cross-cutting consistency)

`TECH-DEBT.md:305`:
```
Status: Open (prospective — dedup.rs not yet created; D3 deferred)
Stop-gap location: Prospective — ...::threshold_cluster will be the stop-gap location when D3 lands.
```
D3 shipped at commit `535210f`. `dedup.rs` exists at 409 lines. `threshold_cluster` is live code
at line 352 with the `// TD-017:` in-source label at line 347. The "prospective" and "will be"
language is factually false.

Similarly, TD-015 (`TECH-DEBT.md:260`) says "prospective — cull.rs not yet created; D4 deferred
due to D0 ABORT + DN-026" — `cull.rs` shipped in session 04. Both TDs need their status and
stop-gap location updated to reflect actual file:line citations.

**Verified**: present=yes at TECH-DEBT.md:305.

**Remediation**:
- TD-017: Update Status to "Open"; stop-gap location to `crates/photohelper-cli/src/commands/dedup.rs:347-352 (commit 535210f)`; change "will carry" to "carries".
- TD-015: Update Status to "Open"; reference `crates/photohelper-cli/src/commands/cull.rs`.

---

## Theme K — `SESSION-STATE.md` component table missing session 05 `photohelper-catalog` additions [MEDIUM]

**Agents**: general-purpose (cross-cutting consistency)

`SESSION-STATE.md:71` shows `photohelper-catalog | **implemented (sessions 01+04)**` with no
mention of session 05's additions: schema v3, `embeddings` table, `dup_clusters` table,
`apply_v2_to_v3` migration, `SCHEMA_VERSION=3`, `EmbeddingRow`, `InsertEmbeddingOutcome`,
`unembedded_rows`, `insert_embedding`, `all_embeddings_for_model`, `insert_dup_cluster`,
decision doc 0003.

**Verified**: present=yes at SESSION-STATE.md:71.

**Remediation**: Update to `**implemented (sessions 01+04+05)**` and append session 05 summary.
(Note: this should have been done as part of D5 ledger update — file it now as part of R1
remediation.)

---

## Theme L — Stale doc comments: `as_slice` claims D3 unimplemented; heartbeat "tick-first" is misleading for production intervals [MEDIUM]

**Agents**: comment-analyzer (primary), code-simplifier

**L1 — `as_slice` stale dead_code reason** (`embedding.rs:45-51`):
The `#[allow(dead_code, reason = "...will be used by dedup.rs (threshold_cluster, D3) — not yet implemented")]`
is factually wrong. D3 is shipped. `threshold_cluster` does NOT use `as_slice()` — it calls
`cosine_similarity()` which accesses `self.0` directly. The function remains dead code on production
paths, but the stated reason ("D3 not yet implemented") no longer applies.

**L2 — heartbeat "tick-first" doc** (`heartbeat.rs:109-116`):
The doc claims "tick-first guarantees at least one liveness signal per `interval`" but the
implementation starts `counter=0` and increments before checking `counter >= ticks`. With
`interval=10s, granularity=100ms, ticks=100`, the first `on_tick()` fires after 100 iterations
(= one full interval). A run that completes in 5s produces zero heartbeats. The "tick-first"
property only holds when `ticks=1` (test mode: interval ≤ granularity).

**Verified**: L1 drifted at embedding.rs:45 (evidence matches at lines 45-51); L2 present=yes at heartbeat.rs:109.

**Remediation**:
- L1: Update reason to "called only by tests; threshold_cluster uses cosine_similarity() instead of raw slice access."
- L2: Clarify doc: tick-first behavior applies when `ticks=1` (interval ≤ granularity). For production intervals, the first tick fires after one full interval.

---

## Theme M — `all_embeddings_for_model` includes superseded photos in clustering [MEDIUM]

**Agents**: code-architect (primary)

`catalog.rs:721`:
```sql
SELECT photo_id, embedding, dim FROM embeddings WHERE model_slug = ?1
```
No join against `photos` to filter `superseded_at_unix_seconds IS NULL`. Contrast with
`unembedded_rows` (line 611) and `unsuperseded_unscored_rows` (line 492), which both correctly
filter on superseded status.

If a photo is ingested, embedded, then superseded (a newer file replaces it), the old embedding
remains in `embeddings` and appears in every clustering pass. At v0.1 scale (single user, supersession
rare) this is cosmetic, but it violates the architectural principle that superseded photos are
invisible to processing pipelines.

**Verified**: present=yes at catalog.rs:721.

**Remediation**: Change query to join photos and add `AND p.superseded_at_unix_seconds IS NULL`.
Add a test analogous to `unembedded_rows_excludes_embedded_and_superseded`.

---

## Theme N — `mobileclip.rs` module/struct named `MobileClip` but implements LAION CLIP ViT-B/32 [MEDIUM]

**Agents**: comment-analyzer (primary)

The module file is `mobileclip.rs`, the public struct is `MobileClip`, but the implementation
uses `laion/CLIP-ViT-B-32-laion2B-s34B-b79K` (MIT) — Apple's MobileCLIP was rejected in D0 due
to the `apple-amlr` license (DN-028). The module doc at line 1 correctly identifies the model but
the naming creates confusion for future contributors.

**Verified**: present=yes at mobileclip.rs:1.

**Remediation**: The rename is low-priority (TD candidate for session 06). At minimum, add a
module-level doc note: "Named `mobileclip` after the original dedup plan (session-05); the actual
model is LAION CLIP ViT-B/32 (MobileCLIP rejected at D0 due to `apple-amlr` license — see DN-028)."

---

## Theme O — `TECH-DEBT.md` TD-019 in-source label quote says "per-cull-run" but actual comment says "per-dedup-run" [LOW]

**Agents**: comment-analyzer

`TECH-DEBT.md:337` states the in-source text is `// TD-019: per-cull-run audit trail absent`
(copied from TD-013's phrasing), but the actual comment at `catalog.rs:775` reads
`// TD-019: no per-dedup-run audit trail; similarity_threshold stored per-row as stop-gap.`
A reviewer reading the TECH-DEBT entry would search for the wrong comment text.

**Verified**: present=yes at TECH-DEBT.md:337.

**Remediation**: Update TECH-DEBT.md to quote the actual in-source text verbatim.

---

## Disposition summary

| Theme | Severity | Status | Action |
|---|---|---|---|
| A — dim consistency gap | CRITICAL | Open | Fix insert_embedding byte-check + dedup.rs caller + docstring |
| B — threshold_cluster zero tests | CRITICAL | Open | Add ≥5 unit tests in dedup.rs |
| C — SESS module-scoped thread_local | HIGH | Open | Add single-instance guard or fingerprint check |
| D — cosine_similarity silent swallow | HIGH | Open | Replace if let Ok with match + tracing::error! |
| E — Phase 2 empty/corrupt silent | HIGH | Open | Add log + deserialize_failed counter |
| F — catalog_inconsistency conflation | MEDIUM | Open | Split into already_embedded + catalog_insert_failed |
| G — insert_dup_cluster silent overwrite | MEDIUM | Open | Return InsertClusterOutcome or add trace! log |
| H — cluster_write_failed strict exclusion | MEDIUM | Open | Add to strict check or file TD with rationale |
| I — TD-017/TD-015 stale "prospective" | MEDIUM | Open | Update TECH-DEBT.md entries |
| J — filter_map silent drop (DISCARDED) | — | Hallucination | 9th agent: infallible by chunks_exact guarantee |
| K — SESSION-STATE catalog missing | MEDIUM | Open | Update component table |
| L — Stale docs (as_slice, tick-first) | MEDIUM | Open | Update reason + doc clarification |
| M — superseded photos in clustering | MEDIUM | Open | Add JOIN to photos + superseded filter |
| N — mobileclip naming confusion | MEDIUM | Open | Add module doc note; TD for rename |
| O — TD-019 misquoted label | LOW | Open | Fix TECH-DEBT.md quote |

**Watch-list for Round 2** (verify every CRITICAL + HIGH closure):
- [ ] R1-A: `insert_embedding` guards `dim*4 == bytes.len()` at line 666
- [ ] R1-A: `dedup.rs:257` uses `dim` (not `_dim`) and validates
- [ ] R1-A: `all_embeddings_for_model` docstring corrected
- [ ] R1-B: `#[cfg(test)] mod tests` in `dedup.rs` with ≥5 threshold_cluster unit tests
- [ ] R1-C: Single-instance guard or fingerprint check in `MobileClip`
- [ ] R1-D: `if let Ok(sim)` → match with `tracing::error!` on Err
- [ ] R1-E1: Log added for empty Phase 2 embedding set
- [ ] R1-E2: `deserialize_failed` counter added and incremented

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 19
  verified: 15
  drifted: 3
  hallucinated: 1
  unreadable: 0
  compromised: 0
  discard_rate: 0.053
  details:
    - finding_id: 9f8000056fb8e2a50adbcbdc4e77f57c3fb99277
      file: crates/photohelper-catalog/src/catalog.rs
      line: 666
      present: yes
      retain: yes
      reason: "dim range guard present but no byte-length check"
      evidence_snippet: "if dim == 0 || dim > 65536 {"
    - finding_id: 707be37f4f1909a63fcd059bbdf8a975da2ee56e
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 257
      present: yes
      retain: yes
      reason: "_dim destructured and discarded; no validation"
      evidence_snippet: "|(pid, bytes, _dim)| match ImageEmbedding::from_f32_le_bytes(&bytes) {"
    - finding_id: 8ffe530a7c8da48e0070695f43130d27f2ef325a
      file: crates/photohelper-catalog/src/catalog.rs
      line: 706
      present: drifted
      retain: yes-with-corrected-line
      reason: "Docstring at 706-708 promises caller validation that dedup.rs doesn't perform"
      evidence_snippet: "validates `dim == bytes.len() / 4` to catch corruption early"
    - finding_id: aa9a12028fcd2f5b7456d918a69f4507c6569009
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 352
      present: yes
      retain: yes
      reason: "function exists; no test module in file"
      evidence_snippet: "fn threshold_cluster(embeddings: &[(PhotoId, ImageEmbedding)], threshold: f32) -> ClusteringResult {"
    - finding_id: 834cbd5f8c0eacbd3fcd015e9a6b6e0b0059ad49
      file: crates/photohelper-ai/src/mobileclip.rs
      line: 39
      present: yes
      retain: yes
      reason: "thread_local! SESS at module scope"
      evidence_snippet: "thread_local! {"
    - finding_id: 4df053f11e26648a10b9cda653573cb272d528d6
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 369
      present: yes
      retain: yes
      reason: "if let Ok(sim) silently discards Err"
      evidence_snippet: "if let Ok(sim) = embeddings[i].1.cosine_similarity(&embeddings[j].1) {"
    - finding_id: 42e542d1d718cdd669b54b00966cfa37fdd684c4
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 249
      present: yes
      retain: yes
      reason: "empty set exits without log"
      evidence_snippet: "if all_embeddings.len() < 2 {"
    - finding_id: d167419bedc041bdce78bae59590d5aca5c93f1c
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 257
      present: yes
      retain: yes
      reason: "Err arm logs but increments no counter"
      evidence_snippet: "tracing::warn!(error = %e, \"skipping corrupt embedding during clustering\");"
    - finding_id: 83933f2260cee0468e23fe696f50667314a645be
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 219
      present: yes
      retain: yes
      reason: "AlreadyEmbedded and Err(e) both increment catalog_inconsistency"
      evidence_snippet: "stats.catalog_inconsistency.fetch_add(1, Ordering::Relaxed);"
    - finding_id: 0965f3ba01fa7ac1628dbcc6cb759035c88238b8
      file: crates/photohelper-catalog/src/catalog.rs
      line: 793
      present: drifted
      retain: yes-with-corrected-line
      reason: "INSERT OR REPLACE SQL at 793, function signature at 777"
      evidence_snippet: "INSERT OR REPLACE INTO dup_clusters"
    - finding_id: cf1ae31ccb99fda792b4f40988d853dd375b0d48
      file: crates/photohelper-cli/src/commands/dedup.rs
      line: 294
      present: yes
      retain: yes
      reason: "cluster_write_failed absent from sum; comment at 301 explains exclusion"
      evidence_snippet: "// Note: cluster_write_failed is a Phase-2 error; it does NOT trigger EX_STRICT_FAIL."
    - finding_id: 11a44427bf18ad50714f397650f018969804e177
      file: TECH-DEBT.md
      line: 305
      present: yes
      retain: yes
      reason: "Status says prospective but dedup.rs exists with threshold_cluster"
      evidence_snippet: "Status: Open (prospective — `dedup.rs` not yet created; D3 deferred)"
    - finding_id: 24c9e3825e17568dd5cd8fb0e0597c49e43bd50b
      file: crates/photohelper-ai/src/embedding.rs
      line: 105
      present: drifted
      retain: no
      reason: "DISCARDED: chunks_exact(4) makes try_from infallible; filter_map cannot silently drop; comment at line 104 documents this explicitly"
      evidence_snippet: "// chunks_exact(4) guarantees exactly 4-byte chunks; try_into().ok() cannot fail."
    - finding_id: a8d6dc6fe51f4e95a3b5aba61f0f1217b93e6aef
      file: SESSION-STATE.md
      line: 71
      present: yes
      retain: yes
      reason: "Row shows sessions 01+04 only; session 05 catalog work absent"
      evidence_snippet: "**implemented (sessions 01+04)**"
    - finding_id: cec957e99ab984303f257a11a29e23be0514a6b8
      file: crates/photohelper-ai/src/embedding.rs
      line: 45
      present: drifted
      retain: yes-with-corrected-line
      reason: "dead_code reason at 45-51 says D3 not yet implemented but D3 shipped"
      evidence_snippet: "will be used by dedup.rs (threshold_cluster, D3) — not yet implemented"
    - finding_id: c5128d3213e1e82e353211267162700acfcb58ef
      file: crates/photohelper-cli/src/heartbeat.rs
      line: 109
      present: yes
      retain: yes
      reason: "tick-first doc misleading for production intervals"
      evidence_snippet: "Tick-first heartbeat loop."
    - finding_id: d2ab735c1d339cd9c1e0d9969b5efe2bf3f2fba9
      file: crates/photohelper-catalog/src/catalog.rs
      line: 721
      present: yes
      retain: yes
      reason: "query lacks superseded_at_unix_seconds IS NULL filter"
      evidence_snippet: "SELECT photo_id, embedding, dim FROM embeddings WHERE model_slug = ?1"
    - finding_id: fc1f11d2ba764193b28daaa1697f5300bc24c2a1
      file: crates/photohelper-ai/src/mobileclip.rs
      line: 1
      present: yes
      retain: yes
      reason: "module named mobileclip but implements LAION CLIP ViT-B/32"
      evidence_snippet: "CLIP ViT-B/32 image encoder for deduplication embeddings."
    - finding_id: b1b13e159f5e076b71fffe9919bd868f110b36a5
      file: TECH-DEBT.md
      line: 337
      present: yes
      retain: yes
      reason: "TECH-DEBT says per-cull-run but actual in-source comment says per-dedup-run"
      evidence_snippet: "In-source: `// TD-019: per-cull-run audit trail absent`"
```
