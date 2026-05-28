# Session 03 — `ai-culling-skeleton`

> **Branch**: `session-03/ai-culling-skeleton`
> **Started**: 2026-05-28
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v2 (R1 remediation)

> **Note on title slug**: branch is `ai-culling-skeleton`; the session
> lands the catalog v1→v2 migration + `cull_scores` table + the
> photohelper-ai end-to-end NIMA culling pipeline + TD-010 full closure
> in one PR. `dup_groups` was deferred to session 04+ (see §D2b + PR1-T30
> remediation).

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the end-to-end AI culling pipeline that turns `photohelper-ai`
from a 6-line stub into a working NIMA aesthetic-score scorer, plus
the catalog v1→v2 migration + new `cull_scores` table those scores land
in, plus closing `TECH-DEBT.md § TD-010` (the full Deliverable-6 test
infrastructure deferred from session 02), plus the first-chore
stub-message fix for `docs/discovery-notes.md § DN-020`. Four
complementary deliverables under the same `cull` subcommand surface:

1. **`photohelper-ai::nima::score(raw_rgb: &RgbImage) -> Result<NimaScore>`** —
   load a vendored NIMA ONNX model, downsample the LibRaw-decoded RGB
   image to the model's input shape (224×224×3), run a single forward
   pass, return the aesthetic score (1.0–10.0 float, lower = worse). This
   is the **DN-005 critical-path remediation**: catalog needs a meaningful
   `cull_score` column to drive `cull` subcommand output.

2. **Catalog v1→v2 migration** — a simple `match`-arm migration (per
   decision-doc 0001:129 "A single-statement migration doesn't justify
   framework overhead") that bumps `PRAGMA user_version` from 1 to 2 and
   adds the `cull_scores` table idempotently. Decision-doc 0001 §
   Amendments (2026-05-28) explicitly assigns this ownership to session 03.

3. **`cull` subcommand wired for real** — was stub `exit 69` from
   session 01; now walks catalog rows, decodes each photo's RAW via
   LibRaw's demosaic pipeline (`photohelper-raw::read_raw_rgb`), runs
   NIMA inference per worker thread, writes `cull_scores` rows. Heartbeat
   liveness + summary line analogous to `ingest`.

4. **TD-010 full closure** — Deliverable-6 test infrastructure ships
   in full: `poison_for_testing` knob on `Catalog`, R2-M8 silent-ROLLBACK
   fix, heartbeat-death test via `photohelper-test-helpers` dev-deps crate
   (closes TD-005), DN-008 6 rows `{6, 17, 39, 42, 43, 49}`, R2-T18 4
   WARN regression tests.

Once wired, integration test `cull_scores_real_canon_r8_cr3_fixture`
flips its assertion from `cull` exits 69 to "row inserted with
aesthetic_score ∈ [1.0, 10.0]" — closing DN-005's "session 03 reopens
for cull/dup-group additions" trigger.

### Scope rationale (why bundle 4 deliverables + TD-010 closure)

**Catalog v2 + AI culling are inseparable**. AI culling exists to write
to a column that doesn't exist yet. Splitting them — adding cull_scores
to the schema in one session without a writer, then implementing the
writer in the next — would leave the catalog in a "schema-v2-but-no-data"
limbo and force two migration-test rounds.

**TD-010 binding trigger fires NOW**. TD-010's binding trigger reads
*"opens the next session that touches `photohelper-catalog::Catalog`
for any reason."* Session 03 touches `Catalog` substantially (new
migration, new schema-version gate, new tables, new insert/select paths
for `cull_scores`). Per `CLAUDE.md § No Acceptable Trade-offs Policy`,
deferring TD-010 a second time without closing the fired trigger is a
policy violation. Full closure here closes TD-010 AND lands the test
infrastructure that the new code will itself need (specifically:
`poison_for_testing` for a `cull`-on-poisoned-catalog regression test).

**DN-020 stub-message fix is the natural first commit**. DN-020's
binding trigger reads *"session 03 session-start sweep."* Lands first
to clear the trivial drift before the substantive work begins.

The session is large but cohesive: every deliverable touches the same
two crates (`photohelper-ai` + `photohelper-catalog`) and the same
subcommand (`cull`). The plan-review and session-end review surface
the actual line counts; preliminary estimate is ~1500–2000 LoC across
~20–25 commits.

### Deliverables (when the PR merges, the following will exist)

#### Deliverable 0 — Pre-flight feasibility probe

**Sequencing (critical — PR1-T9 remediation)**: D0 fires FIRST, before
any ort dep wiring. Order: D0 → D1a (dep-only wiring) → D1b/c → D1d
(model binary). If D0 ABORTs, no model binary is ever committed; no
ort dep is wired (abort before D1a).

Verify (a) `ort` 2.x RC is CVE-clean against RustSec + the OSV.dev
ONNX-ecosystem feed, (b) a NIMA ONNX model exists with a clear license
+ provenance + reproducible SHA-256, (c) end-to-end inference works on
the existing CC0 Canon R8 CR3 fixtures, and **(d) ort threading
semantics are verified empirically (PR1-T5 remediation)**.

