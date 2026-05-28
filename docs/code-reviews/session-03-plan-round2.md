# Session 03 — plan review, Round 2

> Per `docs/quality-assurance.md § Plan-review protocol`.
> Cadence A → Tier 5 (plan stage), full 8-agent suite fired in parallel against
> `docs/plans/session-03.md` v2 (committed at `dc95639`).
> Findings grouped by **theme** (not by agent). Multi-agent convergence is the
> priority signal.

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

## R2 watch-list from Round 1 — all 10 items VERIFIED PASS

| # | Item | Status |
|---|------|--------|
| 1 | D6 targets `main.rs:127-130`; 5 stubs; message → README.md | PASS |
| 2 | D5c: 5c-i + 5c-ii only; no panic in `heartbeat_loop` | PASS (superseded by T2 below — the spec is present but the implementation path is broken) |
| 3 | DN-022/023/024/025 filed; ANL-001 citation removed | PASS (T4 is a label-swap within filed DNs, not a missing filing) |
| 4 | D4 concurrency picks per-worker Session (option b) | PASS |
| 5 | D4 takes concrete `&Nima` (not `&dyn Scorer`) | PASS |
| 6 | §Stop-gap declarations enumerates 5 TDs (TD-012–TD-016) | PASS |
| 7 | D2b E2E = automated integration test (not manual REPL) | PASS |
| 8 | D0 sequencing = D0 → D1a → D1b/c → D1d | PASS |
| 9 | Schema-version state machine fully specified | PASS |
| 10 | Migration trait replaced with match arm + `apply_v1_to_v2()` | PASS |

---

## Triage summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 3 | Implementation cannot begin until all 3 are remediated in plan v3 |
| **HIGH**      | 10 | Address in plan v3 before any code lands |
| **MEDIUM**    | 9  | Address in plan v3 or defer with binding-triggered TD |
| **LOW**       | 4  | When convenient |

