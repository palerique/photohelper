# Session 05 — Duplicate-detection pipeline (MobileCLIP embeddings + dup\_clusters)

> Status: PLAN (pre-review)
> Branch: `session-05/dedup-mobileclip`
> Started: 2026-05-29

---

## Goal

Wire the duplicate-detection pipeline end-to-end: acquire a MobileCLIP (or
equivalent CLIP-family) ONNX model with a verified permissive license, compute
per-photo image embeddings, store them in a v3 catalog schema, cluster by
cosine-similarity threshold, and expose a `dedup` subcommand
(`photohelper dedup`). Closes DN-024.

## What will exist by end-of-session

- `photohelper dedup` subcommand: walks unembedded photos → decodes RGB →
  embeds with MobileCLIP → stores embedding → runs cosine-similarity clustering
  → writes cluster assignments to `dup_clusters`.
- Catalog schema v3: `embeddings` table + `dup_clusters` table + `apply_v2_to_v3`
  migration. `SCHEMA_VERSION = 3`. Decision doc `docs/decisions/0003-catalog-schema-v3.md`.
- `photohelper-ai` crate extended: `ImageEmbedding` type + `MobileClip` struct
  (thread_local! per-worker ort Session, same concurrency model as `Nima`).
- CI gate: `just verify-model-sha256` covers MobileCLIP model binary (Git LFS).
- If `dedup` adds a heartbeat thread (expected: yes — 370 photos × ~0.62s/photo ≈
  229s wall-clock → user-visible), TD-016 binding trigger fires and
  `heartbeat.rs` extraction lands this session.
- Baseline 143 tests → target ≥ 158 (+15 minimum).

## Out of scope (explicit deferrals)

| Deferred item | TD/DN | Notes |
|---|---|---|
| Multi-model or joint NIMA+CLIP ranking | — | v0.3+ |
| `photohelper dedup --show-clusters` UX output | TD-019 (new, stop-gap S3) | v0.2 |
| DBSCAN / hierarchical clustering | TD-017 (new, stop-gap S1) | threshold-based for v0.1 |
| GPU inference (ort CUDA feature) | — | CPU-only for v0.1 |
| Embedding int8 quantization at insert | TD-018 (new, stop-gap S2) | stored as f32 BLOB |
| Windows build verification | DN-013 | v0.2 per existing deferral |
| `--model-path` power-user override for dedup | TD-015 pattern | same deferral as cull |
| ON DELETE CASCADE for `embeddings.photo_id` | DN-023 pattern | no delete path in v0.1 |

---

## Deliverables

### D0 — Pre-flight: MobileCLIP model audit + ort compatibility (ABORT if license fails)

**Must fire FIRST. D1b (model binary via Git LFS) does NOT land until D0 commits
`ANL-003-mobileclip-preflight.md`.**

**Search targets (in order)**:
1. `apple/ml-mobileclip` on GitHub — code is MIT; are the weight files also MIT
   or Apache-2.0? Check the WEIGHTS CARD or LICENSE files in the HuggingFace
   repo `apple/MobileCLIP` for each variant (S0, S1, S2, B).
2. If weights unclear: look for a community ONNX export with explicit license
   (same pattern as DN-026 resolution: convert from MIT/Apache-2.0 source).
3. Fallback — `openai/clip-vit-base-patch32` weights (MIT) exported to ONNX
   via `clip-onnx` or `optimum` (also MIT). ViT-B/32 produces 512-dim
   embeddings; larger but well-established.