- **Artifact**: `docs/analysis/ANL-002-ort-nima-preflight.md` with:
  - **§ ort version**: chosen RC pin (e.g. `=2.0.0-rc.12` or latest at
    D0 time; verified from crates.io + ort GitHub). Record: (a) this is a
    release candidate; (b) upgrade trigger to stable 2.0.0 → cross-ref
    TD-014; (c) `download-binaries` vs static-linking decision.
  - **§ CVE-posture-as-of-pin**: RustSec advisory feed + OSV.dev ONNX
    Runtime grep + ort GitHub Security Advisories for any open CVE
    affecting the chosen version. ABORT if any open CVE.
  - **§ NIMA model provenance**: source URL + author + license (must be
    permissive: MIT / Apache-2.0 / CC-BY-4.0; reject CC-BY-NC or
    research-only); SHA-256 verified; ONNX opset version; input shape;
    output range.
  - **§ Threading semantics (PR1-T5 remediation)**: spawn two rayon
    workers both calling `session.run()` on the same `Arc<Session>`;
    record whether it compiles and is deterministic. Verify whether
    `Session::run` takes `&mut self` in the chosen ort version. Based on
    this, pick one of: (a) `Arc<Mutex<Session>>`, (b) one session per
    rayon worker thread, (c) `Session::run_async` + ort's intra-op
    threadpool. **Session 03 picks option (b): one `Session` per rayon
    worker thread** (no async complexity, correct per `Session::run`
    `&mut self` receiver). ABORT if option (b) fails for a structural
    reason (e.g. ort `Environment` is not `Send`).
  - **§ Inference end-to-end**: run NIMA against both CC0 R8 CR3
    fixtures; record per-fixture aesthetic_score; verify deterministic
    across re-runs. ABORT if fixture inference fails.
  - **§ Per-photo wall-clock**: measure actual decode + infer + downsample
    time on the two CC0 fixtures (acceptance criterion 3 is based on this
    measurement, not a fixed "30 min" bound).
- **Pre-flight output**: commit `crates/photohelper-ai/models/manifest.toml`
  skeleton (SHA-256 + source URL + license + opset + input-shape, all
  string-keyed by model name). Model binary (`models/<name>.onnx`)
  committed in D1d AFTER this artifact validates the chosen model.
- **Commit shape**: dedicated `chore(ai): pre-flight ort + NIMA audit
  (Deliverable 0)` commit. **No D1d or D4 commit may land before this.**
- **Verification surface**: commit message MUST include
  `cve-posture: clean (versus RustSec + OSV.dev YYYY-MM-DD)` AND
  `inference: 2/2 fixtures, scores [a, b]` AND
  `threading: per-worker-session (option-b)`.

#### Deliverable 1 — `photohelper-ai` real implementation

##### 1a — Crate scaffolding + ort dep wiring (dep-only; no model binary)

Lands immediately AFTER D0 validates ort's CVE-posture and before D1d
commits the model binary.

- `crates/photohelper-ai/Cargo.toml`:
  - `[dependencies] ort = { workspace = true }` (workspace dep added).
  - Re-add `photohelper-core` workspace dep; add `photohelper-raw` for
    `RgbImage` input.
  - Feature flag `default = ["ai-culling"]` (`ai-denoise` / `ai-sharpen`
    stay deferred). Gate is for downstream crates wanting to skip ort
    linkage (`default-features = false`); no v0.1 downstream consumer
    exists yet. If the flag provides no current value, drop it and make
    ort a hard dep — simpler. Decision at impl time.
- `crates/photohelper-ai/src/lib.rs`: replace stub with module
  declarations + crate-level rustdoc.
- New `crates/photohelper-ai/src/error.rs`: domain `Error` enum
  (`#[non_exhaustive]`, `thiserror::Error`-derived) with at minimum:
  `ModelLoadFailed { path, source }`, `InferenceFailed { source }`,
  `InvalidInputShape { expected, got }`,
  `ModelVerificationFailed { path, expected_sha256 }` (the verification
  error for `VerifiedModelBytes`; see §1b).
- Workspace `Cargo.toml` `[workspace.dependencies]` adds
  `ort = { version = "=<pin-from-D0>", default-features = false,
  features = [...] }` — exact version and features locked at D0 pin
  time. The `=<pin-from-D0>` is a placeholder for the ANL-002 pin.
  **Note**: this is a release candidate pin; TD-014 tracks upgrade path.

##### 1b — Model registry + loader

**(PR1-T27 remediation: no `ModelRegistry` trait; no `--model-path`.)**

- `crates/photohelper-ai/src/registry.rs`: concrete `pub struct
  ModelRegistry` with:
  - `fn new() -> Self` (standard constructor).
  - `#[doc(hidden)] fn with_test_model_dir(path: PathBuf) -> Self` —
    test-override constructor (same pattern as `Catalog::open_with_retry_delay`).
  - `fn load(&self, name: &str) -> Result<LoadedModel>` — resolves name
    to the bundled model file, calls `VerifiedModelBytes::from_manifest`,
    then `LoadedModel::from_verified`.
- **`VerifiedModelBytes` type-state (PR1-T10 remediation)**: two-phase
  constructor:
  1. `VerifiedModelBytes::from_manifest(model_dir: &Path, name: &str) ->
     Result<Self>` — reads `manifest.toml`, reads model file, checks
     SHA-256, returns typed-verified bytes. This is the attestation step.
  2. `LoadedModel::from_verified(bytes: VerifiedModelBytes) ->
     Result<Self>` — creates `ort::Session` from the verified bytes,
     stores model metadata (input shape, output shape, opset). No
     re-SHA-256-check at this point — the type system prevents unverified
     bytes from reaching this constructor.
- `LoadedModel` newtype wraps `ort::Session` (private) with model
  metadata. Private fields; accessors for metadata only.
- **`--model-path` is NOT in v0.1 scope.** Deferred per PR1-T27
  (resolving the SHA trust boundary of user-supplied models is non-trivial;
  see TD-015).

##### 1c — NIMA scorer + new photohelper-raw entry point

**(PR1-T20 remediation: use LibRaw's dcraw_process pipeline, not bilinear
Rust. PR1-T6 remediation: no `Scorer` trait; D4 takes concrete `&Nima`.)**

- **New `photohelper-raw` entry point**: `read_raw_rgb(path: &Path) ->
  Result<RgbImage>`. Adds two FFI bindings to `crates/photohelper-raw/src/ffi.rs`:
  `libraw_dcraw_process` + `libraw_dcraw_make_mem_image`. `RgbImage` type
  (analogous to existing `RawImage`) in `crates/photohelper-raw/src/decode.rs`
  exposes `width`, `height`, `pixels_rgb: Vec<u8>` (8-bit sRGB, 3 channels)
  for NIMA preprocessing. File DN-023 (demosaic algorithm choice: v0.1
  uses LibRaw's default AHD; session 04+ develop pipeline may select
  AMaZE, AAHD, or VNG4). File TD-012 (stop-gap: AHD demosaic algorithm).