Agent suite: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer` (type),
`pr-review-toolkit:silent-failure-hunter` (sfh), `pr-review-toolkit:comment-analyzer`
(com), `pr-review-toolkit:pr-test-analyzer` (test), `pr-review-toolkit:code-simplifier`
(simp).

---

## CRITICAL

### T1 — Python `onnx` package not installed in CI; D3 sanitize-check will fail on first run (3-way)

**Agents**: rev (CRITICAL), arch (implied), test (implied)

`docs/plans/session-03.md:389` adds to `scripts/sanitize-check.sh`:
```bash
python3 -c "import onnx; m = onnx.load('$ONNX_FILE'); ..."
```

`.github/workflows/ci.yml` test job (lines 61-74) installs
`build-essential pkg-config autoconf libtool libimage-exiftool-perl` — no
`actions/setup-python`, no `pip install onnx`. Ubuntu runners ship Python 3
but NOT the `onnx` PyPI package. `sanitize-check` runs inside `just ci`; on
the first post-merge CI run the job fails with
`ModuleNotFoundError: No module named 'onnx'`. This directly violates CLAUDE.md
"`just ci` runs exactly what CI runs, so green locally == green in CI." The
CLAUDE.md also declares "no Python or Node runtime dependency for end users";
introducing an undocumented Python dev-tooling prerequisite is a process gap.

**Remediation**: Add to D3 sub-deliverables:
1. `.github/workflows/ci.yml` test job: add `actions/setup-python@v5`
   (`python-version: '3.x'`) + `pip install onnx` before the sanitize-check
   step.
2. `scripts/sanitize-check.sh`: gate the ONNX check on availability —
   `python3 -c "import onnx" 2>/dev/null || { echo "ERROR: onnx not installed
   (pip install onnx)"; exit 1; }` — matching the existing exiftool
   availability check pattern.
3. Add `pip install onnx` to README § Development prerequisites.

---

### T2 — `force_heartbeat_panic_in_thread(handle: &JoinHandle<()>)` is not implementable in safe Rust; D5c will fail or silently regress TD-005 (3-way)

**Agents**: sfh (CRITICAL), gp (MEDIUM-escalated), test (HIGH via heartbeat-test architecture conflict)

`docs/plans/session-03.md:489` specifies:
```
pub fn force_heartbeat_panic_in_thread(handle: &JoinHandle<()>)
```
in the `photohelper-test-helpers` crate. `std::thread::JoinHandle<T>` exposes
only `join()`, `is_finished()`, and `thread()` (which yields `id()`, `name()`,
`unpark()`). No API injects a panic into a running thread from outside. Three
implementation paths all fail:

- **(a) Call `panic!()` inside the thread's closure** — impossible from outside
  without the thread's cooperation.
- **(b) Platform `pthread_cancel` / `TerminateThread`** — unsafe, non-portable,
  untestable identically on macOS + Windows.
- **(c) Cooperative flag inside `heartbeat_loop`** — reintroduces a panic site
  in `heartbeat_loop`, which line 487 forbids ("no panic site ever in
  `heartbeat_loop`") and which is the exact pattern TD-005 was filed to prevent.

Additionally, D5e row 4 (lines 522-525) parameterizes the heartbeat-death test
over `[ingest, cull]` drivers via subprocess integration tests. Subprocess tests
have no access to the target thread's `JoinHandle` — making option (c) the only
viable path, which collapses TD-005.

**Remediation**: Drop `force_heartbeat_panic_in_thread`. Instead:
- **D5c-i**: `photohelper-test-helpers` provides a `HeartbeatDeathTrigger`
  struct wrapping an `Arc<AtomicBool>`. A dedicated test thread (NOT
  `heartbeat_loop`) reads the flag and panics when signalled. The test verifies
  the system's RESPONSE to a panicked worker thread (Mutex poison recovery path),
  not a panicked heartbeat thread.
- **D5c-ii**: verify via `JoinHandle::is_finished()` poll on the
  dedicated-panic-thread handle, confirming the WARN fires and the summary still
  prints.
- D5e row 4 parameterization: the `[heartbeat-death-WARN]` tests (via subprocess
  integration) use a NEW approach — an env-var
  `PHOTOHELPER_HEARTBEAT_POISON_TICKS=1` checked in the TEST-HELPER thread (NOT
  `heartbeat_loop`), causing that thread to panic after N ticks. The subprocess
  spawns with this env-var set. Update D5c to state this approach explicitly.

---

### T3 — `CullStats` counter type unspecified; plain `u64` cannot be mutated from rayon workers — compile error or data race (2-way)

**Agents**: type (CRITICAL), arch (implied via IngestStats precedent)

`docs/plans/session-03.md:408` defines `run_cull` returning `CullStats`; lines
429-441 list four mutable per-photo counters: `inference_failed`,
`decode_failed`, `file_missing`, `content_changed`. D4 uses rayon
`par_bridge().for_each(...)` workers — closures are `Fn`, not `FnMut`. Plain
`u64` cannot be incremented from multiple workers:

- `Fn` closures cannot hold `&mut CullStats` → compile error.
- `Arc<Mutex<CullStats>>` is an anti-pattern for simple counters (high
  contention for per-photo increments).
- The established pattern is `Arc<CullStats>` with `AtomicU64` per counter —
  exactly what `IngestStats` uses at
  `crates/photohelper-cli/src/commands/ingest.rs:87-99`.

The plan never states `AtomicU64`. An implementer without knowledge of the
`IngestStats` precedent will produce a compile error or a contended design.

**Remediation**: Add to D4 `CullStats` specification:
"`CullStats` uses `AtomicU64` for all per-photo counters (parallel to
`IngestStats` at `ingest.rs:87`). Shared via `Arc<CullStats>` across rayon
workers. `Ordering::Relaxed` is correct — counters are only read after the rayon
fork-join barrier completes."

---

## HIGH

### T4 — DN-022 / DN-023 cross-references swapped in plan body and TECH-DEBT.md (3-way)

**Agents**: gp (HIGH), com (HIGH), arch (implied)

`docs/discovery-notes.md` authoritative mapping:
- **DN-022** (line 183) = "LibRaw demosaic algorithm selection for NIMA preprocessing"
- **DN-023** (line 191) = "`cull_scores.photo_id` ON DELETE CASCADE absent"

But `docs/plans/session-03.md:208` says:
```
File DN-023 (demosaic algorithm choice: v0.1 uses LibRaw's default AHD...)
```
DN-023 is about ON DELETE CASCADE, not demosaic. The parenthetical describes
DN-022's content. The same swap propagates to `session-03.md:675` (Discovery
items: "See DN-023" for demosaic) and `TECH-DEBT.md:222` (TD-012 demosaic
stop-gap incorrectly cross-references DN-023).

Note: `session-03.md:337` correctly says "ON DELETE CASCADE... see DN-023" —
that site is correct and must be preserved.

**Remediation**:
1. `session-03.md:208`: `"File DN-023 (demosaic..."` → `"File DN-022 (demosaic..."`.
2. `session-03.md:675` (Discovery items): `"See DN-023"` → `"See DN-022"`.
3. `TECH-DEBT.md:222`: `"Cross-reference DN-022 + DN-023"` → `"Cross-reference DN-022"`.

---

### T5 — Per-worker `Nima` construction architecturally incompatible with `LoadedModel` wrapping `ort::Session` (3-way)

**Agents**: arch (HIGH), type (HIGH), gp (MEDIUM), simp (HIGH via VerifiedModelBytes analysis)

