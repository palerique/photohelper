# Session 05 — Duplicate-detection pipeline (MobileCLIP embeddings + dup\_clusters)

> Status: PLAN v3 (post-R2 remediation; CLEAN — implementation ready)
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
- Baseline 143 tests → target ≥ 167 (+24 minimum; see test plan for breakdown).

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
  - Computes L2-norm; rejects if **not finite** (NaN or Inf) → `Error::EmbeddingNotNormalized { norm: f32 }`.
  - Rejects if finite norm < 0.99 or > 1.01 → `Error::EmbeddingNotNormalized { norm }`.
    (NaN comparisons return `false`; the `is_finite()` check must fire first.)
  - Cheaply clonable (Arc<[f32]> ref-count bump).
- `ImageEmbedding::cosine_similarity(&self, other: &Self) -> Result<f32, Error>`:
  - Returns `Error::EmbeddingDimMismatch { expected: usize, got: usize }` if dimensions differ.
  - Since both are L2-normalized: `dot(self, other)` gives the cosine similarity directly.
  - Result clamped to [-1.0, 1.0] before return (float arithmetic may produce values slightly
    outside due to floating-point rounding).
  - Public API returns `Result` for cross-model safety. Inside `threshold_cluster`, add
    `debug_assert!(all embeddings have equal dim)` before the O(n²) loop.
- `ImageEmbedding::dim(&self) -> usize`: returns `self.0.len()`.
- `ImageEmbedding::as_f32_le_bytes(&self) -> Vec<u8>`: serializes to little-endian f32 bytes
  for catalog BLOB storage.
- `ImageEmbedding::from_f32_le_bytes(bytes: &[u8]) -> Result<Self, Error>`:
  deserializes + calls `from_raw` (validates norm again after deserialization).
- `static_assertions::assert_impl_all!(ImageEmbedding: Send, Sync)` at module scope.

#### D1b — MobileCLIP model binary

- Same Git LFS convention as NIMA model:
  - Path: `crates/photohelper-ai/models/<model-filename>.onnx` (filename confirmed in D0)
  - SHA-256 sidecar: `crates/photohelper-ai/models/manifest.toml` — MobileCLIP gets its own
    `[mobileclip_s1_v1]` section (section name = filename stem, same as NIMA's
    `[nima_mobilenet_aesthetic]` convention).
- **D1b sub-task (required)**: Extend `scripts/verify-model-sha256.sh` to iterate over ALL
  `[section_name]` headers in `manifest.toml`, extract `filename` and `sha256` from each
  section, and verify each model file. Replace the current single-model hardcoded
  `MODEL="crates/.../nima_mobilenet_aesthetic.onnx"` with a loop (~25 LoC). After this
  change, `just verify-model-sha256` covers BOTH NIMA and MobileCLIP automatically.
- Constants in `crates/photohelper-ai/src/lib.rs`:
  - `MOBILECLIP_MODEL_SLUG: &str` (e.g. `"mobileclip-s1-v1"`; confirmed in D0)
  - `MOBILECLIP_MODEL_MANIFEST_NAME: &str` (manifest.toml section name = filename stem;
    parallel to `MODEL_MANIFEST_NAME` for NIMA — use `_NAME` not `_KEY`)
- If conversion/export script is needed (same as `scripts/convert-nima-to-onnx.sh`),
  commit it as `scripts/convert-mobileclip-to-onnx.sh`. The D0 probe is session-local
  (not committed) unless it doubles as the conversion script.

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
  4. **L2-normalize output**: Before calling `from_raw`, check for a zero vector (norm < f32::EPSILON);
     return `Error::EmbeddingZeroVector` if detected (prevents NaN from reaching the DB).
     Then normalize: `normalized = raw / norm`. This ensures the DB invariant regardless of model.
  5. Return `Ok(ImageEmbedding::from_raw(normalized)?)`.
- `pub const MODEL_SLUG: &str` re-exported.
- New error variants on `photohelper_ai::Error` (enum is already `#[non_exhaustive]`; adding variants is non-breaking):
  - `Error::EmbeddingEmpty`
  - `Error::EmbeddingNotNormalized { norm: f32 }` (covers NaN/Inf norms too; `from_raw` checks `is_finite()` first)
  - `Error::EmbeddingZeroVector` (model emitted all-zeros; prevents NaN in normalization)
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

`Catalog::open` migration runner extends the `match v` block. The existing pattern has four
arms (0, 1, `v if v == SCHEMA_VERSION`, other). Both migration functions take
`(&mut Connection, &Path)` matching the existing `apply_v1_to_v2` signature:

```rust
0 => {
    // Fresh DB: INIT_SQL ran above (in transaction); chain both migrations.
    apply_v1_to_v2(&mut conn, catalog_path)?;
    apply_v2_to_v3(&mut conn, catalog_path)?;
}
1 => {
    apply_v1_to_v2(&mut conn, catalog_path)?;
    apply_v2_to_v3(&mut conn, catalog_path)?;
}
2 => {
    apply_v2_to_v3(&mut conn, catalog_path)?;
}
v if v == SCHEMA_VERSION => {}
other => {
    return Err(Error::CatalogSchemaTooNew { found: other, expected: SCHEMA_VERSION });
}
```

Both `apply_v1_to_v2` and `apply_v2_to_v3` use `CREATE TABLE IF NOT EXISTS` (idempotent).
`apply_v2_to_v3` runs in a `BEGIN IMMEDIATE` transaction matching the existing pattern.

Decision doc: `docs/decisions/0003-catalog-schema-v3.md` (schema rationale + stop-gap
declarations + ON DELETE CASCADE deferred per DN-023 pattern).

#### D2b — Catalog read/write API

New types:
- `EmbeddingRow { source_path: PathBuf, photo_id: PhotoId }` — private fields, accessors.
  Intentionally distinct from `CullRow` (same shape today; may diverge: EmbeddingRow could
  gain `dim` or `model_slug`; CullRow could gain `existing_score`). Two identical DTOs is
  below the three-instance abstraction threshold per CLAUDE.md.
- `InsertEmbeddingOutcome { Inserted | AlreadyEmbedded }`.
  (Uses `INSERT OR IGNORE` + `changes()` — same pattern as `InsertScoreOutcome`.)