- `crates/photohelper-ai/src/nima.rs`: `Nima` struct holds a
  `LoadedModel`; `Nima::score(rgb: &RgbImage) -> Result<NimaScore>` is
  the public entry. `NimaScore` newtype wraps `f32` constrained to
  `[1.0, 10.0]` (fallible constructor; reject NaN, ±∞, out-of-range).
  - **NimaScore traits (PR1-T31 remediation)**: `Copy + Clone + Debug +
    PartialOrd` (NOT `Eq` — f32 equality is floating-point, not natural
    equality). `NimaScore::from_catalog_f64(f64) -> Result<Self>`:
    separate saturating constructor for read-back from SQLite `REAL` column
    (tolerates rounding-error push past boundary; analogous to `clamp_mtime`
    at `model.rs:206`).
- Preprocessing pipeline: `RgbImage` (from `read_raw_rgb`) → bilinear
  downsample to 224×224 → normalize per NIMA's ImageNet stats (mean
  `[0.485, 0.456, 0.406]`, std `[0.229, 0.224, 0.225]`; per ANL-002 §
  NIMA model provenance). Optional `nima_postproc.rs` for the score-
  distribution → single-scalar reduction (NIMA outputs a 10-bin
  distribution; weighted mean gives the scalar).
- **No `Scorer` trait in v0.1**. D4 takes a concrete `scorer: &Nima`
  parameter. Defer the trait to session 04 when ARNIQA lands as the
  second impl (per CLAUDE.md "don't design for hypothetical future
  requirements").
- **NIMA golden-vector test (PR1-T26 remediation)**:
  - E2E: `score` is within `±1e-3` of golden (CPU-deterministic tolerance;
    platform of record: apple-silicon). Golden vector committed as
    `crates/photohelper-ai/tests/fixtures/nima/golden_cr3_fixture1.bin`.
  - `just nima-regenerate-golden` recipe re-runs inference and overwrites
    the binary fixture (recovery path for failing test).
  - Linux x86_64 CI uses a band assertion `score ∈ [3.0, 9.0]` (per D0's
    actual recorded scores ± safety band).
  - File DN-025: NIMA cross-platform tolerance.

##### 1d — Bundled model file

**(PR1-T16 remediation: no pre-named filename; committed AFTER D0.)**

- `crates/photohelper-ai/models/<model_chosen_at_D0>.onnx` — vendored
  via Git LFS (`.gitattributes` extended to track `*.onnx`). Exact
  filename determined when D0's ANL-002 validates the model.
- `crates/photohelper-ai/models/manifest.toml` — per-model SHA-256 +
  license + provenance + source-URL + opset + input-shape (skeleton
  committed in D0 pre-flight; this commit populates the actual binary
  and completes the manifest entry).
- `crates/photohelper-ai/build.rs` — verifies the bundled model file's
  SHA-256 matches `manifest.toml` at build time.
- **This commit MUST follow the D0 pre-flight commit.**

#### Deliverable 2 — Catalog v1→v2 migration

##### 2a — Migration (match-arm approach, not trait)

**(PR1-T19 remediation: decision-doc 0001:129 — "A single-statement
migration doesn't justify framework overhead." PR1-T17 remediation: full
schema-version state machine. PR1-T22 remediation: PRAGMA foreign_keys
= ON. PR1-T15 remediation: ROLLBACK error propagation.)**

**No `Migration` trait, no `MIGRATIONS` registry**. Instead:

```rust
// in Catalog::open, replacing the existing schema-version gate:
match user_version {
    0          => { init_schema(conn)?; apply_v1_to_v2(conn)?; }
    1          => { apply_v1_to_v2(conn)?; }
    v if v == SCHEMA_VERSION => {} // up-to-date, no-op
    v          => return Err(Error::CatalogSchemaTooNew {
                      found: v, supported: SCHEMA_VERSION }),
}

fn apply_v1_to_v2(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction_with_behavior(Immediate)?;
    tx.execute_batch(/* CREATE TABLE cull_scores + indexes + PRAGMA user_version = 2; */)?;
    tx.commit()?;
    Ok(())
}
```

**Replay-safety invariant**: every `apply_v*_to_v*` function MUST use
`CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS` — no
destructive `DROP`. A second concurrent `Catalog::open` (both find
`user_version = 1`; file-lock serializes them) gets the same safe re-run.

**ROLLBACK error propagation**: if `apply_v1_to_v2`'s implicit rollback-on-
drop fires (transaction not committed), `rusqlite` calls `rollback()`. If
that rollback itself errors:
  - `Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ApiMisuse` →
    silent ignore (no active transaction; already rolled back by SQLite).
  - Any other error → propagate as `Error::CatalogMigrationFailed { op: "rollback", source }`.

**PRAGMA `foreign_keys = ON`** added to the Step 7 PRAGMA loop in
`Catalog::open` (verifying at `catalog.rs:204-212` — the existing
`journal_mode = WAL`, `synchronous = NORMAL`, `busy_timeout = 5000`
block gets `foreign_keys = ON` appended). This is required for the
`cull_scores.photo_id REFERENCES photos(id)` FK to be enforced.

**Recovery integration test**: open a v1-catalog fixture with a partial
`cull_scores` table (simulating a half-applied migration), call
`Catalog::open`; assert it succeeds (idempotent re-run) and
`user_version = 2`.

**Future**: when v3 migration arrives AND is non-trivial (multi-step or
data-migrating), promote to a `Migration` trait + version-registry.
Recorded in decision-doc 0002 as a design direction — not a stop-gap TD
because the match-arm approach is the CORRECT design for a single
two-table migration.

##### 2b — v2 schema (`cull_scores` only)

**(PR1-T30 remediation: drop `dup_groups` from v2. PR1-T12 remediation:
supersede semantics. PR1-T8 remediation: automated test, not manual REPL.
PR1-T22: FK enforced via PRAGMA.)**

New `cull_scores` table:
```sql
CREATE TABLE IF NOT EXISTS cull_scores (
    photo_id BLOB NOT NULL REFERENCES photos(id),
    scorer TEXT NOT NULL,           -- 'nima-aesthetic-v1' for now
    score REAL NOT NULL,
    scored_at_unix_seconds INTEGER NOT NULL,
    PRIMARY KEY (photo_id, scorer)
);
CREATE INDEX IF NOT EXISTS idx_cull_scores_score ON cull_scores(score);
```

- Composite PK so a photo can have one row per scorer (NIMA aesthetic
  now; ARNIQA-technical later).
- FK to `photos(id)` enforced by `PRAGMA foreign_keys = ON` (added in
  §2a).
- `ON DELETE CASCADE` deliberately NOT added (v0.1 has no delete path;
  see DN-023).
- **Supersede semantics**: `cull_scores` references current-only rows.
  When a photo is superseded (`photos.superseded_at_unix_seconds IS NOT
  NULL`), its `cull_scores` rows remain (audit trail) but `run_cull`'s
  SELECT excludes superseded photos (PR1-T12 fix). The `cull_scores` PK
  `(photo_id, scorer)` is unique per scorer; a superseded+re-ingested
  photo gets a new `photo_id` and thus a new row. Recorded in decision-doc
  0002.
- `score REAL` (not INTEGER × 100) — preserve fractional precision.

**`dup_groups` table**: deferred to session 04+ when MobileCLIP arrives
and can validate the embedding shape, dimension, and model-identity
columns. Shipping a table with no writer in v2 wastes a migration slot.
File DN-024 (dup-detection compute deferred) + note in §Out of scope.

**FK regression test**: attempt `Catalog::insert_cull_score` with a
non-existent `photo_id`; assert the FK violation error is returned (not
silently ignored).

**Automated integration test (replaces manual REPL inspection)**:
- Commit a v1-catalog fixture at `tests/fixtures/catalogs/v1.db`.
- Integration test: open the v1 fixture with `Catalog::open`, query
  `PRAGMA user_version` (assert = 2), query
  `SELECT name FROM sqlite_master WHERE type='table'` (assert `cull_scores`
  present), assert an existing v1 `photos` row is preserved.

##### 2c — Decision-doc 0002

- `docs/decisions/0002-catalog-schema-v2.md` — extends decision-doc
  0001 with the v2 additions: `cull_scores` rationale, supersede semantics
  choice, `dup_groups` deferral rationale, why no `ON DELETE CASCADE`,
  why match-arm migration approach, what's out of scope for v3+ (per-cull-
  run audit trail, per-scorer config snapshot, `dup_groups` when
  MobileCLIP arrives).