`docs/plans/session-03.md:192`: "`LoadedModel` newtype wraps `ort::Session`
(private) with model metadata." `session-03.md:425`: "Each worker constructs
its own `Nima` from the shared `LoadedModel`'s model bytes." These contradict:

`LoadedModel::from_verified(bytes: VerifiedModelBytes)` consumes the bytes to
construct one `ort::Session`. After construction the raw bytes are gone.
Workers cannot re-construct sessions from a `LoadedModel` that holds only a
`Session` (not the original bytes). The plan offers no mechanism for workers to
independently call `ort::Session::builder().commit_from_memory(bytes)`.

The `simp` agent further argues that `VerifiedModelBytes` is redundant with the
`build.rs` compile-time SHA-256 check (which already protects bundled models)
and that `--model-path` (the only case needing runtime verification) is deferred
to TD-015 — making the runtime re-check pure overhead.

**Remediation** (choose ONE and declare in plan v3):

**Option A** (minimal change): `VerifiedModelBytes` wraps `Arc<[u8]>` internally.
`LoadedModel::from_verified` takes `&VerifiedModelBytes` (borrow, not move).
Each rayon worker independently calls `LoadedModel::from_verified(&verified_bytes)`.
`VerifiedModelBytes::clone()` is cheap (Arc reference count). Specify this in
D1b.

**Option B** (simplifier-recommended, ~50 LoC saved): Drop `VerifiedModelBytes`.
Replace with a simpler `ModelRegistry::load(name) -> Result<LoadedModel>` that
reads + SHA-256-verifies the bundled model and constructs one `ort::Session`.
For per-worker use, expose `ModelRegistry::create_session(name) ->
Result<ort::Session>` (re-reads from disk, re-verifies). `build.rs` is the
compile-time trust boundary; runtime re-verification deferred to TD-015 when
`--model-path` lands.

Either option eliminates the contradiction; One must be declared in plan v3.

---

### T6 — `content_changed` detection listed in dispatch table but the re-derivation step is absent from the per-photo pipeline (2-way)

**Agents**: sfh (HIGH), arch (implied)

`docs/plans/session-03.md:439` dispatch table:
```
re-derived PhotoId mismatch (content changed) | content_changed | warn, skip
```
The D4 per-photo pipeline (lines 416-441): SELECT `id, source_path` → per worker
→ `read_raw_rgb` → `nima.score` → `insert_cull_score`. Nowhere is there a step
that re-derives `PhotoId` from `source_path` and compares against the catalog's
`id`. Without this step, `content_changed` is dead code. A file replaced between
ingest and cull will be silently scored under the original `photo_id` — silent
data corruption.

**Remediation**: Add an explicit step BEFORE `read_raw_rgb` in D4's per-photo
pipeline:
```rust
let current_id = PhotoId::derive(&source_path)?;
if current_id != catalog_photo_id {
    stats.content_changed.fetch_add(1, Relaxed);
    tracing::warn!(...);
    continue; // skip this photo
}
```
`PhotoId::derive` reads ~128 KB (head + tail windows) per file — negligible vs.
full RAW decode + NIMA inference. Specify in D4 that this step occurs before
decode and what error variant is returned if `derive` fails (e.g., file
unreadable at derivation time).

---

### T7 — FK violation from `insert_cull_score` has no dispatch table row; error propagation unspecified (2-way)

**Agents**: sfh (HIGH), type (implied via FK enforcement)

`docs/plans/session-03.md:352-354` specifies an FK regression test (assert FK
violation returned for non-existent `photo_id`). But the D4 error dispatch table
(lines 429-441) has no row for FK constraint violations. The D4 SELECT filters
to current rows, but a TOCTOU window exists (concurrent deletion between SELECT
and INSERT). With `PRAGMA foreign_keys = ON`, this returns
`rusqlite::Error::SqliteFailure(ErrorCode::ConstraintViolation, ...)` from
`insert_cull_score`. No dispatch handler matches this error class.

Also unspecified: what error variant does `insert_cull_score` return for an FK
violation? Without this, callers cannot distinguish "FK violation (catalog
inconsistency, skip)" from "I/O error (abort)".

**Remediation**:
1. Add dispatch row to D4 table:
   ```
   FK violation (photo deleted between SELECT and INSERT) | catalog_inconsistency
   counter | WARN, skip (not a strict failure — one deleted row should not abort
   a 370-photo run)
   ```
2. Specify that `insert_cull_score` maps `ErrorCode::ConstraintViolation` to
   `Error::CatalogInsert { photo_id, source: ... }` (or a new
   `CatalogForeignKeyViolation` variant). The D4 dispatch handler matches on
   this variant for the skip path.

---