4. **ABORT** if no candidate found with explicit MIT/Apache-2.0/CC-BY-4.0
   license and reproducible export → scope to session 06; close session 05 as
   D0-ABORT (per DN-024 binding trigger "if session 05 scope is too large,
   scope to session 06 with explicit acknowledgment").

**D0 acceptance criteria** (all required before any ort dep changes or model binary):
1. Model license confirmed explicit + permissive. Record in ANL-003.
2. Inference smoke test on 2 CC0 CR3 fixtures: each produces an embedding of
   the expected dimension (≥ 256, ≤ 2048) and L2-norm ∈ [0.98, 1.02].
3. `Session::run` receiver type confirmed (expected `&mut self` per ort
   2.0.0-rc.12; determines D3 concurrency model — thread_local! path chosen
   if `&mut self`, shared Arc<Mutex<Session>> if `&self`).
4. Preprocessing parameters confirmed and recorded in ANL-003:
   - Resize target (expected 224×224; confirm exact resize + crop policy)
   - Normalization mean + std (CLIP-standard:
     mean=[0.48145466, 0.4578275, 0.40821073],
     std=[0.26862954, 0.26130258, 0.27577711]; or model-specific)
   - Tensor layout (expected NCHW [1,C,H,W]; confirm CHW vs HWC)
   - Input value range (expected float32 after normalization)
5. ort 2.0.0-rc.12 CVE-posture re-checked (already clean in session 04;
   quick re-check is sufficient — no new advisories since 2026-05-29).
6. **Artifact**: `docs/analysis/ANL-003-mobileclip-preflight.md` committed.
   (Same format as ANL-001 + ANL-002.)

**D0 sequencing**:
```
D0-probe-script → D0-ANL-003-commit → D1a (no dep changes needed; ort already wired)
→ D1b (model binary in Git LFS) → D1c+D1d (MobileClip struct + sub-component review)
```
ort dep (`=2.0.0-rc.12`) is already in `Cargo.toml` from session 04 — no version change
needed unless D0 finds a newer stable release. Record any version bump decision in ANL-003.

---

### D1 — MobileCLIP integration in `photohelper-ai`

#### D1a — `ImageEmbedding` type (`crates/photohelper-ai/src/embedding.rs`)

```rust
pub struct ImageEmbedding(Arc<[f32]>);
```

- `ImageEmbedding::from_raw(vec: Vec<f32>) -> Result<Self, Error>`:
  - Rejects `vec.is_empty()` → `Error::EmbeddingEmpty`.
  - Rejects L2-norm < 0.99 or > 1.01 → `Error::EmbeddingNotNormalized { norm: f32 }`.
  - Cheaply clonable (Arc<[f32]> ref-count bump).
- `ImageEmbedding::cosine_similarity(&self, other: &Self) -> Result<f32, Error>`:
  - Returns `Error::EmbeddingDimMismatch { expected: usize, got: usize }` if dimensions differ.
  - Since both are L2-normalized: `dot(self, other)` gives the cosine similarity directly.
  - Result clamped to [-1.0, 1.0] before return (float arithmetic may produce values slightly
    outside due to floating-point rounding).
- `ImageEmbedding::dim(&self) -> usize`: returns `self.0.len()`.
- `ImageEmbedding::as_f32_le_bytes(&self) -> Vec<u8>`: serializes to little-endian f32 bytes
  for catalog BLOB storage.
- `ImageEmbedding::from_f32_le_bytes(bytes: &[u8]) -> Result<Self, Error>`:
  deserializes + calls `from_raw` (validates norm again after deserialization).
- `static_assertions::assert_impl_all!(ImageEmbedding: Send, Sync)` at module scope.

#### D1b — MobileCLIP model binary

- Same Git LFS convention as NIMA model:
  - Path: `crates/photohelper-ai/models/<model-filename>.onnx` (filename confirmed in D0)
  - SHA-256 sidecar: `crates/photohelper-ai/models/manifest.toml` extended (or new section)
  - `just verify-model-sha256` CI gate covers BOTH models (NIMA + MobileCLIP)
- Constants in `crates/photohelper-ai/src/lib.rs`:
  - `MOBILECLIP_MODEL_SLUG: &str` (e.g. `"mobileclip-s1-v1"`; confirmed in D0)
  - `MOBILECLIP_MODEL_MANIFEST_KEY: &str` (key inside manifest.toml)
- If conversion script is needed (same as `scripts/convert-nima-to-onnx.sh`),
  commit it as `scripts/convert-mobileclip-to-onnx.sh`.

#### D1c — `MobileClip` struct (`crates/photohelper-ai/src/mobileclip.rs`)

```rust
pub struct MobileClip {
    verified: VerifiedModelBytes,  // reuse type from model_bytes.rs
    model_slug: &'static str,
}
```

- Constructor: `MobileClip::new(verified: VerifiedModelBytes) -> Self`.
- `pub fn embed(&self, image: &RgbImage) -> Result<ImageEmbedding, Error>`:
  1. **Preprocess** (parameters confirmed in D0; numbers below are expected defaults):
     - Resize `image` to 224×224 using nearest-neighbor or bilinear (exact algorithm
       confirmed in D0; bilinear is faster than bicubic for a v0.1 stop-gap — file TD if
       the paper specifies bicubic).
     - Convert `u8` pixels to `f32` in [0.0, 1.0].
     - Normalize per channel with CLIP-standard mean/std (confirmed in D0).
     - Transpose HWC (height×width×channels) → CHW (channels×height×width).
     - Add batch dim: shape `[1, C, H, W]` = `[1, 3, 224, 224]` (NCHW, confirmed in D0).
     - Boxed slice for ort `Tensor::<f32>::from_array`.
  2. **Session construction** (same thread_local! pattern as Nima::score):
     ```rust
     thread_local! {
         static SESSION: RefCell<Option<ort::session::Session>> = RefCell::new(None);
     }
     SESSION.with(|cell| {
         let mut guard = cell.borrow_mut();
         if guard.is_none() {
             match ort::session::Session::builder()?.commit_from_memory(&self.verified.bytes()) {
                 Ok(s) => *guard = Some(s),
                 Err(e) => return Err(Error::ModelLoad { source: Box::new(e) }),
             }
         }
         let sess = guard.as_mut().unwrap(); // #[allow(..., reason="proven Some")]
         // ...run inference...
     })
     ```
  3. **Inference**: extract input/output names to `String` BEFORE `sess.run()` (avoids
     borrow conflict, per session-04 lesson). Use `ort::inputs!["name" => tensor]`.
  4. **L2-normalize output**: ort may or may not emit a normalized embedding depending on
     the model variant. Normalize explicitly with `l2_norm(raw_embedding)` before calling
     `ImageEmbedding::from_raw`. This ensures the DB invariant holds regardless of model.
  5. Return `Ok(ImageEmbedding::from_raw(normalized)?)`.
- `pub const MODEL_SLUG: &str` re-exported.
- New error variants on `photohelper_ai::Error`:
  - `Error::EmbeddingEmpty`
  - `Error::EmbeddingNotNormalized { norm: f32 }`
  - `Error::EmbeddingDimMismatch { expected: usize, got: usize }`
  - `Error::MobileClipInferenceFailed { source: Box<dyn std::error::Error + Send + Sync> }`

#### D1d — Sub-component review fires after D1a + D1b + D1c land

Artifact: `docs/code-reviews/session-05-mobileclip-ai-round{1,2}.md`.
Scope: `crates/photohelper-ai/src/{embedding,mobileclip,lib,model_bytes}.rs`.

---

### D2 — Catalog v2 → v3 migration

#### D2a — Schema v3 (`crates/photohelper-catalog/src/schema.rs`)

`SCHEMA_VERSION = 3`.

`MIGRATE_V2_TO_V3_SQL`:

```sql
-- embeddings: one row per (photo_id, model_slug)
CREATE TABLE IF NOT EXISTS embeddings (
    photo_id    BLOB    NOT NULL REFERENCES photos(id),
    model_slug  TEXT    NOT NULL,
    dim         INTEGER NOT NULL CHECK(dim > 0 AND dim <= 65536),
    quantization TEXT   NOT NULL DEFAULT 'f32',
    embedding   BLOB    NOT NULL,
    embedded_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY (photo_id, model_slug)
);

-- dup_clusters: cluster assignment per (photo_id, model_slug)
-- cluster_id is a run-local integer; no cull_run_id equivalent in v0.1 (stop-gap S3)
CREATE TABLE IF NOT EXISTS dup_clusters (
    photo_id    BLOB    NOT NULL,
    model_slug  TEXT    NOT NULL,
    cluster_id  INTEGER NOT NULL CHECK(cluster_id >= 0),
    similarity_threshold REAL NOT NULL,
    clustered_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY (photo_id, model_slug),
    FOREIGN KEY (photo_id, model_slug) REFERENCES embeddings(photo_id, model_slug)
);
```

`Catalog::open` migration runner extends the `match v` block:
```rust
1 => { apply_v1_to_v2(&conn)?; apply_v2_to_v3(&conn)?; }
2 => { apply_v2_to_v3(&conn)?; }
3 => {} // SCHEMA_VERSION
```

Both `apply_v1_to_v2` and `apply_v2_to_v3` use `CREATE TABLE IF NOT EXISTS` (idempotent).
`apply_v2_to_v3` runs in a transaction.

Decision doc: `docs/decisions/0003-catalog-schema-v3.md` (schema rationale + stop-gap
declarations + ON DELETE CASCADE deferred per DN-023 pattern).

#### D2b — Catalog read/write API

New types:
- `EmbeddingRow { source_path: PathBuf, photo_id: PhotoId }` — private fields, accessors.
  (Same shape as `CullRow`.)
- `InsertEmbeddingOutcome { Inserted | AlreadyEmbedded }`.
- `InsertClusterOutcome { Inserted | AlreadyAssigned }`.

New `Catalog` methods:

**`unembedded_rows(model_slug: &str) -> Result<Vec<EmbeddingRow>, Error>`**:
```sql
SELECT source_path, id
FROM   photos
WHERE  superseded_at_unix_seconds IS NULL
  AND  id NOT IN (
           SELECT photo_id FROM embeddings WHERE model_slug = ?1
       )
ORDER BY ingested_at ASC
```
(mirrors `unsuperseded_unscored_rows`; same NOT-IN pattern)

**`insert_embedding(photo_id: &PhotoId, model_slug: &str, embedding: &ImageEmbedding,
   embedded_at: i64) -> Result<InsertEmbeddingOutcome, Error>`**:
- Validates `embedding.dim() > 0`.
- Serializes `embedding.as_f32_le_bytes()` to BLOB.
- `INSERT OR IGNORE INTO embeddings ...`; uses `changes()` to discriminate outcome.
- Stores `dim = embedding.dim()` and `quantization = 'f32'`.

**`all_embeddings_for_model(model_slug: &str) -> Result<Vec<(PhotoId, ImageEmbedding)>, Error>`**:
```sql
SELECT photo_id, embedding, dim FROM embeddings WHERE model_slug = ?1
```
Deserializes BLOB via `ImageEmbedding::from_f32_le_bytes`. Used in the clustering pass.

**`insert_dup_cluster(photo_id: &PhotoId, model_slug: &str, cluster_id: u64,
   threshold: f32, clustered_at: i64) -> Result<InsertClusterOutcome, Error>`**:
- `INSERT OR REPLACE INTO dup_clusters ...` (re-clustering a photo replaces the old
  assignment — acceptable for v0.1; stop-gap S3 covers audit-trail gap).

#### D2c — Sub-component review fires after D2a + D2b land

Artifact: `docs/code-reviews/session-05-catalog-v3-round{1,2}.md`.
Scope: `crates/photohelper-catalog/src/{catalog,schema}.rs` + new types.

---

### D3 — `dedup` subcommand

File: `crates/photohelper-cli/src/commands/dedup.rs`.
Wired in `main.rs` alongside `cull.rs`. `PHOTOHELPER_MODEL_DIR` env-var supplies
both NIMA and MobileCLIP model directories (same convention as cull).

#### `DedupeArgs`

```rust
pub struct DedupeArgs {
    /// Path to the catalog DB file.
    #[arg(long, default_value = ".photohelper/catalog.db")]
    catalog: PathBuf,
    /// Exit non-zero if any per-photo error occurs.
    #[arg(long)]
    strict: bool,
    /// Cosine-similarity threshold: photos with sim >= this are considered duplicates.
    #[arg(long, default_value_t = 0.95_f32)]
    similarity_threshold: f32,
}
```

#### `DedupeStats`

`AtomicU64` fields (8 total):
`walked`, `already_embedded`, `embedded`, `decode_failed`, `infer_failed`,
`file_missing`, `content_changed`, `catalog_inconsistency`.

Summary line format (stderr):
```
walked: N, embedded: N, already-embedded: N, decode-failed: N, infer-failed: N,
file-missing: N, content-changed: N, catalog-inconsistency: N,
clusters-found: N, singletons: N
```
(clusters-found + singletons computed after the clustering pass; printed in `run_dedup` after
the clustering step, not in `DedupeStats` — they are not per-photo errors.)

#### `run_dedup` pipeline

```
Phase 1 — Embed:
  Catalog::unembedded_rows(MODEL_SLUG) → row_list
  if row_list.is_empty() → print summary ("walked: 0, ...") → exit 0
  [heartbeat thread spawned here — see TD-016 note below]
  rayon par_bridge over row_list:
    for each EmbeddingRow:
      1. file_missing check (stat the path)
      2. PhotoId::derive(path) → content_changed check
      3. read_raw_rgb(path) → RgbImage  (decode_failed counter)
      4. mobileclip.embed(&image) → ImageEmbedding  (infer_failed counter)
      5. Catalog::insert_embedding(...) → InsertEmbeddingOutcome
         - Inserted: increment embedded
         - AlreadyEmbedded: increment already_embedded (catalog_inconsistency
           if this was in the unembedded_rows list — data race between workers)
  heartbeat.signal(); heartbeat_handle.join();

Phase 2 — Cluster (sequential, after Phase 1 completes):
  Catalog::all_embeddings_for_model(MODEL_SLUG) → all (PhotoId, ImageEmbedding) pairs
  threshold_cluster(all, args.similarity_threshold) → Vec<(PhotoId, cluster_id: u64)>
  for each assignment: Catalog::insert_dup_cluster(...)
  compute clusters_found + singletons from assignments

Print summary to stderr.
```

#### `threshold_cluster` algorithm

```
Input: &[(PhotoId, ImageEmbedding)], threshold: f32
Output: Vec<(usize /* index */, u64 /* cluster_id */)]

Algorithm (union-find):
  1. Initialize each photo in its own cluster: parent[i] = i.
  2. For each pair (i, j) where i < j:
       if cosine_similarity(emb[i], emb[j]) >= threshold → union(i, j)
  3. Flatten to cluster_id = find(i) for each photo.
  4. Renumber cluster IDs to 0..k-1 (compact).

O(n²) cosine similarity comparisons.
Warn to stderr if n > 5_000: "dedup: large corpus ({n} photos); clustering may take
>60s; consider future DBSCAN upgrade (TD-017)."
```

#### TD-016 trigger (heartbeat)

`run_dedup` adds a heartbeat thread (Phase 1 is long-running for 370+ photos at
~0.62s/photo). This makes dedup the **third** heartbeat consumer (ingest + cull + dedup).
TD-016 binding trigger fires → **D4 (heartbeat.rs extraction) MUST land this session**.

#### Exit codes

- 0: success (no per-photo errors).
- 1 (EX_NOPERM): `--strict` + any per-photo error counter > 0.
- 69 (EX_UNAVAILABLE): model file not found (same as other stubs).
- 75 (EX_TEMPFAIL): catalog lock timeout.

#### Integration tests (≥ 3)

1. **End-to-end**: ingest CC0 fixtures → dedup → assert `embeddings` rows exist +
   `dup_clusters` rows exist (at least one cluster or singletons). Check `embedded == 2`.
2. **Idempotency**: run dedup twice → second run asserts `walked == 0` (already_embedded
   filter works). No new `embeddings` rows inserted.
3. **Strict mode**: create a catalog with a photo whose file has been removed →
   `dedup --strict` → exit 1; `file_missing > 0` in stderr.

---

### D4 — TD-016 closure: heartbeat.rs extraction (CONDITIONAL on D3 trigger)

Per TD-016: "Three consumers is the threshold for extracting the abstraction." D3
confirms dedup needs a heartbeat → three consumers → extract.

New file: `crates/photohelper-cli/src/heartbeat.rs` (`pub(crate)` module).

Contents extracted (not copied) from `ingest.rs` + `cull.rs`:
- `HeartbeatStop` struct (Mutex<bool> + Condvar + signal() + wait_for_stop())
- `heartbeat_loop` fn (tick-first-wait-after; emits `[heartbeat]` lines to stderr)
- `HeartbeatHandle` (thin wrapper around `JoinHandle<()>`)

Both `ingest.rs` and `cull.rs` import from `heartbeat.rs` (no behavioral change).
`dedup.rs` imports from `heartbeat.rs`.

Remove both `// TD-016` in-source comments from `cull.rs`.
Update TECH-DEBT.md: TD-016 → Closed.

0 new tests (existing heartbeat tests in `ingest.rs` + `cull.rs` pass unchanged under the
new module; module-level is `pub(crate)` so test files can import).

**TD-010 interaction**: D4 touches `commands/ingest.rs` (import site update) →
TD-010's binding trigger fires ("next session that touches `commands/ingest.rs`").
The 2 remaining TD-010 sub-items (build_global WARN + heartbeat-death-WARN in-process
tests) MUST also land in D4 or as D4a companion commit. Scope: ~50 LoC.
See TD-010 for concrete test seam specs.

---

### D5 — Docs + ledger updates

- `SESSION-STATE.md` update (status, component table update for `photohelper-ai`
  and `photohelper-catalog`).
- `HANDOFF_REPORT.md` Checkpoint 13.
- `TECH-DEBT.md`:
  - TD-016 → Closed (if D4 fires).
  - TD-010 → Closed (remaining 2 sub-items closed in D4).
  - TD-017 (new): O(n²) clustering stop-gap.
  - TD-018 (new): f32 BLOB quantization stop-gap.
  - TD-019 (new): `--show-clusters` UX deferred.
- `docs/discovery-notes.md`:
  - DN-024 → closed (dedup pipeline shipped).
  - Any new DNs from D0 pre-flight (e.g. demosaic algorithm interaction with
    CLIP preprocessing, cross-platform embedding tolerance).

---

## Test plan summary

| Deliverable | Min new tests | What's verified |
|---|---|---|
| D1a ImageEmbedding | 4 | from_raw happy, empty rejects, norm rejects, cosine_similarity |
| D1c MobileClip | 3 | embed on CC0 fixture (dim + norm assert), golden-band check, dim-mismatch error |
| D2a schema v3 | 3 | migration idempotent v2→v3, migration chain v1→v3, FK enforcement |
| D2b catalog API | 5 | insert_embedding, all_embeddings round-trip, unembedded_rows filter, insert_dup_cluster, AlreadyEmbedded outcome |
| D3 dedup | 3 | e2e, idempotency, strict-mode |
| D4 TD-010 remaining | 2 | build_global WARN, heartbeat-death-WARN in-process |
| D4 heartbeat refactor | 0 new | existing tests pass |

**Total: ≥ 20 new tests → target ≥ 163 total** (143 baseline + 20).

---

## Stop-gap declarations

All stop-gaps require companion TDs filed at the commit that introduces the stop-gap.

| # | Stop-gap | New TD | Location | Binding trigger |
|---|---|---|---|---|
| S1 | O(n²) cosine-similarity threshold clustering | TD-017 | `dedup.rs::threshold_cluster` | n > 10K photos in a real user corpus OR user request for faster clustering |
| S2 | Embedding stored as raw f32 LE bytes; `quantization = 'f32'` hardcoded | TD-018 | `catalog.rs::insert_embedding` | first user request for int8/f16 quantization or storage size complaint |
| S3 | No per-dedup-run audit trail (no `dedup_runs` table equivalent) | TD-019 | `dedup.rs::run_dedup` + `dup_clusters` schema | first user report "I ran dedup twice, what changed?" or before v0.3 |

---

## Checkpoints

| Gate | When |
|---|---|
| D0 ABORT | If no permissive-licensed CLIP ONNX found → close session as D0-ABORT |
| D1d sub-component review | After D1a + D1b + D1c land (photohelper-ai extension) |
| D2c sub-component review | After D2a + D2b land (catalog v3 API) |
| D4 TD-016 trigger | If D3 adds a heartbeat (expected: yes); fires TD-010 closure also |
| Session-end R1 + R2 | After D4 + D5 complete |

---

## Binding triggers from prior sessions that fire this session

- **DN-024 binding trigger**: "Session 05's plan MUST include MobileCLIP dup-detection
  as a primary deliverable." ✓ (satisfied by this plan's goal).
- **TD-016 binding trigger**: fires if D3 adds a heartbeat (three consumers). ✓ (D4 closes it).
- **TD-010 binding trigger**: "next session that touches `commands/ingest.rs`." D4 touches
  ingest.rs → ✓ (D4 companion commit closes TD-010 remaining 2 sub-items).

---

## Implementation sequencing

```
D0 probe → D0 ANL-003 commit
    ↓
D1a (ImageEmbedding type + tests)
    ↓
D1b (model binary Git LFS + verify-model-sha256 update)
    ↓
D1c (MobileClip struct + tests)
    ↓
D1d sub-component review R1 → remediate → R2
    ↓
D2a (schema v3 migration + decision doc)
    ↓
D2b (catalog API: unembedded_rows, insert_embedding, all_embeddings, insert_dup_cluster)
    ↓
D2c sub-component review R1 → remediate → R2
    ↓
D3 (dedup subcommand + threshold_cluster + tests)
    ↓
D4 (heartbeat.rs extraction + TD-010 close; conditional on D3 trigger, expected to fire)
    ↓
D5 (ledger updates)
    ↓
Session-end review R1 → remediate → R2
    ↓
PR → CI → merge
```