- Cross-link to ANL-002 (the NIMA model is what the `cull_scores` rows
  reference via `scorer`).

#### Deliverable 3 — Fixture additions

- Reuse session 02's two CC0 Canon R8 CR3 fixtures at
  `tests/fixtures/cr3/*.cr3` — no new image fixtures needed.
- New `crates/photohelper-ai/tests/fixtures/nima/golden_cr3_fixture1.bin`
  — binary golden-vector fixture for inference-regression tests (the
  output distribution for one CR3); `just nima-regenerate-golden` recipe
  overwrites it.
- **ONNX sanitize-check (PR1-T28 remediation)**: ONNX files are
  Protobuf-encoded; `exiftool` does not know ONNX fields. What ONNX files
  DO carry: `producer_name`, `doc_string`, `metadata_props` key-value
  strings which may leak training-environment absolute paths or internal
  identifiers. Extend `scripts/sanitize-check.sh` with an ONNX-aware
  check:
  ```bash
  python3 -c "import onnx; m = onnx.load('$ONNX_FILE');
    print(m.producer_name, m.doc_string, [str(p) for p in m.metadata_props])"
  ```
  Allow-list: `producer_name` in known frameworks (PyTorch, ONNX exporter,
  TensorFlow ONNX); reject absolute paths in `doc_string` or
  `metadata_props`; reject email addresses or internal hostnames.
- Commit a pre-built v1 SQLite catalog fixture at
  `tests/fixtures/catalogs/v1.db` for D2b's automated migration test.

#### Deliverable 4 — `cull` subcommand rewire

**(PR1-T6: concrete `&Nima`, not `&dyn Scorer`. PR1-T12: supersede filter.
PR1-T13: explicit error dispatch. PR1-T14: --strict semantics. PR1-T18:
source_path in SELECT. PR1-T5: per-worker Session. PR1-T33: heartbeat
duplicated in cull.rs.)**

- `crates/photohelper-cli/src/commands/cull.rs` — new file (replaces
  inline stub in `main.rs`):
  ```rust
  pub fn run_cull(catalog: &Catalog, scorer: &Nima, opts: &CullOpts)
      -> Result<CullStats, Error>
  ```
  No `Scorer` trait; concrete `&Nima`. This decision is correct for v0.1
  (one scorer); session 04 introduces `Scorer` trait when ARNIQA is the
  second impl.

- **SELECT with supersede filter and source_path (PR1-T12 + PR1-T18)**:
  ```sql
  SELECT id, source_path
  FROM photos
  WHERE superseded_at_unix_seconds IS NULL
    AND id NOT IN (SELECT photo_id FROM cull_scores WHERE scorer = ?1)
  ```

- **ort concurrency model (PR1-T5)**: one `ort::Session` per rayon
  worker thread (option b). Each worker constructs its own `Nima` from
  the shared `LoadedModel`'s model bytes. No `Mutex` wrapping; no async.
  This is verified by D0 §Threading semantics. Memory cost: N workers ×
  ~50 MB model = ~400 MB on 8-core apple-silicon (acceptable for v0.1).