### T8 — Existing test `open_schema_version_too_new_returns_error` hardcodes `user_version = 2`; breaks silently when SCHEMA_VERSION becomes 2 (2-way)

**Agents**: gp (HIGH), test (implied)

`crates/photohelper-catalog/src/catalog.rs:499-515`:
```rust
fn open_schema_version_too_new_returns_error() {
    conn.execute_batch("PRAGMA user_version = 2").unwrap();
    assert!(matches!(err, Error::CatalogSchemaTooNew { found: 2, expected: 1 }));
}
```
Session 03 bumps `SCHEMA_VERSION` to 2. After the bump, `user_version = 2`
matches the `v if v == SCHEMA_VERSION` arm (no-op) — `Catalog::open` returns
`Ok`. The test's `unwrap_err()` panics — test fails. The plan's D2a does not
mention updating this existing test, so the failure is silent until the first
`just ci` run after D2a lands.

**Remediation**: Add to D2a test specification: "Update existing test
`open_schema_version_too_new_returns_error` at `catalog.rs:499` to use
`PRAGMA user_version = 3` and assert
`Error::CatalogSchemaTooNew { found: 3, expected: 2 }`." (~2 lines.)

---

### T9 — Decision-doc 0001 §Migration policy committed session 03 to `Migration` trait + `dup_groups`; plan v2 drops both without proposing an amendment (2-way)

**Agents**: gp (HIGH), com (HIGH)

`docs/decisions/0001-catalog-schema-v1.md:133-136`:
```
The framework lives in `photohelper-catalog::migrations` as a
`Vec<&'static dyn Migration>` and a per-version applier; **session 03**
adds it + adds migration `v1 → v2` alongside the cull-score + dup-group
tables (DN-005).
```
Plan v2 drops both: the Migration trait (lines 264-266) and `dup_groups`
(lines 347-349). D2c creates decision-doc 0002 but does not propose amending
decision-doc 0001's §Migration policy. A future reader of decision-doc 0001
expects a `Migration` trait and `dup_groups` in v2 — corrupting the audit trail.

**Remediation**: Add to D2c or D7: "Amend
`docs/decisions/0001-catalog-schema-v1.md` §Migration policy lines 133-136:
replace 'a `Vec<&'static dyn Migration>`' with 'a `match`-arm extension in
`Catalog::open` (per decision-doc 0002; `Migration` trait deferred until v3
migration is non-trivial)'; replace 'cull-score + dup-group tables' with
'`cull_scores` table (`dup_groups` deferred per DN-024)'." (~3-line amendment.)

---

### T10 — D0 ABORT conditions incomplete: license rejection and SHA-256 failure lack explicit ABORT procedure (2-way)

**Agents**: rev (HIGH), sfh (implied)

D0 ABORT triggers: line 108 (CVE), line 121 (threading option-b failure), line
125 (fixture inference failure). But line 110-111: "license must be permissive
... **reject** CC-BY-NC or research-only." The word "reject" does not specify
ABORT — what happens next? Does the session halt? Continue searching? The
Verification surface commit-message template (lines 135-138) requires
`cve-posture: clean`, `inference: 2/2`, `threading: per-worker-session` but has
no `license: <SPDX-id> (verified)` line. SHA-256 verification failure (corrupted
download) also has no explicit ABORT.

**Remediation**:
1. Add two explicit ABORT triggers to D0:
   - "ABORT if model license is not in {MIT, Apache-2.0, CC-BY-4.0}."
   - "ABORT if model file SHA-256 cannot be verified (corrupted download or Git
     LFS corruption)."
2. Add `license: <SPDX-id> (verified)` to the Verification surface commit
   message template.
3. Specify the ABORT procedure: if any ABORT fires, session 03 narrows to
   D5 (TD-010 closure) + D6 (stub messages) + D7 (docs) only; no ort dep wired,
   no model committed.

---

### T11 — v1 catalog fixture lacks creation method; binary blob committed without reproducibility recipe (3-way)

**Agents**: rev (HIGH), test (MEDIUM-escalated), arch (implied)

`docs/plans/session-03.md:357`: "Commit a v1-catalog fixture at
`tests/fixtures/catalogs/v1.db`." No creation method is specified. Without a
reproducible creation method, the fixture cannot be regenerated when the v1
schema definition changes. Also unspecified: whether `.db` files should be Git
LFS-tracked.

**Remediation**:
1. Add a `just create-v1-fixture` recipe or `scripts/create-v1-catalog-fixture.sh`
   that creates the fixture deterministically via inline SQL (opening a tempdb,
   executing `INIT_SQL`, inserting one row, setting `PRAGMA user_version = 1`).
   Commit the script alongside the fixture.
