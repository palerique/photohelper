# Session 04 — plan review, Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "Opus 4.7 [1m]"
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
| CRITICAL | 6 | T1, T2, T3, T4, T5, T6 |
| HIGH | 13 | T7–T19 |
| MEDIUM | 10 | T20–T29 |
| LOW | 3 | T30–T32 |

---

## PR1-T1 — `get_or_insert_with` is incompatible with fallible `Session` construction (CRITICAL)

**Agents**: code-reviewer, silent-failure-hunter
**File**: `docs/plans/session-04.md:144`

Plan says: `borrow_mut(); .get_or_insert_with(|| Session::new(bytes))`. `get_or_insert_with` takes `FnOnce() -> T` (infallible). `Session::builder().commit_from_memory(bytes)` returns `Result<Session, ort::Error>`. The only way to satisfy `get_or_insert_with`'s signature is to call `.unwrap()` or `.expect()` — a **panic on a production path**, violating CLAUDE.md. A failed session construction would poison the rayon worker or silently not increment any counter.

The session-03 plan v4 (lines 558-576) spelled out the correct pattern: `if borrow.is_none() { match build_session(bytes) { Ok(s) => *borrow = Some(s), Err(e) => return Err(Error::InferenceFailed(...)) } }` inside a `with(|cell| -> Result<NimaScore, Error> { ... })` closure so `?` propagates cleanly.

**Remediation**: Replace line 144 with the session-03 `if borrow.is_none() { match ... }` pattern. Specify that construction failure propagates as `Error::InferenceFailed` and increments `infer_failed`. The `thread_local!` slot stays `None` so the next photo retries construction.

---

## PR1-T2 — Existing `stub_subcommands_exit_69` test will break after D3 (CRITICAL)

**Agents**: pr-test-analyzer
**Files**: `docs/plans/session-04.md:D3`, `crates/photohelper-cli/tests/cli.rs:324`

The test at `cli.rs:324` iterates over `["cull", "develop", "export", "run", "models", "camera"]` and asserts each exits 69 with the stub message. After D3 wires `cull` as a real subcommand, `"cull"` will no longer exit 69. The plan does not mention removing `"cull"` from this list.

**Remediation**: D3 must explicitly state: "Remove `"cull"` from `stub_subcommands_exit_69_with_not_yet_implemented_message` (leaving `["develop", "export", "run", "models", "camera"]`)."

---

## PR1-T3 — `cull_strict_exits_nonzero_on_decode_fail` fixture setup is ambiguous (CRITICAL)

**Agents**: pr-test-analyzer, general-purpose (cross-cutting)
**File**: `docs/plans/session-04.md:232`

The test row reads "non-CR3 in catalog → strict exit ≠ 0". But `ingest` filters to `RAW_EXTS = ["cr3"]`. A non-CR3 cannot enter the catalog via normal ingest. The test description is infeasible as written; the correct setup (per session-03 plan lines 615-616) is: **ingest the synthetic 0xCC-byte `.cr3` fixture** (which `ingest` accepts by extension), then `cull --strict` — `read_raw_rgb` fails on the invalid magic bytes → `decode_failed++` → strict exit ≠ 0.

**Remediation**: Amend the test row to: "Ingest the synthetic 0xCC-byte `.cr3` fixture (already used by `fixture_dir_with_one_cr3`), then run `cull --strict`; `read_raw_rgb` fails → `decode_failed > 0` → exit ≠ 0."

---

## PR1-T4 — FFI layer missing `libraw_dcraw_process`, `libraw_dcraw_make_mem_image`, `libraw_dcraw_clear_mem` (CRITICAL)

**Agents**: feature-dev:code-architect
**Files**: `docs/plans/session-04.md:D1e`, `crates/photohelper-raw/src/ffi.rs:56`

D1e says `read_raw_rgb` "Opens with `LibrawGuard`, calls `libraw_dcraw_process`, `libraw_dcraw_make_mem_image`". But the `unsafe extern "C"` block in `ffi.rs` (lines 56–81) does not bind ANY of these three functions. They must be added for D1e to be implementable.

Additionally, the plan does not specify:
1. Whether `libraw_processed_image_t` is mirrored as a `#[repr(C)]` struct in Rust OR accessed via new C-shim accessors (following the existing shim pattern in `cpp/photohelper_libraw_shim.c`)
2. The name `libraw_dcraw_clear_mem` for freeing the buffer
3. That LibRaw's default `output_bps` is 8 (no need to call `libraw_set_output_bps`), but a defensive runtime check `assert!(processed.bits == 8 && processed.colors == 3)` is needed

