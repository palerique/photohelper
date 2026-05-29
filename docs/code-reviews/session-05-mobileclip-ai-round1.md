# Session 05 — MobileCLIP AI sub-component (D1a+D1b+D1c), Review Round 1

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

**Scope**: `crates/photohelper-ai/src/{embedding,mobileclip,error,lib,model_bytes}.rs`
and `crates/photohelper-raw/tests/integration_clip.rs`.

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 1 |
| HIGH | 2 |
| MEDIUM | 5 |
| LOW | 7 |

---

## Theme A — TD-020 not filed in TECH-DEBT.md [CRITICAL]

- [feature-dev:code-reviewer]: CRITICAL — `mobileclip.rs:66` references `(TD-020: bicubic center-crop deferred)` and `nima.rs:254` references `(TD-020)` in the `bilinear_resize` doc. `integration_clip.rs:78` also references TD-020. TECH-DEBT.md contains zero entries for TD-020.
- [pr-review-toolkit:comment-analyzer]: CRITICAL — three source files carry the phantom reference; TECH-DEBT.md ends at TD-016.
- [pr-review-toolkit:silent-failure-hunter]: CRITICAL (same finding from a different lens).
- [pr-review-toolkit:pr-test-analyzer]: CRITICAL (same finding).

**Verification (F1)**: `present: yes` — `mobileclip.rs:66` verbatim: `"Bilinear resize to 224×224 (TD-020: bicubic center-crop deferred)."` No TD-020 entry exists in TECH-DEBT.md.

Per CLAUDE.md § No Acceptable Trade-offs Policy: "every stop-gap commit MUST file a TD entry in TECH-DEBT.md" and "Stop-gap commits without companion TDs violate this policy and are treated as deferral-without-a-plan (a CRITICAL finding)."

**Remediation**: File TD-020 in TECH-DEBT.md with all required fields:
- **Status**: Open
- **Opened**: 2026-05-29 (session 05, D1c)
- **Stop-gap location**: `crates/photohelper-ai/src/mobileclip.rs:82` (`bilinear_resize` call) + `crates/photohelper-ai/src/nima.rs:255` (visibility promotion to `pub(crate)`)
- **Fundamental fix**: Replace `nima::bilinear_resize` (bilinear, 1:1 resize) with a CLIP-standard preprocessing pipeline: `resize(shortest_edge=224, bicubic) → center_crop(224×224)`. Implement as a standalone function or use the `image` crate's `FilterType::CatmullRom`. Expected ~30 LoC.
- **Binding trigger**: next session that touches `MobileClip::embed` preprocessing OR user-reported clustering quality regression traceable to preprocessing differences.
- **Scope estimate**: ~30 LoC / low risk.
- **Consequence of inaction**: CLIP embeddings computed with bilinear 1:1 resize may have reduced similarity to the model's training distribution (trained on center-cropped bicubic), producing slightly lower inter-photo cosine similarities. Empirical delta: cosine_sim(CRAW, RAW) = 0.843 (bilinear) vs 0.923 (Python bicubic).

---

## Theme B — `EmbeddingNotNormalized` misused for corrupt byte slices [HIGH]

- [pr-review-toolkit:type-design-analyzer]: HIGH — `embedding.rs:101-105` returns `EmbeddingNotNormalized { norm: f32::NAN }` for a non-multiple-of-4 byte slice. The error message reads "embedding L2-norm is not finite or out of range [0.99, 1.01]: NaN" which misrepresents a structural corruption as a normalization failure.
- [pr-review-toolkit:comment-analyzer]: The comment `// sentinel: indicates corrupt deserialization` acknowledges the misuse.

**Verification (F2)**: `present: yes` — embedding.rs:103 verbatim: `"return Err(Error::EmbeddingNotNormalized {\n    norm: f32::NAN, // sentinel: indicates corrupt deserialization\n});"`

**Remediation**: Add a new `Error::EmbeddingCorruptBytes { len: usize }` variant with message `"embedding bytes are corrupt: length {len} is not a multiple of 4"`. Change the `bytes.len() % 4 != 0` branch to return this variant. The `EmbeddingNotNormalized` variant remains for genuine norm-range failures. `Error` is already `#[non_exhaustive]` so adding a variant is non-breaking.