2. State explicitly: "v1.db is < 20 KB; committed directly to Git (no LFS
   needed at this size). If fixtures exceed 1 MB, revisit."
3. Add fixture lifecycle note: "v1.db persists across v3+ to test chained
   migration; regenerate via `just create-v1-fixture` if v1 schema DDL changes."

---

### T12 — `read_raw_rgb` has no test entries in the test table; new public FFI API has zero explicit coverage (2-way)

**Agents**: test (HIGH), arch (MEDIUM via SAFETY audit gap)

`docs/plans/session-03.md:203-206` adds `read_raw_rgb(path: &Path) ->
Result<RgbImage>` as a new public entry point in `photohelper-raw`. The D1c
test table row (line 617) covers only `NimaScore` unit tests and NIMA inference
integration — no coverage for `read_raw_rgb` itself. The existing pattern at
`tests/integration_cr3.rs:58` tests `read_raw` with dimension, CFA pattern,
sensor-levels, and white-balance assertions. `read_raw_rgb` adds two new FFI
bindings (`libraw_dcraw_process` + `libraw_dcraw_make_mem_image`) and a new
`RgbImage` type — safety-critical FFI code. `assert_impl_all!(RgbImage: Send,
Sync)` is also missing (required: `RgbImage` is sent across rayon workers).