**Remediation**: D1e must specify: (1) add three `extern "C"` bindings; (2) choose the shim approach (recommended: add C-shim accessors for `libraw_processed_image_t` fields per the existing pattern, avoiding a `#[repr(C)]` struct); (3) name `libraw_dcraw_clear_mem` explicitly as the deallocation function; (4) add a runtime assert on `bits == 8 && colors == 3`.

---

## PR1-T5 — `cull_scores` column names diverge from session-03's binding schema (CRITICAL)

**Agents**: general-purpose (cross-cutting)
**Files**: `docs/plans/session-04.md:41`, `docs/plans/session-03.md:373`

Session-03 plan v4 (the binding design source) defines the schema:
```sql
cull_scores (photo_id, scorer TEXT, score REAL, scored_at_unix_seconds INTEGER, PK(photo_id, scorer))
```
Session-04 uses: `model_slug TEXT`, `aesthetic_score REAL`, `scored_at INTEGER`, PK `(photo_id, model_slug)`.

Three column names differ, the PK references the wrong column, and these names are embedded throughout D2a, D2b, D3, and the `insert_cull_score` API. The plan claims §Binding design decisions are from session-03 v4 but silently changes the schema.

**Remediation**: Either adopt session-03's exact names (`scorer`, `score`, `scored_at_unix_seconds`) OR explicitly declare the renaming with rationale. The Rust API (`insert_cull_score` parameters) must also be consistent with whichever names are chosen.

---

## PR1-T6 — `Catalog::all_rows()` returns superseded photos; plan claims "unsuperseded" (CRITICAL)

**Agents**: feature-dev:code-reviewer
**Files**: `docs/plans/session-04.md:197`, `crates/photohelper-catalog/src/catalog.rs:469`

D3 says `run_cull` does `rayon par_bridge over Catalog::all_rows() ... per-row: read_raw_rgb → Nima::score`. D3 line 50 says "Reads `photos` catalog rows (unsuperseded)". But `all_rows()` executes `SELECT ... FROM photos ORDER BY ingested_at_unix_seconds` with NO `WHERE superseded_at_unix_seconds IS NULL` filter.

Session-03 plan v4 (line 535-541) specified a custom SQL:
```sql
SELECT id, source_path FROM photos
WHERE superseded_at_unix_seconds IS NULL
  AND id NOT IN (SELECT photo_id FROM cull_scores WHERE scorer = ?1)
```
Using `all_rows()` will decode + score superseded photos (wasted compute; double-billing on re-ingested files).

**Remediation**: Add `Catalog::unsuperseded_unscored_rows(scorer: &str) -> Result<Vec<CullRow>>` implementing the session-03 SQL, OR at minimum `Catalog::unsuperseded_rows()` with `WHERE superseded_at_unix_seconds IS NULL`. D3 must reference this new method, not `all_rows()`.

---

## PR1-T7 — `RgbImage` location ambiguous; circular/inverted dependency if in `photohelper-ai` (HIGH)

**Agents**: feature-dev:code-architect, pr-review-toolkit:code-simplifier
**Files**: `docs/plans/session-04.md:140,163`

D1c lists `RgbImage` as an `photohelper-ai` deliverable (line 140). D1e specifies `read_raw_rgb` in `photohelper-raw` returning `RgbImage` (line 163). If `RgbImage` is in `photohelper-ai`, then `photohelper-raw` depends on `photohelper-ai` — an inverted dependency (raw decode should not depend on AI inference). If `RgbImage` is in `photohelper-raw`, D1c's attribution is wrong.

The natural home is `photohelper-core` (alongside `PhotoId`, `AbsPath`, `RawImage`) — both `photohelper-raw` and `photohelper-ai` already depend on it.

**Remediation**: Declare `RgbImage` in `photohelper-core::model` (or a new `photohelper-core::image` module). D1c should say "`RgbImage` (from `photohelper-core`; used by `Nima::score`)". D1e should say `read_raw_rgb(path) -> Result<RgbImage, Error>` where `RgbImage` is from `photohelper-core`.

---

## PR1-T8 — Bilinear resize: no dependency or algorithm specified; "in-place" wording is wrong (HIGH)

**Agents**: feature-dev:code-architect
**File**: `docs/plans/session-04.md:142`

D1c says "resizes to 224×224 (bilinear, in-place in pixels buffer)". Three problems:
1. No dependency for resizing is declared. The `image` crate is not in `Cargo.toml`. A hand-rolled bilinear resize is ~50-80 LoC but must be specified.
2. "In-place" is wrong: input is ~72 MB (6000×4000×3), output is 151 KB (224×224×3). This is not in-place; it allocates a new buffer.
3. `Nima::score` takes `&RgbImage` (immutable reference), making true in-place transformation impossible by the signature.