---

## Theme C — `extract_field` has zero unit tests [HIGH]

- [pr-review-toolkit:pr-test-analyzer]: HIGH — `model_bytes.rs:115-133` is a hand-rolled pure TOML parser with zero `#[cfg(test)]` coverage. It is security-critical: it extracts both `sha256` (SHA-256 verification) and `filename` (which model file to load). A regression here silently loads an unverified or wrong model file. Happy path, missing section, missing key, empty value, and fallback behavior are all untested.

**Verification (F3)**: `present: yes` — grep confirms no `#[cfg(test)]` in `model_bytes.rs`.

**Remediation**: Add a `#[cfg(test)] mod tests` block in `model_bytes.rs` covering:
1. `extract_field` happy path — known section and key → value
2. `extract_field` with missing section → None
3. `extract_field` with missing key in present section → None
4. `extract_field` with empty value (`key = ""`) → None
5. `extract_field` fallback in `from_manifest` — no `filename` field → falls back to `{name}.onnx`
6. `extract_sha256` and `extract_filename` wrappers

---

## Theme D — `MobileClip::new` missing `model_path`; ModelLoad errors are anonymous [MEDIUM]

- [general-purpose]: MEDIUM — `mobileclip.rs:57`: `pub fn new(model: &VerifiedModelBytes) -> Self`. `nima.rs:127`: `pub fn new(model: &VerifiedModelBytes, model_path: PathBuf) -> Self`. When `commit_from_memory` fails in `mobileclip.rs:114-120`, the `Error::ModelLoad` has no path context. The user cannot tell which model file failed to load.
- [pr-review-toolkit:silent-failure-hunter]: MEDIUM (same finding from the silent-failure lens).

**Verification (F4)**: `present: yes` — MobileClip::new signature confirmed as `pub fn new(model: &VerifiedModelBytes) -> Self`.