**Remediation**: Add a dedicated sub-row in the D1c test table:
```
read_raw_rgb | assert_impl_all!(RgbImage: Send, Sync); RgbImage::new validates
pixels_rgb.len() == width*height*3 | read_raw_rgb on both CC0 CR3 fixtures
returns correct width, height, channel count, pixels_rgb.len(); read_raw_rgb
on invalid file returns Err | n/a
```
Also add to D1c spec: "`RgbImage::new` validates `pixels_rgb.len() == width *
height * 3`, returning `Err(Error::RawImageDimensionMismatch)` on mismatch
(analogous to `BayerPlane::new` at `decode.rs:158-178`)."

---

### T13 — D4 per-case test fixtures underspecified for 3 of 6 cases; implementor cannot determine injection strategy (2-way)

**Agents**: test (HIGH), rev (implied)

`docs/plans/session-03.md:445-447`: "Integration test per case (6 total:
model-missing, SHA-mismatch, per-photo decode fail, per-photo inference fail,
file-missing, existing score)." Three of six cases have no fixture construction
specification:

- **model-missing**: How? Point `with_test_model_dir` at empty tempdir?
- **SHA-mismatch**: How? Wrong-SHA manifest.toml in test model dir? Corrupt the
  model bytes?
- **inference-fail**: How? A tiny ONNX with wrong input shape? A zero-byte ONNX
  (ort parse failure)? Unspecified.

**Remediation**: Add a fixture-construction sub-table to D4:

| Case | Fixture construction |
|------|---------------------|
| model-missing | `ModelRegistry::with_test_model_dir(empty_tempdir)` |
| SHA-mismatch | `with_test_model_dir(dir)` containing dummy `.onnx` + `manifest.toml` with wrong SHA-256 |
| per-photo decode fail | catalog row pointing at a `.txt` file (not a CR3) |
| inference-fail | `with_test_model_dir(dir)` containing a zero-byte `nima.onnx` (ort fails to parse → `ModelLoadFailed` or `InferenceFailed`) |
| file-missing | catalog row pointing at a non-existent path |
| existing score | pre-insert a `cull_scores` row before calling `run_cull` |

---

## MEDIUM

### T14 — init_schema + apply_v1_to_v2 two-transaction crash-recovery relies on undocumented PRAGMA transactionality (2-way)

**Agents**: arch (HIGH reclassified MEDIUM — the design is correct), sfh (MEDIUM)

When `user_version == 0`, the state machine runs two sequential transactions.
A crash between them leaves `user_version = 1`, handled by the `1 =>` arm on
re-open. This is correct, but the plan does not document the invariant: `PRAGMA
user_version = N` is transactional in SQLite (stored in the database header
page, protected by the WAL). Without this documentation, a future maintainer
might "optimize" `PRAGMA user_version = 2` to outside the transaction, breaking
atomicity.

**Remediation**: Add to D2a: "The two-transaction approach for fresh DBs
(init_schema then apply_v1_to_v2) is crash-safe: `PRAGMA user_version` is
transactional in SQLite (header-page write protected by the WAL). A crash
between the two commits leaves `user_version = 1`, correctly handled by the
`1 =>` arm on re-open."

---

### T15 — `VerifiedModelBytes` model_dir source at runtime unspecified; build.rs + runtime SHA-256 trust boundary unexplained (2-way)

**Agents**: arch (MEDIUM), simp (HIGH via T5)

`docs/plans/session-03.md:185`: `from_manifest(model_dir: &Path, name: &str)`
reads the model file and `manifest.toml` at runtime. But `model_dir` at runtime
is never specified: is it `OUT_DIR`? The binary's sibling directory? A
user-config path? This is partially resolved by T5's remediation (which requires
choosing Option A or B for the per-worker Session design). If Option B is chosen
(drop VerifiedModelBytes), T15 is moot. If Option A is kept, `model_dir` must
be specified.

**Remediation** (if Option A chosen for T5): Add to D1b: "At runtime,
`model_dir` resolves to `[binary-sibling-directory]/models/` for installed
builds, or `OUT_DIR/models/` for `cargo-test` runs (configurable via
`PHOTOHELPER_MODEL_DIR` env-var for tests). `build.rs` protects the source
repository; runtime `VerifiedModelBytes` protects the file as loaded by the
running binary."

---

### T16 — Migration recovery test "half-applied" state construction is ambiguous (2-way)

**Agents**: test (MEDIUM), arch (implied)

`docs/plans/session-03.md:303-306`: "open a v1-catalog fixture with a partial
`cull_scores` table." But `apply_v1_to_v2` uses `execute_batch` inside a
transaction — SQLite atomicity prevents true partial execution. The "half-applied"
state (cull_scores exists, user_version = 1) can only be constructed
programmatically — a binary fixture cannot represent it durably.

**Remediation**: Reword: "Construct the half-applied state programmatically in
the test: open v1.db, directly `CREATE TABLE IF NOT EXISTS cull_scores (...)`
via `conn.execute_batch()` without bumping `user_version`, then re-open via
`Catalog::open`. Assert `user_version = 2` and no error — idempotent `IF NOT
EXISTS` handles the already-existing table."

---

### T17 — `nima_postproc.rs` labeled "Optional" but the 10-bin reduction is mandatory for every inference call (2-way)

**Agents**: rev (MEDIUM), simp (LOW)

`docs/plans/session-03.md:225-226`: "Optional second module `nima_postproc.rs`
for the score-distribution → single-scalar reduction." NIMA always outputs a
10-bin probability distribution; the weighted-mean reduction is required for
every `Nima::score` call. "Optional" misleads — the postprocessing is mandatory;
only the module boundary is optional.

**Remediation**: Reword: "The 10-bin distribution → scalar weighted-mean
reduction is implemented as a private function in `nima.rs` (or optionally
extracted into `nima_postproc.rs` for readability — implementer's choice). The
reduction itself is required for every inference call."

---

### T18 — Feature gate `default = ["ai-culling"]` deferred to "impl time" despite plan recommending its removal (2-way)

**Agents**: simp (MEDIUM), arch (implied)

`docs/plans/session-03.md:151-155` acknowledges no current consumer for the
`ai-culling` feature gate, recommends dropping it ("simpler"), then says
"Decision at impl time." Per CLAUDE.md "Don't design for hypothetical future
requirements" — if the plan can't justify the gate now, resolve it now.

**Remediation**: Drop the feature gate. Make `ort` a hard dependency. Add to
§Out of scope: "`ai-culling` feature gate dropped from v0.1; add when a
downstream consumer needs `default-features = false`."

---

### T19 — `--threshold-warn <N>` is scope creep; not in v1, not a remediation for any R1 finding (2-way)

**Agents**: simp (MEDIUM), rev (implied)

`docs/plans/session-03.md:460` adds `--threshold-warn <N>` "for informational
output." This flag was not in plan v1, is not on the R2 watch-list, has no
user story, and adds conditional output logic + a `CullOpts` field + at least
one test. Per CLAUDE.md "Don't add features beyond what the task requires."

**Remediation**: Remove `--threshold-warn <N>`. Add to §Out of scope:
"Threshold-based cull output deferred to session 05+ when cull-decision UI
workflow is defined."

---

### T20 — `NimaScore::from_catalog_f64` saturates silently; rounding-error clamp indistinguishable from data corruption (2-way)

**Agents**: type (MEDIUM), sfh (implied)

`docs/plans/session-03.md:217-219`: `from_catalog_f64` "saturates to [1.0,
10.0] for rounding-error tolerance." The analogy to `clamp_mtime` is sound, but
`clamp_mtime` returns `(value, was_clamped: bool)` and the caller logs a WARN.
`from_catalog_f64` returns `Result<Self>` with no `was_clamped` signal. If
`score = 15.0` (corrupted catalog), silent saturation to 10.0 masks the
corruption.

**Remediation**: Emit `tracing::warn!` inside `from_catalog_f64` when
`|value - clamped| > 1e-6` (rounding-error epsilon). Values within epsilon are
silently clamped; values outside emit a WARN so operators can investigate catalog
corruption.

---

### T21 — Per-worker Session OOM produces undiagnosable `inference_failed × N` with no memory-pressure signal (2-way)

**Agents**: sfh (MEDIUM), arch (implied)

On M2 Ultra (24 cores), 24 workers × 50 MB sessions + 24 × 60 MB RAW buffers ≈
2.64 GB peak. If all workers fail at session construction (OOM), the summary
shows `inference_failed: 370` with no indication of memory pressure. The user
cannot distinguish corrupt model from OOM.

**Remediation**: Add to D4: "Log a session-level WARN if `inference_failed`
equals `num_workers` at run completion: 'All N inference workers failed at
session init; if inference_failed matches worker count, check available memory
(N workers × ~50 MB each).' Optionally cap workers to `min(num_cpus, 8)` via
rayon `ThreadPoolBuilder` to bound peak memory to ~400 MB."

---

### T22 — `RgbImage` construction invariant and `Send + Sync` assertion absent from D1c specification (2-way)

**Agents**: type (MEDIUM), test (via T12 remediation)

`docs/plans/session-03.md:206-208` describes `RgbImage` with `pixels_rgb:
Vec<u8>` but neither specifies the `len() == width * height * 3` invariant
(analogous to `BayerPlane::new`'s `data.len() == width * height` check at
`decode.rs:158-178`) nor mandates `assert_impl_all!(RgbImage: Send, Sync)`
(required: `RgbImage` crosses rayon worker threads).

**Remediation**: Consolidated into T12 remediation (add to D1c spec:
`RgbImage::new` validates dimension invariant + `assert_impl_all!(RgbImage:
Send, Sync)` at module scope).

---

## LOW

### T23 — `NimaScore` lacks `Ord` despite being totally ordered; downstream `sort()` needs `unwrap()` on `partial_cmp` (type)

`NimaScore` rejects NaN at construction, so `partial_cmp` never returns `None`
— total order is guaranteed. Only `PartialOrd` is derived. `Vec<NimaScore>::
sort_by(|a, b| a.partial_cmp(b).unwrap())` triggers `unwrap_used = "warn"`.

**Remediation**: Implement `Ord` on `NimaScore`:
`fn cmp(&self, other: &Self) -> Ordering { self.0.partial_cmp(&other.0).expect("NimaScore is NaN-free") }`.

---

### T24 — `nima-regenerate-golden` recipe has no explicitly owning deliverable (rev, test)

The recipe appears in D1c and D3 but is not listed as a sub-deliverable. If
omitted from the PR, no acceptance criterion catches the gap.

**Remediation**: Add to D1c: "Add `just nima-regenerate-golden` to `justfile`:
runs NIMA inference on `tests/fixtures/cr3/CRAW_FULL_FRAME.CR3`, writes
output distribution to `crates/photohelper-ai/tests/fixtures/nima/
golden_cr3_fixture1.bin`. First run creates the file; subsequent runs overwrite."

---

### T25 — `CullOpts` struct premature for ≤2 fields; inconsistent with `run_ingest(&Cli, &IngestArgs)` pattern (simp)

With `--threshold-warn` removed (T19), `CullOpts` carries at most `strict: bool`.
The existing pattern is `run_ingest(&Cli, &IngestArgs)` — no intermediate opts
struct.

**Remediation**: Replace `CullOpts` with direct `&CullArgs` parameter:
`run_cull(cli: &Cli, args: &CullArgs, scorer: &Nima) -> anyhow::Result<u8>`.

---

### T26 — `v1.db` LFS status unspecified (overlap with T11; addressed in T11 remediation)

T11 remediation already includes: "v1.db is < 20 KB; committed directly to Git
(no LFS needed)." No separate action required.

---

## Disposition summary

| Disposition | Count | Action |
|-------------|------:|--------|
| **Fix in plan v3 (CRITICAL)** | 3 | T1, T2, T3 — all must close before implementation |
| **Fix in plan v3 (HIGH)** | 10 | T4–T13 |
| **Fix in plan v3 or defer with TD** | 9 MEDIUM | T14–T22 |
| **Accept / minor fix** | 4 LOW | T23–T26 |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 13
  verified: 10
  drifted: 3
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    All 13 CRITICAL+HIGH findings verified by 9th-agent with orchestrator
    post-hoc substring-grep. Three findings are drifted (line numbers off by
    1-4 lines due to plan layout); all retain=yes with corrected lines. Zero
    hallucinations. MEDIUM/LOW findings carry direct reasoning from agent reports
    and can be spot-checked at remediation time. T22 consolidated into T12.
  details:
    - {finding_id: T1,  file: docs/plans/session-03.md,                  line: 389, present: yes,     retain: yes,                    evidence_snippet: "python3 -c \"import onnx; m = onnx.load('$ONNX_FILE');"}
    - {finding_id: T2,  file: docs/plans/session-03.md,                  line: 489, present: yes,     retain: yes,                    evidence_snippet: "pub fn force_heartbeat_panic_in_thread(handle: &JoinHandle<()>)"}
    - {finding_id: T3,  file: docs/plans/session-03.md,                  line: 408, present: yes,     retain: yes,                    evidence_snippet: "pub fn run_cull(catalog: &Catalog, scorer: &Nima, opts: &CullOpts)"}
    - {finding_id: T4,  file: docs/plans/session-03.md,                  line: 208, present: drifted, retain: yes,                    evidence_snippet: "File DN-023 (demosaic algorithm choice: v0.1"}
    - {finding_id: T4b, file: docs/discovery-notes.md,                   line: 183, present: yes,     retain: yes,                    evidence_snippet: "### DN-022 — LibRaw demosaic algorithm selection for NIMA preprocessing"}
    - {finding_id: T4c, file: docs/discovery-notes.md,                   line: 191, present: yes,     retain: yes,                    evidence_snippet: "### DN-023 — `cull_scores.photo_id` ON DELETE CASCADE absent from v2 schema"}
    - {finding_id: T5,  file: docs/plans/session-03.md,                  line: 425, present: drifted, retain: yes-with-corrected-line, evidence_snippet: "the shared `LoadedModel`'s model bytes. No `Mutex` wrapping; no async."}
    - {finding_id: T6,  file: docs/plans/session-03.md,                  line: 439, present: yes,     retain: yes,                    evidence_snippet: "re-derived PhotoId mismatch (content changed) | `content_changed` | warn, s"}
    - {finding_id: T7,  file: docs/plans/session-03.md,                  line: 353, present: yes,     retain: yes,                    evidence_snippet: "non-existent `photo_id`; assert the FK violation error is returned (not"}
    - {finding_id: T8,  file: crates/photohelper-catalog/src/catalog.rs, line: 499, present: yes,     retain: yes,                    evidence_snippet: "fn open_schema_version_too_new_returns_error() {"}
    - {finding_id: T9,  file: docs/decisions/0001-catalog-schema-v1.md,  line: 134, present: yes,     retain: yes,                    evidence_snippet: "`Vec<&'static dyn Migration>` and a per-version applier; **session 03**"}
    - {finding_id: T10, file: docs/plans/session-03.md,                  line: 108, present: drifted, retain: yes-with-corrected-line, evidence_snippet: "ABORT if any open CVE."}
    - {finding_id: T11, file: docs/plans/session-03.md,                  line: 357, present: yes,     retain: yes,                    evidence_snippet: "Commit a v1-catalog fixture at `tests/fixtures/catalogs/v1.db`."}
    - {finding_id: T12, file: docs/plans/session-03.md,                  line: 617, present: yes,     retain: yes,                    evidence_snippet: "D1c NIMA scorer | `NimaScore::new` rejects NaN/∞/out-of-range"}
    - {finding_id: T13, file: docs/plans/session-03.md,                  line: 445, present: yes,     retain: yes,                    evidence_snippet: "model-missing, SHA-mismatch,"}