**Remediation**: Specify the resize approach: either (a) add `image = { version = "0.25", default-features = false }` to `photohelper-ai/Cargo.toml` and use `imageops::resize`, or (b) implement a private `bilinear_resize(src: &RgbImage, w: u32, h: u32) -> Vec<u8>` (~60 LoC). Change "in-place" to "into a new 224×224 buffer".

---

## PR1-T9 — `CullStats` field list diverges from session-03's binding enumeration (HIGH)

**Agents**: general-purpose, pr-review-toolkit:silent-failure-hunter
**Files**: `docs/plans/session-04.md:54`, `docs/plans/session-03.md:499`

Session-03 v4 enumerated 8 fields: `in_flight`, `scored`, `inference_failed`, `decode_failed`, `file_missing`, `content_changed`, `catalog_inconsistency`, `derive_failed`.

Session-04 lists 8 different fields: `walked`, `scored`, `already_scored`, `decode_failed`, `infer_failed`, `catalog_written`, `catalog_inconsistency`, `derive_failed`.

Silently dropped: `in_flight` (heartbeat counter), `file_missing` (file-missing dispatch), `content_changed` (content-change detection). Added without declaration: `walked`, `already_scored`, `catalog_written`. Renamed: `inference_failed` → `infer_failed`.

Losing `file_missing` and `content_changed` means the per-photo dispatch table from session-03 (lines 599-604) has no counters for those paths — they would be silently absorbed. Losing `in_flight` removes the heartbeat's per-photo liveness signal.

**Remediation**: Either reconcile with session-03's field list (restore `in_flight`, `file_missing`, `content_changed`; rename is acceptable if declared), OR explicitly document each field change as an amendment with rationale. The summary line and `--strict` predicate must be consistent with the final field list.

---

## PR1-T10 — NIMA preprocessing normalization contradicts session-03 (unacknowledged) (HIGH)

**Agents**: general-purpose, feature-dev:code-architect, pr-review-toolkit:type-design-analyzer
**File**: `docs/plans/session-04.md:143`

Session-03 plan v4 (lines 256-258) specified ImageNet mean/std normalization: `(pixel/255 - mean[c]) / std[c]`. Session-04 D1c specifies MobileNet `preprocess_input`: `pixel / 127.5 - 1.0` (scaling to `[-1,1]`).

These are different formulas. Session-04 is correct for the `idealo/image-quality-assessment` model (TF MobileNet uses `preprocess_input` = `[-1,1]`). But session-04 claims to inherit session-03's binding decisions without acknowledging this correction.

**Remediation**: Add a note in D1c: "Preprocessing uses MobileNet `preprocess_input` (`pixel / 127.5 - 1.0` per channel, output range `[-1,1]`). Session-03 plan v4 lines 256-258 incorrectly specified ImageNet mean/std normalization; this supersedes that specification."

---

## PR1-T11 — `run_cull` return type changed from `anyhow::Result<u8>` to `ExitCode` without amendment (HIGH)

**Agents**: general-purpose (cross-cutting)
**File**: `docs/plans/session-04.md:88`

Session-03 plan v4 (line 492-493) specified `pub fn run_cull(...) -> anyhow::Result<u8>`. Session-04 binding decision #2 says `-> ExitCode`. This is a material API change (errors are handled internally vs propagated). The plan claims "these are locked — not up for re-evaluation" while silently changing the locked decision.

**Remediation**: Declare the amendment: "`ExitCode` return type (not `anyhow::Result<u8>`) aligns `run_cull` with `run_ingest`'s pattern in the existing codebase; session-03 plan v4 is superseded on this point." The `main.rs` error path handling must also be clarified.

---

## PR1-T12 — `RgbImage` uses `u32` dimensions; zero-dimension images bypass the invariant (HIGH)

**Agents**: pr-review-toolkit:type-design-analyzer
**Files**: `docs/plans/session-04.md:140`, `crates/photohelper-raw/src/decode.rs:141`

`RgbImage { pixels: Vec<u8>, width: u32, height: u32 }` allows `width=0` or `height=0`. `0 == 0 * 0 * 3` passes the invariant check, producing a 0-sized image that then causes division-by-zero in the bilinear resize scale factor.

`BayerPlane` (same crate) uses `NonZeroU32` for both dimensions (verified at `decode.rs:141`). `RgbImage` should follow this established pattern.