**Remediation**: Add `model_path: PathBuf` to `MobileClip::new` and store it as a field. The `ModelLoad` error path (lines 113-120) then becomes `Error::ModelLoad { source: Box::new(e) }` with no path (ModelLoad has no path field in the current error enum — which is consistent with Nima's behavior). Alternatively: add a `model_name: String` field derived from the manifest section name, and include it in the `ModelLoad` error via a wrapping `MobileClipInferenceFailed`. Simplest: match `Nima::new`'s signature for API symmetry, storing the path for future use.

---

## Theme E — No logging in `mobileclip.rs`; retry on deterministic failure is silent [MEDIUM]

- [pr-review-toolkit:silent-failure-hunter]: MEDIUM — `mobileclip.rs` imports zero `tracing::` items and emits zero log events. If `commit_from_memory` fails deterministically (e.g., corrupt 85 MB model file, OOM), every photo on that rayon worker thread retries the full model load before failing. The operator sees N identical `infer_failed` increments with no indication the root cause is a one-time model load failure being retried N × 0.5s.

**Verification (F5)**: `present: yes` — grep confirms zero `tracing::` calls in `mobileclip.rs`.

**Remediation**: Add `use tracing` (already a workspace dep). On first `ModelLoad` failure, emit `tracing::error!(path = ?..., "CLIP model load failed; this worker thread will retry on each photo")`. Optionally store a `thread_local! { static LOAD_FAILED: Cell<bool> }` to short-circuit retries after first failure. Same fix applies to `nima.rs` (same pattern, same gap).

---

## Theme F — CLIP and NIMA constants placed in different modules [MEDIUM]

- [general-purpose]: MEDIUM — `NIMA: MODEL_SLUG` + `MODEL_MANIFEST_NAME` live in `model_bytes.rs` (re-exported via `lib.rs`). `CLIP: CLIP_MODEL_SLUG` + `CLIP_MODEL_MANIFEST_NAME` live directly in `lib.rs`. When a third model is added, the author must choose one of three homes.

**Verification (F6)**: `present: yes` — CLIP constants at `lib.rs:23-26`; NIMA at `model_bytes.rs:13,21`.

**Remediation**: Move `CLIP_MODEL_SLUG` and `CLIP_MODEL_MANIFEST_NAME` to `model_bytes.rs` alongside the NIMA constants and add them to the `pub use model_bytes::{...}` re-export line in `lib.rs`. One consistent home: `model_bytes.rs` holds all per-model slug + manifest-name constants.

---

## Theme G — `from_f32_le_bytes(&[])` empty-bytes path not tested [MEDIUM]

- [pr-review-toolkit:pr-test-analyzer]: MEDIUM — `embedding.rs:97-100` has a dedicated `is_empty()` guard that returns `EmbeddingEmpty`. This path has no unit test. The existing `from_f32_le_bytes_rejects_non_aligned` test covers misaligned bytes but not the empty-slice case.

**Verification (F7)**: `present: yes` — no `from_f32_le_bytes(&[])` test exists in embedding.rs.

**Remediation**: Add:
```rust
#[test]
fn from_f32_le_bytes_rejects_empty() {
    let err = ImageEmbedding::from_f32_le_bytes(&[]).unwrap_err();
    assert!(matches!(err, Error::EmbeddingEmpty));
}
```

---

## Theme H — `as_slice` dead_code allow reason is inaccurate [LOW]

- [feature-dev:code-reviewer]: LOW — `embedding.rs:49-51`: reason says "used by mobileclip.rs (D1c)". But `mobileclip.rs` does NOT call `as_slice` — it operates on `raw_emb: Vec<f32>` directly and passes `&raw_emb` to `from_raw`. The `mobileclip.rs (D1c)` claim is factually wrong now that D1c is shipped.
- [pr-review-toolkit:comment-analyzer]: LOW (same finding).

**Remediation**: Update the reason to: `"called only by tests; will be used by dedup.rs (threshold_cluster, D3) — not yet implemented"`.

---

## Theme I — `from_manifest` docstring stale after filename-field change [LOW]

- [pr-review-toolkit:comment-analyzer]: LOW — `model_bytes.rs:43-44` says "Reads `{model_dir}/{name}.onnx` and verifies its SHA-256" but the implementation now reads the `filename` field from manifest first, falling back to `{name}.onnx`.

**Remediation**: Update the docstring to: "Reads the model file specified by the `filename` field in `manifest.toml` under the `[{name}]` section (defaulting to `{name}.onnx` if absent), and verifies its SHA-256 against the manifest."

---

## Theme J — `MobileClipInferenceFailed` / `InferenceFailed` structural duplication [LOW]

- [pr-review-toolkit:type-design-analyzer]: LOW — both variants have identical fields `{ path: PathBuf, source: Box<dyn Error + Send + Sync> }`. A third model would add a third copy-paste variant. Acceptable for v0.1 (two models, distinct match arms), but note for the session that adds a third model.

**Remediation**: No action for v0.1. Note for the session that adds a third inference model: unify into `InferenceFailed { model: &'static str, path, source }`.

---

## Theme K — `MobileClip` (and `Nima`) missing manual `Debug` impl [LOW]

- [pr-review-toolkit:type-design-analyzer]: LOW — neither `MobileClip` nor `Nima` derive or implement `Debug`. They hold `Arc<[u8]>` (85 MB or 12 MB of model bytes). A naive `Debug` derive would dump the full byte slice. Any downstream struct holding `MobileClip` that tries `#[derive(Debug)]` will fail to compile.

**Remediation**: Add a manual `impl fmt::Debug for MobileClip` that prints `MobileClip { bytes: {} B }` using `self.bytes.len()`. Same for `Nima`.

---

## Theme L — Retry behavior not documented in `mobileclip.rs` [LOW]

- [feature-dev:code-architect]: LOW — the thread-local lazy-init pattern in `mobileclip.rs:112-122` has no comment explaining that construction failures leave `guard` as `None` and subsequent calls retry. `nima.rs` has the same gap.

**Remediation**: Add comment at `mobileclip.rs:112`: `// If construction fails, guard stays None; next embed() call on this thread retries.`

---

## Theme M — Additional test gaps [LOW]

- [pr-review-toolkit:pr-test-analyzer]: LOW items:
  - No test for `cosine_similarity` returning negative values (antipodal unit vectors).
  - `EmbeddingZeroVector` error path untested (requires ort output mocking).
  - No negative-path integration tests (degenerate 1×1 RGB input).

**Remediation (batched)**: In the D1c companion tests: add `cosine_similarity_antipodal_returns_negative` test. Document the `EmbeddingZeroVector` gap with a TODO comment. The 1×1 integration test is low priority.

---

## Disposition summary

| Theme | Severity | Fix location |
|---|---|---|
| A — TD-020 not filed | CRITICAL | TECH-DEBT.md |
| B — EmbeddingCorruptBytes missing | HIGH | error.rs + embedding.rs |
| C — extract_field no tests | HIGH | model_bytes.rs |
| D — MobileClip::new missing model_path | MEDIUM | mobileclip.rs |
| E — No logging on model load failure | MEDIUM | mobileclip.rs |
| F — CLIP constants in wrong module | MEDIUM | model_bytes.rs + lib.rs |
| G — from_f32_le_bytes(&[]) untested | MEDIUM | embedding.rs tests |
| H — as_slice docstring stale | LOW | embedding.rs |
| I — from_manifest docstring stale | LOW | model_bytes.rs |
| J — InferenceFailed duplication | LOW | defer to third-model session |
| K — MobileClip missing Debug | LOW | mobileclip.rs + nima.rs |
| L — retry behavior undocumented | LOW | mobileclip.rs |
| M — test gaps | LOW | embedding.rs + integration_clip.rs |

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
    - finding_id: F1
      file: TECH-DEBT.md / crates/photohelper-ai/src/mobileclip.rs
      line: 66
      present: yes
      retain: yes
      reason: "TD-020 referenced at mobileclip.rs:66; absent from TECH-DEBT.md"
      evidence_snippet: "Bilinear resize to 224×224 (TD-020: bicubic center-crop deferred)."
    - finding_id: F2
      file: crates/photohelper-ai/src/embedding.rs
      line: 103
      present: yes
      retain: yes
      reason: "EmbeddingNotNormalized { norm: f32::NAN } returned for non-multiple-of-4 bytes"
      evidence_snippet: "return Err(Error::EmbeddingNotNormalized {\n    norm: f32::NAN, // sentinel: indicates corrupt deserialization\n});"
    - finding_id: F3
      file: crates/photohelper-ai/src/model_bytes.rs
      line: 0
      present: yes
      retain: yes
      reason: "No #[cfg(test)] block in model_bytes.rs; extract_field untested"
      evidence_snippet: "(no #[cfg(test)] found)"
    - finding_id: F4
      file: crates/photohelper-ai/src/mobileclip.rs
      line: 57
      present: yes
      retain: yes
      reason: "MobileClip::new takes only &VerifiedModelBytes; Nima::new takes &VerifiedModelBytes + PathBuf"
      evidence_snippet: "pub fn new(model: &VerifiedModelBytes) -> Self"
    - finding_id: F5
      file: crates/photohelper-ai/src/mobileclip.rs
      line: 0
      present: yes
      retain: yes
      reason: "Zero tracing:: calls in mobileclip.rs; model load failure not logged"
      evidence_snippet: "(no tracing:: found)"
    - finding_id: F6
      file: crates/photohelper-ai/src/lib.rs
      line: 23
      present: yes
      retain: yes
      reason: "CLIP constants at lib.rs:23-26; NIMA constants at model_bytes.rs:13,21"
      evidence_snippet: "pub const CLIP_MODEL_SLUG: &str = \"clip-vit-b32-laion2b-v1\";"
    - finding_id: F7
      file: crates/photohelper-ai/src/embedding.rs
      line: 129
      present: yes
      retain: yes
      reason: "No from_f32_le_bytes(&[]) test in the test module"
      evidence_snippet: "(no such test found)"
```