```

---

## R3 watch-list (must verify in Round 3 after plan v3 remediation)

1. T1: D3 includes CI `pip install onnx` step + `sanitize-check.sh` gates on `onnx` availability.
2. T2: D5c drops `force_heartbeat_panic_in_thread(handle: &JoinHandle<()>)`; `HeartbeatDeathTrigger` (or equivalent) specified; D5e parameterization clarified.
3. T3: `CullStats` uses `AtomicU64`; `Arc<CullStats>` specified; `Ordering::Relaxed` documented.
4. T4: DN-022 and DN-023 references corrected in D1c and Discovery items; TD-012 TECH-DEBT.md cross-reference corrected.
5. T5: D1b declares ONE of Option A (`VerifiedModelBytes` wraps `Arc<[u8]>`) or Option B (simplified `ModelRegistry::load`); per-worker Session construction path is unambiguous.
6. T6: D4 per-photo pipeline includes explicit `PhotoId::derive` + compare step BEFORE `read_raw_rgb`.
7. T7: D4 dispatch table includes FK violation row; `insert_cull_score` error propagation specified.
8. T8: D2a specifies update to `open_schema_version_too_new_returns_error` test (user_version = 3, expected = 2).
9. T9: D2c or D7 specifies amendment to `docs/decisions/0001-catalog-schema-v1.md` §Migration policy.
10. T10: D0 has explicit ABORT for license rejection and SHA-256 failure; `license:` line in Verification surface.
11. T13: D4 includes fixture-construction table for all 6 per-case tests.