**Remediation**: Use `NonZeroU32` for `RgbImage.width` and `RgbImage.height`, OR (if keeping `u32`) explicitly reject `width == 0 || height == 0` at construction AND compute the expected pixel count as `u64::from(width) * u64::from(height) * 3` to avoid overflow (matching `BayerPlane::new`'s pattern).

---

## PR1-T13 — `InsertScoreOutcome` branch detection mechanism unspecified (HIGH)

**Agents**: pr-review-toolkit:type-design-analyzer, feature-dev:code-reviewer
**File**: `docs/plans/session-04.md:183`

D2b specifies `INSERT OR IGNORE` semantics and `InsertScoreOutcome: { Inserted, AlreadyScored }` — but does not specify HOW to distinguish the two outcomes. `INSERT OR IGNORE` does not tell the caller which branch fired. The standard SQLite mechanism is `conn.changes()` after the statement: `1` = inserted, `0` = ignored.

Without this specification, an implementer may always return `Inserted` (breaking the `already_scored` counter) or add an expensive pre-SELECT.

**Remediation**: Add to D2b: "Distinguish outcomes via `conn.changes()` after `INSERT OR IGNORE`: `1` → `Inserted`; `0` → `AlreadyScored`. This is the single-statement approach, avoiding the pre-SELECT round-trip."

---

## PR1-T14 — `--strict` predicate excludes `derive_failed`; contradicts session-03 dispatch table (HIGH)

**Agents**: pr-review-toolkit:silent-failure-hunter, feature-dev:code-reviewer
**File**: `docs/plans/session-04.md:57`

D3 line 57: `--strict cull: exits non-zero if decode_failed + infer_failed > 0`. Session-03 plan v4 dispatch table (line 602) says `derive_failed` SHOULD fail under `--strict`. A file whose `PhotoId` cannot be re-derived has been corrupted since ingest — this is exactly the condition `--strict` should surface.

**Remediation**: Amend line 57 to: "`--strict` exits non-zero if `decode_failed + infer_failed + derive_failed > 0`. `catalog_inconsistency` does NOT trigger strict failure (transient FK violation is not an error in the scored photo set)."

---

## PR1-T15 — `nima_scores_cc0_r8_cr3_fixture` tolerance strategy is [1.0,10.0] — the entire valid domain (HIGH)

**Agents**: pr-review-toolkit:pr-test-analyzer
**Files**: `docs/plans/session-04.md:224`, `docs/discovery-notes.md:DN-025`

Test row says "score ∈ [1.0, 10.0], deterministic". But `[1.0, 10.0]` is the full valid range of `NimaScore` — a catastrophically broken model would still pass this check. DN-025 specifies a two-tier strategy: Apple Silicon golden vector with `±1e-3` tolerance; Linux x86_64 CI band assertion `score ∈ [3.0, 9.0]`.

**Remediation**: Replace with: "Apple Silicon: `abs(score - golden) < 1e-3`. Linux x86_64 CI: `score ∈ [band_low, band_high]` (bounds determined by D0' actual measurements on the CC0 fixtures). Add `just nima-regenerate-golden` recipe for golden-vector recovery."

---

## PR1-T16 — `read_raw_rgb_cc0_fixture_dimensions` has no content plausibility check (HIGH)

**Agents**: pr-review-toolkit:pr-test-analyzer
**File**: `docs/plans/session-04.md:226`

The test row asserts only `len == w*h*3`. An all-zeros buffer, a padded buffer, or a buffer with a LibRaw copy bug would pass. Existing `read_raw` integration tests (`integration_cr3.rs`) check pixel-content properties (white balance, CFA pattern, color matrix non-identity) — `read_raw_rgb` should maintain this standard.

**Remediation**: Add a content plausibility assertion: "mean pixel value ∈ (20, 240)" (rules out all-zeros and all-max degenerate outputs) AND "standard deviation > 5" (rules out flat images). These are cheap and detect silent LibRaw copy bugs.

---

## PR1-T17 — `derive_failed` counter is dead weight without `content_changed` counterpart (HIGH)

**Agents**: pr-review-toolkit:code-simplifier, pr-review-toolkit:silent-failure-hunter
**File**: `docs/plans/session-04.md:55`

`derive_failed` tracks I/O failures during `PhotoId::derive` re-derivation. But session-04 drops `content_changed` (the counter that fires when re-derived ID ≠ catalog ID) and never shows the re-derivation logic in D3. Without `content_changed`, the re-derivation step can only fail — its success path has no counter. This means `derive_failed` is either dead code (if the re-derivation step is dropped) or broken (if kept without its companion counter).

**Remediation**: Either (a) restore `content_changed` as a 9th counter and document the re-derivation logic in D3 (`if current_id != catalog_id { content_changed++; continue }`), OR (b) drop the re-derivation step AND `derive_failed` entirely for v0.1 (file deletion/corruption → `read_raw_rgb` fails → `decode_failed` catches it cleanly without re-derivation).

---

## PR1-T18 — D0' specifies `rawpy` for CR3 decode but rawpy is not installed anywhere; no D0' script exists (HIGH)

**Agents**: pr-review-toolkit:comment-analyzer
**File**: `docs/plans/session-04.md:110`

D0' says "via Python onnxruntime + rawpy decode". `rawpy` is a separate PyPI package (Python bindings for LibRaw). It is not in `scripts/convert-nima-to-onnx.sh`'s `pip install` line. No D0' verification script exists in the repo. On arm64 macOS, `rawpy` requires a LibRaw installation accessible to pip — potentially conflicting with the vendored LibRaw 0.22.1.

**Remediation**: Either (a) create `scripts/verify-nima-d0-prime.sh` that installs `rawpy onnxruntime numpy` and decodes both CC0 fixtures, running NIMA + recording scores; OR (b) replace `rawpy` with `Pillow` + a TIFF intermediate from LibRaw's command-line tools (avoids the pip/LibRaw conflict). Reference the script in D0' and add it to §Prerequisites.

---

## PR1-T19 — Existing `open_schema_version_too_new_returns_error` test will break silently (HIGH)

**Agents**: feature-dev:code-architect, pr-review-toolkit:pr-test-analyzer
**Files**: `crates/photohelper-catalog/src/catalog.rs:560`, `docs/plans/session-04.md:D2a`

The test at `catalog.rs:560` creates a DB with `PRAGMA user_version = 2` and asserts `CatalogSchemaTooNew { found: 2, expected: 1 }`. When `SCHEMA_VERSION` becomes 2, `user_version = 2` becomes the CURRENT valid version — the test will PASS (no error returned), silently losing its `CatalogSchemaTooNew` coverage.

Additionally, `open_init_idempotent` (`catalog.rs:579`) asserts `v == 1` after a fresh open — will also fail.

**Remediation**: D2a must explicitly state: "Update `open_schema_version_too_new_returns_error` to use `user_version = 3` (or `SCHEMA_VERSION + 1`). Update `open_init_idempotent` assertion from `v == 1` to `v == 2`. Prefer `SCHEMA_VERSION + 1` over hardcoded values for forward-compatibility."

---

## PR1-T20 — DN-026 still marked BLOCKER in `docs/discovery-notes.md`; plan claims "resolved" (MEDIUM)

**Agents**: pr-review-toolkit:comment-analyzer
**Files**: `docs/plans/session-04.md:20`, `docs/discovery-notes.md:215`

Plan §Prerequisites line 20: "DN-026 is the only resolved blocker (tf2onnx Path A executed)." `docs/discovery-notes.md:215` still reads: `**BLOCKER** — D0 ABORT triggered. AI culling pipeline (D1–D4) halted until resolved.`

The ledger was not updated when the conversion script was run pre-session.

**Remediation**: D0' already closes DN-026 in the discovery-notes as one of its deliverables (plan line 113). Correct line 20 of the plan to: "DN-026 resolution path executed (ONNX model generated, SHA-256 verified); **formal closure in D0'**." This removes the contradiction.

---

## PR1-T21 — `ModelRegistry`/`LoadedModel` two-phase architecture silently collapsed (MEDIUM)

**Agents**: general-purpose (cross-cutting)
**File**: `docs/plans/session-04.md:§Binding design decisions`

Session-03 v4 (lines 183-211) specified a three-type hierarchy: `VerifiedModelBytes` → `LoadedModel` → `Nima`. Session-04 collapses to: `VerifiedModelBytes` → `Nima` (D1c has no `LoadedModel`). The plan claims "these are locked — not up for re-evaluation" but omits `LoadedModel` entirely.

**Remediation**: Declare the simplification: "Session-04 removes `LoadedModel` (was an intermediate type in session-03 v4). `Nima` wraps `VerifiedModelBytes` directly and constructs per-worker `Session` in its `thread_local!`. This reduces the type count without losing the SHA-256 trust boundary."

---

## PR1-T22 — `NimaInferenceCause` type name has no precedent; error variant names diverge (MEDIUM)

**Agents**: general-purpose (cross-cutting)
**File**: `docs/plans/session-04.md:33`

Line 33 lists `NimaInferenceCause` as a type. But D1c (lines 138-139) defines the Error enum variants as `ModelSha256Mismatch`, `ManifestParse`, `ModelLoad`, `InferenceFailed`. Session-03 plan used different names: `ModelLoadFailed`, `ModelVerificationFailed`, etc. `NimaInferenceCause` does not appear in D1c's own spec.

**Remediation**: Either (a) clarify that `NimaInferenceCause` is the nested cause enum for `InferenceFailed`'s `source` field, or (b) remove the reference from line 33 if it is just an alias for the `Error` enum. Align error variant names between line 33 and D1c's Error enum spec.

---

## PR1-T23 — Decision-doc 0002 missing from deliverables; decision-doc 0001 has stale migration text (MEDIUM)

**Agents**: general-purpose (cross-cutting)
**File**: `docs/plans/session-04.md:§Out of scope`

Session-03 plan v4 scoped `docs/decisions/0002-catalog-schema-v2.md` as D2c (never created — D0 ABORT). Session-04 inherits the v2 migration work but lists no decision-doc 0002 deliverable. Decision-doc 0001 §Migration policy still references the abandoned `Vec<&'static dyn Migration>` design (session-03 plan-review amended this to match-arm approach).

**Remediation**: Add `docs/decisions/0002-catalog-schema-v2.md` as a D7 (or D2c) deliverable. Amend decision-doc 0001 §Migration policy to replace `Vec<&'static dyn Migration>` language with the match-arm approach.

---

## PR1-T24 — `from_catalog_f64` constructor missing from D1c (MEDIUM)

**Agents**: pr-review-toolkit:type-design-analyzer
**File**: `docs/plans/session-04.md:D1c`

Session-03 plan v4 (line 250-253) specified `NimaScore::from_catalog_f64(f64) -> Result<Self>` with separate saturation semantics and a WARN on `|delta| > 1e-6`. The `cull_scores.score REAL` column stores `f64` in SQLite. Without `from_catalog_f64`, any code reading scores back from the DB cannot construct a `NimaScore`.

**Remediation**: Add to D1c: "`NimaScore::from_catalog_f64(v: f64) -> Result<Self>`: saturating cast to `f32`, WARN if `|f32_val - v as f32| > 1e-6`, then validate range. Returns `Err(InferenceFailed)` if out of `[1.0, 10.0]`."

---

## PR1-T25 — `catalog_fresh_db_initializes_to_v2` test missing; two existing tests will break (MEDIUM)

**Agents**: pr-review-toolkit:pr-test-analyzer
**Files**: `docs/plans/session-04.md:§Test plan`, `crates/photohelper-catalog/src/catalog.rs:560,579`

`open_schema_version_too_new_returns_error` (T19) and `open_init_idempotent` (asserts `v == 1`) both break when `SCHEMA_VERSION` becomes 2. The plan's test-plan table covers the v1→v2 upgrade path but not the fresh-DB `0→2` path (init_v1 then apply_v1_to_v2).

**Remediation**: Add test row: "`catalog_fresh_db_initializes_to_v2` — open a Catalog at a non-existent path; assert `PRAGMA user_version = 2`; assert both `photos` and `cull_scores` tables exist." Note updates to both existing tests in D2a.

---

## PR1-T26 — `verified_model_bytes_missing_manifest` test missing (MEDIUM)

**Agents**: pr-review-toolkit:pr-test-analyzer
**File**: `docs/plans/session-04.md:§Test plan`

The test plan includes `verified_model_bytes_sha_mismatch` (SHA wrong → Err) but not the "missing `manifest.toml`" failure mode. This is the first error a new contributor would hit when cloning without LFS or without running the conversion script.

**Remediation**: Add test row: "`verified_model_bytes_missing_manifest` — `from_manifest` with no `manifest.toml` in model_dir returns `Err`; error message includes the expected file path."

---

## PR1-T27 — Migration SQL should use `CREATE TABLE IF NOT EXISTS` for idempotency (MEDIUM)

**Agents**: feature-dev:code-architect
**File**: `docs/plans/session-04.md:D2a`

The plan says `apply_v1_to_v2` "creates `cull_scores` table." If the migration is interrupted after table creation but before `PRAGMA user_version = 2`, the next `Catalog::open` would re-run the migration and `CREATE TABLE` would fail with "table already exists." The existing `INIT_SQL` pattern uses `CREATE TABLE IF NOT EXISTS`.

**Remediation**: D2a should specify: "`CREATE TABLE IF NOT EXISTS cull_scores` and `CREATE INDEX IF NOT EXISTS` throughout `apply_v1_to_v2`, matching the existing `INIT_SQL` pattern."

---

## PR1-T28 — ort dep features list missing `"std"` (MEDIUM)

**Agents**: pr-review-toolkit:comment-analyzer
**File**: `docs/plans/session-04.md:121`

D1a specifies `features = ["ndarray"]`. Every ort `default-features = false` example includes `"std"` alongside `"ndarray"`. Without `"std"`, ort attempts a `no_std` build which will not compile for a desktop CLI.

**Remediation**: D1a feature list: `features = ["std", "ndarray"]`.

---

## PR1-T29 — `verify-model-sha256.sh` redundancy with runtime check should be acknowledged (MEDIUM)

**Agents**: pr-review-toolkit:code-simplifier
**File**: `docs/plans/session-04.md:D1d`

Both `VerifiedModelBytes::from_manifest` (runtime) and `verify-model-sha256.sh` (CI gate) verify the same SHA-256. The redundancy is not wrong but is worth documenting. Importantly, the shell script must handle the LFS pointer case (file is 130-byte pointer, not 12 MB ONNX) with an actionable error message — `sha256sum` on a pointer will produce a hash mismatch, not a clear "run git lfs pull" message.

**Remediation**: D1d should note: "Shell gate provides fast CI fail without building the full binary. The script MUST detect LFS pointers (file < 1 MB) and emit an actionable `git lfs pull` message rather than a cryptic SHA mismatch."

---

## PR1-T30 — TD-011 trigger wording relaxed from "session 05" to "session 06" (LOW)

**Agents**: general-purpose
**File**: `docs/plans/session-04.md:74`

Session-03's binding obligation: "by session 05 AT LATEST" (3-session bound: sessions 03/04/05). Session-04 says "by session 06." Silently extending the trigger beyond the binding obligation.

**Remediation**: Change "by session 06" to "by session 05" to match the binding trigger from session-03.

---

## PR1-T31 — "In-place" resize wording contradicts `&RgbImage` signature (LOW)

**Agents**: feature-dev:code-architect
**File**: `docs/plans/session-04.md:142`

`Nima::score(rgb: &RgbImage)` takes an immutable reference. "In-place in pixels buffer" implies mutation. Covered by T8; cosmetic fix only.

**Remediation**: Replace "in-place in pixels buffer" with "into a new local 224×224 buffer."

---

## PR1-T32 — `nima_score_out_of_range_rejected` test should explicitly list NaN (LOW)

**Agents**: pr-review-toolkit:pr-test-analyzer
**File**: `docs/plans/session-04.md:§Test plan`

The test row lists `from_f32(-0.1)` and `from_f32(10.1)` → Err. D1c says `from_f32` also rejects NaN, but NaN is a separate code path.

**Remediation**: Add `from_f32(f32::NAN)` to the test row.

---

## Disposition summary

| Theme | Severity | Action |
|---|---|---|
| T1 thread_local get_or_insert_with | CRITICAL | Remediate R1 |
| T2 stub test will break | CRITICAL | Remediate R1 |
| T3 decode-fail test fixture | CRITICAL | Remediate R1 |
| T4 FFI missing dcraw bindings | CRITICAL | Remediate R1 |
| T5 column name divergence | CRITICAL | Remediate R1 |
| T6 all_rows superseded | CRITICAL | Remediate R1 |
| T7 RgbImage location / circular dep | HIGH | Remediate R1 |
| T8 resize no dep/algorithm | HIGH | Remediate R1 |
| T9 CullStats field list divergence | HIGH | Remediate R1 |
| T10 preprocessing normalization | HIGH | Remediate R1 |
| T11 run_cull return type | HIGH | Remediate R1 |
| T12 RgbImage u32 / NonZeroU32 | HIGH | Remediate R1 |
| T13 InsertScoreOutcome mechanism | HIGH | Remediate R1 |
| T14 --strict exclude derive_failed | HIGH | Remediate R1 |
| T15 tolerance strategy incomplete | HIGH | Remediate R1 |
| T16 read_raw_rgb content plausibility | HIGH | Remediate R1 |
| T17 derive_failed dead weight | HIGH | Remediate R1 |
| T18 D0' rawpy missing | HIGH | Remediate R1 |
| T19 existing schema test breaks | HIGH | Remediate R1 |
| T20 DN-026 ledger inconsistency | MEDIUM | Remediate R1 |
| T21 ModelRegistry collapsed | MEDIUM | Remediate R1 |
| T22 NimaInferenceCause divergence | MEDIUM | Remediate R1 |
| T23 decision-doc 0002 missing | MEDIUM | Remediate R1 |
| T24 from_catalog_f64 missing | MEDIUM | Remediate R1 |
| T25 catalog_fresh_db test missing | MEDIUM | Remediate R1 |
| T26 missing_manifest test missing | MEDIUM | Remediate R1 |
| T27 CREATE TABLE IF NOT EXISTS | MEDIUM | Remediate R1 |
| T28 ort features missing std | MEDIUM | Remediate R1 |
| T29 sha256 script redundancy/LFS msg | MEDIUM | Remediate R1 |
| T30 TD-011 trigger drift | LOW | Remediate R1 |
| T31 in-place wording | LOW | Remediate R1 |
| T32 NaN test case missing | LOW | Remediate R1 |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 16
  verified: 16
  drifted: 1
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - {finding_id: T1_thread_local_get_or_insert_with, file: session-04.md, line: 144, present: yes, retain: yes}
    - {finding_id: T2_stub_test_will_break, file: cli.rs, line: 324, present: yes, retain: yes}
    - {finding_id: T4_ffi_missing_dcraw, file: ffi.rs, line: 56, present: yes, retain: yes}
    - {finding_id: T5_column_name_divergence, file: session-04.md, line: 41, present: yes, retain: yes}
    - {finding_id: T5_session03_schema, file: session-03.md, line: 373, present: yes, retain: yes}
    - {finding_id: T6_all_rows_includes_superseded, file: catalog.rs, line: 469, present: yes, retain: yes}
    - {finding_id: T7_rgbimage_location_d1c, file: session-04.md, line: 140, present: yes, retain: yes}
    - {finding_id: T7_rgbimage_location_d1e, file: session-04.md, line: 163, present: drifted, retain: yes-with-corrected-line}
    - {finding_id: T9_cullstats_field_list, file: session-04.md, line: 54, present: yes, retain: yes}
    - {finding_id: T9_session03_field_list, file: session-03.md, line: 499, present: yes, retain: yes}
    - {finding_id: T12_rgbimage_u32_not_nzero, file: session-04.md, line: 140, present: yes, retain: yes}
    - {finding_id: T12_bayerplane_nonzero, file: decode.rs, line: 141, present: yes, retain: yes}
    - {finding_id: T13_insert_or_ignore_mechanism, file: session-04.md, line: 184, present: yes, retain: yes}
    - {finding_id: T18_rawpy_missing, file: session-04.md, line: 110, present: yes, retain: yes}
    - {finding_id: T19_schema_version_test, file: catalog.rs, line: 560, present: yes, retain: yes}
    - {finding_id: T20_dn026_still_blocker, file: discovery-notes.md, line: 215, present: yes, retain: yes}
```

## Round 2 watch-list

After R1 remediation, Round 2 MUST verify:

1. T1: thread_local Session construction uses `if borrow.is_none() { match build_session() { ... } }` pattern — no `get_or_insert_with`, no panic.
2. T2: D3 explicitly removes `"cull"` from stub iteration list.
3. T3: `cull_strict_exits_nonzero_on_decode_fail` setup specifies the synthetic 0xCC fixture.
4. T4: D1e specifies 3 new FFI bindings + shim approach + `libraw_dcraw_clear_mem` naming.
5. T5: `cull_scores` column names consistent throughout (session-03's `scorer/score/scored_at_unix_seconds` OR declared amendment).
6. T6: D3 uses `unsuperseded_unscored_rows(scorer)` or equivalent, NOT `all_rows()`.
7. T7: `RgbImage` placed in `photohelper-core`; D1c/D1e attribution corrected.
8. T8: Resize dep/algorithm specified; "in-place" removed.
9. T9: CullStats field list reconciled with session-03 (or amendments declared).
10. T10: Preprocessing normalization note added (supersedes session-03 ImageNet spec).
11. T11: `run_cull` return type amendment declared.
12. T12: `RgbImage` uses `NonZeroU32` or explicit zero-rejection + u64 arithmetic.
13. T13: `changes()` mechanism specified for `InsertScoreOutcome` distinction.
14. T14: `--strict` predicate includes `derive_failed` (or re-derivation dropped + derive_failed removed).
15. T15: Tolerance strategy references DN-025 two-tier bounds.
16. T16: `read_raw_rgb` test includes content plausibility check.
17. T17: Either `content_changed` restored OR re-derivation step + `derive_failed` dropped.
18. T18: D0' references a D0'-prime script with rawpy (or alternative).
19. T19: D2a explicitly updates two existing tests.
20. T20–T29: All MEDIUM items addressed.