- **Per-photo error dispatch table (PR1-T13 + PR1-T14 remediation)**:

  | Error class | `CullStats` counter | `--strict` behavior |
  |-------------|---------------------|---------------------|
  | `ModelLoadFailed` | n/a (ABORT run) | always FAIL |
  | `ModelVerificationFailed` (SHA mismatch) | n/a (ABORT run) | always FAIL |
  | ort version mismatch at load time | n/a (ABORT run) | always FAIL |
  | `InferenceFailed { source }` | `inference_failed` | FAIL if `inference_failed > 0` |
  | `read_raw_rgb` → `RawDecodeError` | `decode_failed` | FAIL if `decode_failed > 0` |
  | `read_raw_rgb` → file not found | `file_missing` | warn, skip (not a failure) |
  | re-derived PhotoId mismatch (content changed) | `content_changed` | warn, skip |
  | `cull_scores` row already exists | (skip, no counter) | no-op, not an error |

  Low aesthetic score does NOT fail `--strict` (it's a feature, not an
  error — a 1.0-scoring photo is a valid result).

  Integration test per case (6 total: model-missing, SHA-mismatch,
  per-photo decode fail, per-photo inference fail, file-missing, existing
  score).

- **TD-006 closed inline (PR1-T13 + PR1-T24)**: session 03 cull IS the
  TD-006 trigger consumer (first consumer of `decode::read_raw` /
  `read_raw_rgb`). `CullStats` carries per-cause counters (`inference_failed`,
  `decode_failed`, `file_missing`, `content_changed`). TD-006 marked
  Closed after this deliverable's commit.

- **Heartbeat (PR1-T33)**: duplicate `HeartbeatStop` + `heartbeat_loop`
  scaffolding from `ingest.rs` into `cull.rs`. Two consumers is NOT the
  threshold for refactoring — three is. File TD-016 (factor into
  `photohelper-cli::heartbeat` when the third subcommand adds a heartbeat).

- `crates/photohelper-cli/src/commands/mod.rs` — register `cull` module;
  CLI clap subcommand wired with real opts (`catalog path` inherits,
  `--scorer nima` default, `--threshold-warn <N>` for informational
  output, `--strict` extension).

#### Deliverable 5 — TD-010 full closure (Deliverable-6 test infrastructure)

Per `TECH-DEBT.md § TD-010`, ship every sub-item:

- **5a `poison_for_testing` knob** on `Catalog` (3 tests:
  `poison_propagates_as_catalog_poisoned_error`,
  `poison_rollback_discards_panicked_workers_partial_insert`,
  `poison_recovery_admits_subsequent_inserts`). `#[cfg(test)]`-gated
  public-shadowed-by-private approach to avoid R2-T15's dead-public-API
  anti-pattern.
- **5b R2-M8 silent-ROLLBACK fix (PR1-T23 remediation)** at
  `catalog.rs:304` (corrected from `:281` which is `Ok(Self { conn: …
  })`) — explicit match on the extended error code:
  ```rust
  Err(rusqlite::Error::SqliteFailure(e, _))
      if e.code == rusqlite::ErrorCode::ApiMisuse => {} // no active transaction — ignore
  Err(e) => return Err(Error::CatalogTransaction {
      op: "rollback-after-worker-panic", source: Box::new(e) }),
  ```
  NOT matching on the message string (fragile across rusqlite versions).
- **5c Heartbeat death test — restructured (PR1-T2 remediation)**: no
  panic site ever in `heartbeat_loop`; the `cfg!(debug_assertions)` env-var
  approach is dropped entirely.
  - **5c-i** Create `crates/photohelper-test-helpers` (dev-deps only)
    with `pub fn force_heartbeat_panic_in_thread(handle: &JoinHandle<()>)`
    helper. This crate is `[dev-dependencies]` only in
    `photohelper-cli/Cargo.toml`. No env-var path added to production code.
  - **5c-ii** Wire the heartbeat-death-WARN regression test via the
    helper — subprocess integration test or in-process `is_finished()`
    predicate. NO `panic!()` macro in `heartbeat_loop`. TD-005 closed.
  - **5c E2E** (per PR1-T34 remediation): verify `photohelper-test-helpers`
    is `[dev-dependencies]` only (not `[dependencies]`) via `cargo metadata
    --format-version 1 | jq '.packages[] | select(.name == "photohelper-cli")
    | .dependencies | map(select(.name == "photohelper-test-helpers"))'` and
    assert `kind = "dev"`. No `objdump` needed.
- **5d DN-008 6 rows** (relabeled from "DN-008 12 rows" per PR1-T29
  remediation — these are TD-010's 6-of-12-row subset; rows 12, 13, 14,
  18, 19, 34 deferred with companion binding trigger in TECH-DEBT.md):
  - row 6: trybuild compile-fail test for `assert_send_sync!(Arc<Catalog>)`.
  - row 17: hardlink dedup integration test (two paths → same id →
    one row, second `ingest` returns `AlreadyCatalogued`).
  - row 39: `--strict` on CR3-only dir asserts exit 0 with all-EXIF-ok.
  - row 42: walker mtime-future + nested-dirs + broken-symlinks
    edge cases (one fixture per case).
  - row 43: mtime_anomalous flag round-trip — write photo with mtime
    > 2100, assert catalog row's flag is 1.
  - row 49: fatal exit codes (catalog locked / permission denied /
    disk full) → CLI exits with the correct EX_* code (EX_TEMPFAIL =
    75 for locked, EX_NOPERM = 77 for permission, EX_IOERR = 74 for
    disk).