- `insert_dup_cluster` returns `Result<(), Error>` — no outcome enum.
  `INSERT OR REPLACE` always succeeds by replacing; there is no meaningful outcome to
  discriminate. `AlreadyAssigned` would be dead code.

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
- Serializes `embedding.as_f32_le_bytes()` to BLOB. (`embedding.dim() > 0` is guaranteed by
  `ImageEmbedding`'s construction invariant; no redundant Rust-layer check needed.)
- `INSERT OR IGNORE INTO embeddings ...`; uses `changes()` to discriminate outcome.
- Stores `dim = embedding.dim()` and `quantization = 'f32'`.

**`all_embeddings_for_model(model_slug: &str) -> Result<Vec<(PhotoId, ImageEmbedding)>, Error>`**:
```sql
SELECT photo_id, embedding, dim FROM embeddings WHERE model_slug = ?1
```
Deserializes BLOB via `ImageEmbedding::from_f32_le_bytes`. Used in the clustering pass.

**`insert_dup_cluster(photo_id: &PhotoId, model_slug: &str, cluster_id: i64,
   threshold: f32, clustered_at: i64) -> Result<(), Error>`**:
- `INSERT OR REPLACE INTO dup_clusters ...` (re-clustering a photo replaces the old
  assignment — acceptable for v0.1; stop-gap S3 covers audit-trail gap).
- `cluster_id` is `i64` (matching SQLite INTEGER range). `threshold_cluster` compact-renumbers
  to `0..k-1` so values are always non-negative and small.
  Note: schema `CHECK(cluster_id >= 0)` enforces this at the DB layer.

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
    /// Valid range: (0.0, 1.0]. Values outside this range are rejected at parse time.
    #[arg(long, default_value_t = 0.95_f32, value_parser = parse_similarity_threshold)]
    similarity_threshold: f32,
}
```

`fn parse_similarity_threshold(s: &str) -> Result<f32, String>`: parses as f32; rejects NaN,
infinity, values ≤ 0.0, and values > 1.0 with a human-readable error message.

#### `DedupeStats`

`AtomicU64` fields (9 concurrent-write fields, all incremented from rayon workers):
`walked`, `embedded`, `derive_failed`, `decode_failed`, `infer_failed`,
`file_missing`, `content_changed`, `catalog_inconsistency`, `cluster_write_failed`.

Note: `already_embedded` is **not** a per-photo counter here. Within a single dedup process,
`unembedded_rows` returns unique rows (photos.id is PK), so `AlreadyEmbedded` returned by
`insert_embedding` always indicates an inter-process race → `catalog_inconsistency`.

`cluster_count` and `singleton_count` are NOT fields of `DedupeStats`. They come from
`ClusteringResult` returned by `threshold_cluster` and are used as local `u64` variables
in `run_dedup` after Phase 2. Storing them in `DedupeStats` would require special handling
(`Arc::get_mut` or interior mutability) since the Arc is still live after Phase 1.

Summary line format (stderr) — values mixed from `Arc<DedupeStats>` and post-Phase-2 locals:
```
walked: N, embedded: N, derive-failed: N, decode-failed: N, infer-failed: N,
file-missing: N, content-changed: N, catalog-inconsistency: N,
cluster-write-failed: N, clusters-found: N, singletons: N
```

#### `run_dedup` pipeline

```
Phase 1 — Embed:
  Catalog::unembedded_rows(MODEL_SLUG) → row_list
  if row_list.is_empty() → print summary ("walked: 0, ...") → exit 0
  [heartbeat thread spawned — dedup adds the third heartbeat consumer; TD-016 fires]
  rayon into_par_iter() over row_list:  // into_par_iter not par_bridge (consistent with cull.rs)
    for each EmbeddingRow:
      1. stat(path) → if Err or missing: file_missing++, continue
      2. PhotoId::derive(path) → if Err: derive_failed++, continue
                                → if Ok but differs from stored: content_changed++, continue
      3. read_raw_rgb(path) → RgbImage; if Err: decode_failed++, continue
      4. mobileclip.embed(&image) → ImageEmbedding; if Err: infer_failed++, continue
      5. Catalog::insert_embedding(...) → InsertEmbeddingOutcome
         - Inserted: embedded++
         - AlreadyEmbedded: catalog_inconsistency++; WARN ("AlreadyEmbedded for unembedded row —
           inter-process race?")  [not already_embedded; within-process duplicates are impossible]
  // into_par_iter().for_each() blocks until all workers finish → Phase 1 complete
  if heartbeat_handle.is_finished() { WARN("heartbeat died before end-of-dedup") }
  stop.signal();
  let _ = heartbeat_handle.join();  // discard result — WARN already surfaced above

Phase 2 — Cluster (sequential, after Phase 1 completes):
  Catalog::all_embeddings_for_model(MODEL_SLUG) → all: Vec<(PhotoId, ImageEmbedding)>
  if all.len() < 2 → print summary (0 clusters, all singletons) → skip DB writes → done
  threshold_cluster(&all, args.similarity_threshold) → ClusteringResult
    ClusteringResult { assignments: Vec<(PhotoId, i64)>, cluster_count: usize, singleton_count: usize }
  for each (photo_id, cluster_id) in assignments:
    match Catalog::insert_dup_cluster(photo_id, MODEL_SLUG, cluster_id, threshold, now):
      Ok(()) => {}
      Err(e) => { WARN(...); cluster_write_failed++ }
  cluster_count = result.cluster_count   // local u64; not in DedupeStats
  singletons = result.singleton_count   // local u64; not in DedupeStats

Print summary to stderr.
```

**Clustering transitivity design decision** (binding for D3 + `docs/decisions/0003-catalog-schema-v3.md`):
Union-find produces connected components via transitive closure. If sim(A,B) ≥ threshold and
sim(B,C) ≥ threshold but sim(A,C) < threshold, A/B/C are in the same cluster despite A and C
being dissimilar. This is the v0.1 choice — near-duplicate chains are transitive by design.
TD-017's DBSCAN upgrade addresses this if users report confusing cross-cluster groupings.

#### `threshold_cluster` algorithm

`ClusteringResult` is a `pub(crate)` struct in `crates/photohelper-cli/src/commands/dedup.rs`:
`{ assignments: Vec<(PhotoId, i64)>, cluster_count: usize, singleton_count: usize }`.

```
Input: &[(PhotoId, ImageEmbedding)], threshold: f32
Output: ClusteringResult { assignments: Vec<(PhotoId, i64)>, cluster_count: usize, singleton_count: usize }

Pre-condition debug_assert: all embeddings have the same dim (same model_slug guaranteed
by the SQL query). Dimension equality means cosine_similarity cannot return EmbeddingDimMismatch.

Algorithm (union-find with path compression + union-by-rank):
  1. Initialize: parent[i] = i, rank[i] = 0 for all i.
  2. find(i): path-compression variant — follow parent chain, flatten as we go. O(α(n)) amortized.
  3. union(i, j): union-by-rank — attach smaller-rank tree under larger-rank root. O(α(n)).
  4. For each pair (i, j) where i < j (O(n²) pairs):
       sim = cosine_similarity(emb[i], emb[j]).expect("same-model embeddings have equal dim")
       if sim >= threshold → union(i, j)
  5. Flatten: root[i] = find(i) for each photo.
  6. Renumber roots to compact IDs 0..k-1 (HashMap<root, new_id>).
  7. Build ClusteringResult: assignments, cluster_count = k, singleton_count = count(roots with one member).

Complexity: O(n² · α(n)) ≈ O(n²) with path compression + union-by-rank.
Memory: O(n · dim) for embeddings + O(n) for parent/rank arrays.

Warn to stderr if n > 5_000:
  "dedup: large corpus ({n} photos, ~{n * dim * 4 / 1_048_576} MiB); clustering O(n²) may
   take >60s; consider upgrading to DBSCAN (TD-017)."
```

#### TD-016 trigger (heartbeat)

`run_dedup` adds a heartbeat thread (Phase 1 is long-running for 370+ photos at
~0.62s/photo). This makes dedup the **third** heartbeat consumer (ingest + cull + dedup).
TD-016 binding trigger fires → **D4 (heartbeat.rs extraction) MUST land this session**.

#### Exit codes

- 0: success (no per-photo errors; clustering may be incomplete if cluster_write_failed > 0
  but that is not a per-photo error).
- 1 (EX_STRICT_FAIL): `--strict` + any Phase-1 per-photo error counter > 0.
  **Note**: `cluster_write_failed > 0` does NOT trigger EX_STRICT_FAIL — cluster-insert
  errors are Phase-2 write failures, not per-photo embedding errors.
- 64 (EX_USAGE): unembedded_rows returned rows, but `embedded == 0 AND all_per_photo_errors == 0`
  (likely wrong catalog path or model-slug mismatch — "nothing useful happened").
- 69 (EX_UNAVAILABLE): model file not found (same as other stubs).
- 75 (EX_TEMPFAIL): catalog lock timeout.

#### `scripts/photohelper-dedup.sh` wrapper (D3 companion)

Add `scripts/photohelper-dedup.sh` following the `photohelper-cull.sh` pattern:
- Export `PHOTOHELPER_MODEL_DIR="$ROOT_DIR/crates/photohelper-ai/models"`.
- Accept `--catalog` + `--similarity-threshold` as passthrough flags.
- Forward to `cargo run --release -p photohelper-cli -- dedup "$@"`.
Add a `just dedup <args>` recipe to `justfile`.

#### Integration tests (≥ 5)

1. **End-to-end**: ingest CC0 fixtures → dedup → assert `embeddings` rows exist +
   `dup_clusters` rows written. Check stderr: `embedded: 2`, `clusters-found: N`,
   `singletons: M` where N+M == 2.
2. **Idempotency**: run dedup twice → second run: `walked: 0`, exit 0.
   No new `embeddings` rows inserted (SQL filter verified).
3. **Strict mode**: create a catalog with a photo whose file has been removed →
   `dedup --strict` → exit 1; `file-missing: 1` in stderr.
4. **Threshold boundary**: `--similarity-threshold 1.0` on 2 CC0 fixtures →
   `clusters-found: 0, singletons: 2` (no pair can have sim == 1.0 unless identical).
5. **Empty catalog**: `dedup` on a freshly opened catalog with no photos → `walked: 0`,
   exit 0 (early-return path verified).

---

### D4 — TD-016 closure: heartbeat.rs extraction (mandatory — D3 adds third consumer)

Per TD-016: "Three consumers is the threshold for extracting the abstraction." D3
unconditionally adds a heartbeat to `dedup` (Phase 1 takes ~229s → user-visible).
Ingest (1) + cull (2) + dedup (3) = three consumers → **D4 is mandatory, not conditional.**

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
The 2 remaining TD-010 sub-items MUST land in D4 or as a D4a companion commit (~50 LoC).

**TD-010 test seam design** (after heartbeat.rs extraction):
- `heartbeat.rs` exposes `#[cfg(test)] pub(crate) fn spawn_dying_heartbeat(stop: Arc<HeartbeatStop>) -> JoinHandle<()>`:
  spawns a thread that panics after one tick (replaces `HeartbeatDeathTrigger` pattern for
  in-process tests). This is the test seam `run_ingest`'s heartbeat-death-WARN test uses.
- **TD-010 row 1** (`build_global WARN`): add `#[cfg(test)] mod tests` in `ingest.rs` that
  calls `run_ingest(...)` twice in the same test process (rayon global pool persists). ~20 LoC.
- **TD-010 row 4** (heartbeat-death-WARN): use the `spawn_dying_heartbeat` seam above to
  inject a dying heartbeat into `run_ingest`; assert the WARN appears in test logs. ~30 LoC.
Close TD-010 when both tests are green.

---

### D5 — Docs + ledger updates

- `SESSION-STATE.md` update (status, component table update for `photohelper-ai`
  and `photohelper-catalog`).
- `HANDOFF_REPORT.md` Checkpoint 14. (Checkpoint 13 was written during the context-refresh
  pause before plan-review; next is 14.)
- `TECH-DEBT.md`:
  - TD-016 → Closed (unconditional — D3 adds heartbeat; D4 is mandatory).
  - TD-010 → Closed (remaining 2 sub-items closed in D4).
  - Verify TD-017 (O(n²) clustering), TD-018 (f32 BLOB), TD-019 (`--show-clusters` UX)
    were filed at their introducing commits (D3 and D2b respectively). Do NOT file them here;
    per CLAUDE.md, stop-gap TDs must be filed at the introducing commit.
- `docs/discovery-notes.md`:
  - DN-024 → closed (dedup pipeline shipped).
  - **DN-027** (new, committed): MobileCLIP cross-platform embedding tolerance for cosine-
    similarity clustering. For NIMA a ±1e-3 score shift is acceptable; for clustering, the
    same f32 drift can flip cluster assignments when pairs have similarity near the threshold.
    Record the empirical cross-platform embedding delta from D0/D1c and document the
    safe-margin for the default threshold.
  - Any additional DNs from D0 pre-flight (e.g., demosaic algorithm interaction with CLIP
    preprocessing).

---

## Test plan summary

| Deliverable | Min new tests | What's verified |
|---|---|---|
| D1a ImageEmbedding | 6 | from_raw happy, empty rejects, norm+NaN+Inf rejects, cosine_similarity happy, cosine_similarity dim-mismatch, from_f32_le_bytes round-trip |
| D1c MobileClip | 3 | embed on CC0 fixture (dim + norm assert), golden cosine_sim vs committed vector (≥0.999 arm64; ≥0.98 x86_64), EmbeddingZeroVector error path |
| D2a schema v3 | 4 | migration idempotent v2→v3, migration chain v1→v3, FK enforcement (insert into dup_clusters with nonexistent embedding → `SqliteError { code: ConstraintViolation }` — requires PRAGMA foreign_keys=ON already set by Catalog::open), schema_version_too_new gate |
| D2b catalog API | 5 | insert_embedding, all_embeddings round-trip, unembedded_rows filter, insert_dup_cluster (replace), AlreadyEmbedded→catalog_inconsistency |
| D3 dedup | 5 | e2e (with summary-line assertions), idempotency, strict-mode, threshold=1.0→all-singletons, empty-catalog→exit0 |
| D4 TD-010 remaining | 2 | build_global WARN, heartbeat-death-WARN in-process |
| D4 heartbeat refactor | 0 new | existing heartbeat tests pass under new module |

**Total: ≥ 25 new tests → target ≥ 168 total** (143 baseline + 25).
(Header "What will exist" says ≥ 167; the 1-test difference is a rounding buffer — either number is acceptable.)

---

## Stop-gap declarations

**All stop-gaps require companion TDs filed IN THE SAME COMMIT that introduces the stop-gap
code (CLAUDE.md § No Acceptable Trade-offs Policy).** The TDs below must appear in TECH-DEBT.md
AND have in-source `// TD-NNN: <description>` comments at the stop-gap site — both in the
INTRODUCING commit, not in a later D5 docs sweep.

| # | Stop-gap | New TD | Introducing commit | In-source label location | Binding trigger |
|---|---|---|---|---|---|
| S1 | O(n²) union-find clustering; O(n×dim) memory | TD-017 | D3 (`threshold_cluster`) | `dedup.rs::threshold_cluster` | n > 10K photos OR user request for faster/lower-memory clustering |
| S2 | Embedding stored as raw f32 LE BLOB; `quantization = 'f32'` hardcoded | TD-018 | D2b (`insert_embedding`) | `catalog.rs::insert_embedding` | first user request for int8/f16 quantization or storage-size complaint |
| S3 | No per-dedup-run audit trail (no `dedup_runs` table) | TD-019 | D3 (`run_dedup`) + D2a (schema) | `dedup.rs::run_dedup` | first user report "I ran dedup twice, what changed?" or before v0.3 |

---

## Checkpoints

| Gate | When |
|---|---|
| D0 ABORT | If no permissive-licensed CLIP ONNX found → close session as D0-ABORT |
| D1d sub-component review | After D1a + D1b + D1c land (photohelper-ai extension) |
| D2c sub-component review | After D2a + D2b land (catalog v3 API) |
| D4 mandatory | D3 adds heartbeat (3rd consumer) → D4 is unconditional; also closes TD-010 |
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
D4 (heartbeat.rs extraction + TD-010 close — mandatory)
    ↓
D5 (ledger updates)
    ↓
Session-end review R1 → remediate → R2
    ↓
PR → CI → merge
```