- **5e R2-T18 4 WARN regression tests (PR1-T37 remediation)**:
  - `build_global already initialized` (run `ingest` twice in the
    same process; second call WARNs).
  - `wal_checkpoint recovered N frames` (write, kill, re-open; WARN
    fires with N>0).
  - `file-lock` op-tag (parent dir read-only on Unix; skip Windows;
    error WARNs with op="lock-file-create" per R2-T11).
  - **heartbeat death test parameterized over `[ingest, cull]` drivers
    (PR1-T37 remediation)**: trigger heartbeat death via 5c's test-helper,
    assert `[heartbeat-death-WARN]` substring fires, assert summary line
    still prints — for BOTH `ingest` and `cull` code paths.
- **5f** no-op (R2-T19 already closed at session 01 R2).

#### Deliverable 6 — DN-020 stub-message fix (first chore commit)

**(PR1-T1 + PR1-T35 remediation: correct target, correct message,
correct LoC estimate.)**

The stubs live in `crates/photohelper-cli/src/main.rs:127-130` via the
shared `stub(name, planned_in)` helper and the per-arm `planned_in`
literals. There are NO per-subcommand files in `commands/` for these
stubs. The `cull` subcommand is NOT in this fix (D4 replaces its stub
with real impl).

Changes:
- Rewrite the `stub(name, planned_in)` body in `main.rs:127-130` to
  emit: `"photohelper {name}: not yet implemented in v0.1 (ingest + cull
  only); see README.md for the current scope."`
- Remove the `planned_in` parameter (no longer needed — the new message
  points at a public artifact, not a session identifier).
- Update the 5 call sites (`camera`, `develop`, `export`, `run`, `models`)
  in `main.rs` to drop the `planned_in` argument.
- **Negative test**: assert that `photohelper cull --help` does NOT emit
  the stub-message text (confirms D4 wired cull correctly).
- **LoC estimate**: ~8 (one function rewrite + 5 call-site tweaks).
- **Commit shape**: dedicated `chore(cli): refresh stub-subcommand
  messages (closes DN-020)` as session 03's FIRST commit after the
  plan remediation lands.

#### Deliverable 7 — Documentation polish

- README: add a `Cull a catalog` quickstart section.
- README: add `§ Roadmap` section (replaces `SESSION-STATE.md` reference
  in stub message; users running release binaries can find the scope here).
- README: add a one-paragraph "two-shell PATH drift footgun" callout
  closing DN-021.
- `docs/discovery-notes.md` checkpoint at session end.
- `docs/decisions/0002-catalog-schema-v2.md` per Deliverable 2c.
- **DN-003 closure (PR1-T40 remediation)**: close DN-003 with an
  append-only addendum: "session 03 decides in-process for v0.1;
  reassessment trigger = 5+ GitHub issues tagged `crash:ort-inference`
  OR 2026-12-01, whichever first." Drop ADR-0003 from scope (a deferral
  with a future reassessment trigger is not a binding architectural
  decision; decision-doc 0002 covers the schema decisions).

### What is out of scope (deferrals)

- **`Scorer` trait** — session 04 (introduce when ARNIQA lands as
  the second impl; one impl does not need a trait).
- **`--model-path` power-user flag** — deferred (TD-015; requires
  `--model-sha256` companion to maintain the SHA trust boundary).
- **ARNIQA technical-quality model** — session 04.
- **Face / eye-state model** — session 04+.
- **MobileCLIP dup-group computation** — session 04+ (DN-024). The
  `dup_groups` table itself also deferred; see §D2b.
- **AI denoise** — session 05+ (SCUNet or replacement; deferral stands
  on its own without fabricated ANL-001 cross-reference — PR1-T4 fix).
- **AI sharpen** — session 06+.
- **In-process vs subprocess inference for crash isolation** — DN-003;
  reassessment at 5+ crash issues OR 2026-12-01 (see §D7).
- **TD-002 full rusqlite bump** — depends on MSRV bump (1.88 → 1.92).
- **TD-001 GitHub Actions SHA pinning** — release-engineering session.
- **TD-004 LibRaw CVE monitoring** — bundle with release-engineering.
- **TD-005 heartbeat env-var panic site** — **closed in D5c this session.**
- **TD-006 RawDecodeCause dispatch** — **closed inline in D4 this session
  (PR1-T24: session 03 cull IS the trigger consumer).**
- **TD-007 empty-path PathBuf** — session 03 D1c USES `decode.rs` types
  but adds no new constructors; TD-007's "next session touching
  `decode.rs`" trigger does NOT fire here (only new `read_raw_rgb` path
  is added; the existing constructors in `decode.rs` are unchanged).
  Calendar trigger 2026-08-01 remains operative.
- **TD-011 deferred session-end review** — deferred to **session 05 AT
  LATEST** (TD-011's 3-session bound from session 02 = sessions 03/04/05;
  if not closed by session-05 session-end, escalate to CRITICAL per
  TD-011's own binding trigger clause — PR1-T25 remediation).
- **`Migration` trait upgrade** — recorded in decision-doc 0002 as a
  design direction; promote when v3 migration is multi-step or
  data-migrating (NOT a stop-gap TD — the match-arm is the correct
  design for one migration).
- **DN-008 deferred 6 rows** (rows 12, 13, 14, 18, 19, 34): companion
  binding trigger filed alongside TD-010 closure at §D5d. Each row has
  a known owner in TD-010's revised ledger entry.
- **Cull-decision UI** — session 05+.
- **Per-cull-run audit trail** — deferred per TD-013.

### How each deliverable is tested

| Deliverable | Unit | Integration | End-to-end |
|-------------|------|-------------|------------|
| D0 pre-flight | n/a | n/a | manual + ANL-002 artifact + commit-message gate (`cve-posture:`, `inference:`, `threading:`) |
| D1a scaffolding | trybuild for new lints | `cargo test -p photohelper-ai --no-run` compiles | n/a |
| D1b registry | `VerifiedModelBytes` rejects SHA mismatch; `LoadedModel` rejects unverified bytes | `ModelRegistry::load` returns Err on missing model; round-trips on success | n/a |
| D1c NIMA scorer | `NimaScore::new` rejects NaN/∞/out-of-range; `from_catalog_f64` saturates at boundary; preprocessing dimension assertions | inference against the CC0 CR3 fixtures (~1s/inference acceptable); score within ±1e-3 of golden on apple-silicon; band `[3.0, 9.0]` on Linux | `cull` writes a row whose score matches the golden fixture vector |
| D1d model file | `models/manifest.toml` SHA-256 matches actual file (verified by build.rs) | sanitize-check (ONNX-aware) passes on the model file | n/a |
| D2a migration | `apply_v1_to_v2` idempotency (run twice, second is no-op per IF NOT EXISTS); ROLLBACK error propagated vs ApiMisuse silenced | `Catalog::open` on a v1-DB upgrades to v2; existing rows preserved; half-applied-migration recovery | `photohelper ingest` on a v1-DB upgrades it transparently |
| D2b v2 schema | FK violation test: non-existent `photo_id` rejected; trybuild for type-mismatch | automated integration test: v1 fixture → user_version=2, `cull_scores` present, existing rows intact | n/a |
| D3 fixtures | sanitize-check (ONNX-aware) on the new model file | golden-vector fixture exists at the right path; v1 catalog fixture at `tests/fixtures/catalogs/v1.db` | n/a |
| D4 `cull` real | `run_cull` skips already-scored photos; supersede filter excludes superseded rows | per-case error dispatch tests (6 cases); end-to-end against both CC0 CR3 fixtures: 2 rows in cull_scores after one cull-run | `photohelper cull --strict` exits 0 on the CC0 fixture set |
| D5a poison knob | 3 unit tests per the TD-010 spec | `cull` continues after a per-photo panic without poisoning the whole catalog | n/a |
| D5b ROLLBACK fix | unit test: `ApiMisuse` swallowed; real DB error propagated | n/a | n/a |
| D5c heartbeat test | 5c-i `photohelper-test-helpers` is `[dev-dependencies]`-only (cargo metadata assertion); 5c-ii heartbeat-death test passes without any panic in `heartbeat_loop` | subprocess integration asserts WARN substring fires for BOTH ingest and cull drivers | n/a |
| D5d DN-008 rows | per-row unit/integration tests per spec above | each test runs independently | n/a |
| D5e WARN regressions | 4 subprocess integration tests asserting WARN substring; heartbeat-death test parameterized over [ingest, cull] | n/a | n/a |
| D6 stub messages | `cargo test -p photohelper-cli --test cli`: each stub's stderr matches the new format; cull --help does NOT emit stub-message | n/a | n/a |
| D7 docs | proofread + link-check | n/a | n/a |

### Which checkpoints fire this session

| When | Checkpoint | Agents | Artifact |
|------|-----------|--------|----------|
| Now (after plan v2 commit) | **Plan review Round 2** (Tier 5, full suite) | 8 + 9th verifier | `docs/code-reviews/session-03-plan-round2.md` |
| After D1a + D1b + D1c land | Sub-component review | 3–5 agents (Cadence A Tier 4) | `docs/code-reviews/session-03-photohelper-ai-round{1,2}.md` |
| After D2a + D2b land | Sub-component review | 3–5 agents | `docs/code-reviews/session-03-catalog-migration-round{1,2}.md` |
| After all deliverables land + `just ci` green | **Session-end review** (Tier 5, full suite) | 8 + 9th verifier | `docs/code-reviews/session-03-round{1,2,3?}.md` |

### Acceptance criteria (PR-merge gate)

1. `just ci` GREEN on apple-silicon (fmt + clippy zero-warnings +
   tests + audit + sanitize-check + unsafe-isolation).
2. `photohelper cull --strict` exits 0 against the existing
   `tests/fixtures/cr3/` set; `cull_scores` table has one row per
   fixture with `score ∈ [1.0, 10.0]`.
3. `photohelper ingest "$HOME/Pictures/tests"` followed by
   `photohelper cull "$HOME/Pictures/tests"` (the user's 371-entry test
   directory — 370 CR3 + 1 `.photohelper` catalog dir, per session-02
   production trace) produces a non-zero count of `cull_scores` rows
   within `D0_measured_per_photo_time × 370 / num_cpus × 1.5` seconds
   wall-clock (50% headroom over D0's measured per-photo benchmark,
   per PR1-T21 remediation — NOT a fixed "30 min" bound).
4. `cargo audit --deny warnings` clean on the bumped workspace
   (now includes ort).
5. ANL-002 records CVE-posture clean + NIMA license verified +
   2/2 fixture inference success + threading semantics verified.
6. TD-010 closed in `TECH-DEBT.md`; TD-005 + TD-006 closed in lockstep.
7. DN-005 + DN-020 closed in `docs/discovery-notes.md`.
8. Decision-doc 0002 lands authoritative for catalog v2.
9. `git log --first-parent main` shows one merge commit for this
   session.

### Discovery items expected

- **ort version + features finalize at D0**: `download-binaries`
  vs static linking is the most consequential choice; static
  linking eliminates the runtime-download step but balloons the
  binary.
- **NIMA model provenance + license**: the most cited "NIMA"
  weights come from `idealo/image-quality-assessment` (MIT) but the
  ONNX export quality varies; plan-review may surface an alternative.
- **Demosaic algorithm**: v0.1 uses LibRaw's default AHD; if D0's
  inference results are clearly degraded vs LibRaw's full dcraw
  pipeline, upgrade to AMaZE or AAHD within D1c scope. See DN-023.
- **cull_scores supersede semantics**: composite PK `(photo_id,
  scorer)` is the bet; decision-doc 0002 records this choice.
- **ort RC stability**: 2.0.0-rc.12 (or whichever RC D0 pins) may
  have known rough edges; TD-014 tracks the upgrade path to stable.

### Stop-gap declarations (per `CLAUDE.md § No Acceptable Trade-offs Policy`)

*(PR1-T7 remediation: §Stop-gap declarations now enumerates all stop-gaps
with companion TD entries. "None" was incorrect.)*

| # | Stop-gap | In-source label | TD |
|---|----------|-----------------|----|
| 1 | LibRaw default AHD demosaic algorithm for NIMA preprocessing; session 04+ develop pipeline may need explicit AMaZE/AAHD/VNG4 selector | `// TD-012: AHD demosaic stop-gap` in `nima.rs` preprocessing call | TD-012 |
| 2 | Per-cull-run audit trail absent from `cull_scores` (no `run_id`, no config snapshot per run) | `// TD-013: per-cull-run audit trail absent` in `insert_cull_score` | TD-013 |
| 3 | ort RC pin `=<pin-from-D0>` (release candidate, not stable) | `// TD-014: ort RC pin; upgrade to stable when released` in `Cargo.toml` comment | TD-014 |
| 4 | `--model-path` power-user override dropped from v0.1; users cannot supply custom ONNX models | noted in `main.rs` clap subcommand comment | TD-015 |
| 5 | `HeartbeatStop` + `heartbeat_loop` duplicated in `cull.rs` (two callers; factor at three) | `// TD-016: heartbeat duplicated; factor at third subcommand` | TD-016 |

### Plan revisions log

- **v1** (2026-05-28) — initial contract. Plan-review Round 1 surfaced
  10 CRITICAL + 18 HIGH + 10 MEDIUM + 5 LOW. All 34 CRITICAL+HIGH
  verified by 9th-agent (0 hallucinated).
- **v2** (2026-05-28) — R1 remediation. Closes all 10 CRITICAL + 18
  HIGH + 5 relevant MEDIUM:
  - **PR1-T1**: D6 retargeted to `main.rs:127-130`; LoC estimate corrected
    to ~8; message points at `README.md`; cull excluded from stub loop.
  - **PR1-T2**: D5c restructured as 5c-i (test-helpers crate) + 5c-ii
    (test via helper); no panic site ever in `heartbeat_loop`.
  - **PR1-T3**: DN-022/023/024/025 filed (phantom citations removed).
  - **PR1-T4**: fabricated `per ANL-001 § out-of-scope` clause dropped.
  - **PR1-T5**: D0 §Threading semantics added; D4 picks per-worker
    Session (option b).
  - **PR1-T6**: Scorer trait dropped; D4 takes concrete `&Nima`.
  - **PR1-T7**: stop-gap table with 5 TDs (TD-012–TD-016).
  - **PR1-T8**: D2b automated integration test replaces manual REPL.
  - **PR1-T9**: D0 sequencing corrected to D0→D1a→D1b/c→D1d.
  - **PR1-T10**: `VerifiedModelBytes` type-state added to D1b.
  - **PR1-T11**: ort dep uses `<pin-from-D0>` placeholder; TD-014 filed.
  - **PR1-T12**: D4 SELECT adds supersede filter; supersede semantics
    documented in decision-doc 0002.
  - **PR1-T13**: D4 error dispatch table added; TD-006 closed inline.
  - **PR1-T14**: `--strict` semantics table with 6 cases resolved.
  - **PR1-T15**: `apply_v1_to_v2` ROLLBACK propagation specified via
    `ErrorCode::ApiMisuse` match; recovery test added.
  - **PR1-T16**: D1d filename is `<model_chosen_at_D0>.onnx` placeholder.
  - **PR1-T17**: full schema-version state machine specified in D2a.
  - **PR1-T18**: D4 SELECT includes `source_path`; file-missing + content-
    changed per-photo handling specified.
  - **PR1-T19**: Migration trait replaced with `match` arm + `apply_v1_to_v2()`.
  - **PR1-T20**: NIMA preprocessing uses LibRaw `dcraw_process` pipeline;
    `read_raw_rgb` added to photohelper-raw.
  - **PR1-T21**: acceptance criterion 3 SLO based on D0 measurement.
  - **PR1-T22**: `PRAGMA foreign_keys = ON` added to §2a PRAGMA loop.
  - **PR1-T23**: D5b corrected to `catalog.rs:304`; `ErrorCode::ApiMisuse`.
  - **PR1-T24**: TD-006 + TD-007 binding-trigger status explicitly acked.
  - **PR1-T25**: TD-011 3-session bound explicitly noted (session 05 latest).
  - **PR1-T26**: NIMA golden-vector ±1e-3 tolerance + `nima-regenerate-golden`
    recipe + DN-025.
  - **PR1-T27**: ModelRegistry trait dropped; `--model-path` dropped; TD-015.
  - **PR1-T28**: D3 ONNX sanitize-check uses Python ONNX-aware check.
  - **PR1-T29**: DN-008 rows relabeled as "TD-010's 6-of-12-row subset."
  - **PR1-T30**: `dup_groups` dropped from v2 schema; DN-024 filed.
  - **PR1-T31**: NimaScore gets `Copy + PartialOrd + from_catalog_f64`.
  - **PR1-T33**: heartbeat factoring decided: duplicate for v0.1; TD-016.
  - **PR1-T34**: D5c E2E is `cargo metadata` grep (not `objdump`).
  - **PR1-T35**: D6 message points at README.md (not SESSION-STATE.md).
  - **PR1-T36**: DN-005 updated in discovery-notes.md (append-only).
  - **PR1-T37**: D5e row 4 parameterized over [ingest, cull] drivers.
  - **PR1-T38**: "371-entry test directory (370 CR3 + 1 .photohelper)".
  - **PR1-T39**: ai-culling feature gate purpose documented inline.
  - **PR1-T40**: ADR-0003 dropped; DN-003 closed with addendum in D7.
  - **PR1-T41**: R3-T3 citation corrected to "per R3-T3 remediation
    option (c)" (moot: D5c now structured differently).
  - **PR1-T42**: empty plan-revisions-log section removed from v1.
