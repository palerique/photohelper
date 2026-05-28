# Session 03 — `ai-culling-skeleton`

> **Branch**: `session-03/ai-culling-skeleton`
> **Started**: 2026-05-28
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v1 (this revision)

> **Note on title slug**: branch is `ai-culling-skeleton` but the session
> lands the catalog v1→v2 migration framework + cull_scores + dup_groups
> tables AND the photohelper-ai end-to-end NIMA culling pipeline AND
> closes TD-010's full Deliverable-6 test infrastructure in one PR (see
> §Scope rationale).

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the end-to-end AI culling pipeline that turns `photohelper-ai`
from a 6-line stub into a working NIMA aesthetic-score scorer, plus
the catalog v1→v2 migration framework + new `cull_scores` and
`dup_groups` tables those scores land in, plus closing
`TECH-DEBT.md § TD-010` (the full Deliverable-6 test infrastructure
deferred from session 02), plus the first-chore stub-message fix for
`docs/discovery-notes.md § DN-020`. Four complementary deliverables
under the same `cull` subcommand surface:

1. **`photohelper-ai::nima::score(raw: &RawImage) -> Result<NimaScore>`** —
   load a vendored NIMA ONNX model, downsample the LibRaw-decoded RAW
   to the model's input shape (224×224×3), run a single forward pass,
   return the aesthetic score (1.0–10.0 float, lower = worse). This
   is the **DN-005 critical-path remediation**: catalog needs a
   meaningful `cull_score` column to drive `cull` subcommand output.

2. **`photohelper-catalog::migrations`** — a small migration framework
   (Migration trait + version registry + applier) that lets us bump
   `PRAGMA user_version` from 1 to 2 and add the new tables idempotently.
   Decision-doc 0001 § Amendments (2026-05-28) explicitly assigns this
   ownership to session 03.

3. **`cull` subcommand wired for real** — was stub `exit 69` from
   session 01; now walks catalog rows, decodes each photo's RAW via
   `photohelper-raw::decode::read_raw`, runs NIMA inference, writes
   `cull_scores` rows. Heartbeat liveness + summary line analogous to
   `ingest`.

4. **TD-010 full closure** — Deliverable-6 test infrastructure ships
   in full: `poison_for_testing` knob on `Catalog`, R2-M8 silent-ROLLBACK
   fix, heartbeat `panic_for_testing` env-var (which also retires TD-005),
   DN-008 6 rows `{6, 17, 39, 42, 43, 49}`, R2-T18 4 WARN regression
   tests.

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
migration runner, new schema-version gate, new tables, new
insert/select paths for `cull_scores`). Per `CLAUDE.md § No Acceptable
Trade-offs Policy`, deferring TD-010 a second time without closing the
fired trigger is a policy violation. Full closure here closes TD-010
AND lands the test infrastructure that the new code will itself need
(specifically: `poison_for_testing` for a `cull`-on-poisoned-catalog
regression test).

**DN-020 stub-message fix is the natural first commit**. DN-020's
binding trigger reads *"session 03 session-start sweep — the
SESSION-STATE.md Goal block surfaces this as a quick-win first-commit
candidate."* Lands first to clear the trivial drift before the
substantive work begins.

The session is large but cohesive: every deliverable touches the same
two crates (`photohelper-ai` + `photohelper-catalog`) and the same
subcommand (`cull`). The plan-review and session-end review surface
the actual line counts; preliminary estimate is ~1500-2000 LoC across
~20-25 commits.

### Deliverables (when the PR merges, the following will exist)

#### Deliverable 0 — Pre-flight feasibility probe

Before any ort/NIMA wiring, verify (a) `ort` 2.x is CVE-clean against
RustSec + the OSV.dev ONNX-ecosystem feed, (b) a NIMA ONNX model
exists with a clear license + provenance + reproducible SHA-256, and
(c) end-to-end inference works on the existing CC0 Canon R8 CR3
fixtures from session 02.

- **Sequencing**: fires AFTER Deliverable 1's first scaffolding commit
  (need `ort` dep wired) AND BEFORE Deliverable 4's `cull` rewire.
- **Artifact**: `docs/analysis/ANL-002-ort-nima-preflight.md` with:
  - **§ ort version**: chosen X.Y.Z (latest 2.x; check rust-lang.org/cargo
    + ort github for the current pin) + `download-binaries` vs
    static-linking decision.
  - **§ CVE-posture-as-of-pin**: RustSec advisory feed + OSV.dev ONNX
    Runtime grep + ort GitHub Security Advisories on the pin date for
    any open CVE affecting the chosen version; record per-CVE
    pass/fail. ABORT if any open CVE → escalate to plan-review v4.
  - **§ NIMA model provenance**: source URL + author + license (must
    be permissive: MIT / Apache-2.0 / CC-BY-4.0; reject CC-BY-NC or
    research-only); SHA-256 verified; ONNX opset version; input
    shape; output range.
  - **§ Inference end-to-end**: run NIMA against both CC0 R8 CR3
    fixtures from `tests/fixtures/cr3/`; record per-fixture
    aesthetic_score; verify deterministic across re-runs. ABORT if
    fixture inference fails → escalate to plan-review v4.
- **Commit shape**: dedicated `chore(ai): pre-flight ort + NIMA
  audit (Deliverable 0)` commit; result auditable in `git log`. **No
  Deliverable 4 commit may land before this one.**
- **Verification surface**: pre-flight commit message MUST include
  `cve-posture: clean (versus RustSec + OSV.dev YYYY-MM-DD)` AND
  `inference: 2/2 fixtures, scores [a, b]` so session-end review can
  grep the commit.

#### Deliverable 1 — `photohelper-ai` real implementation

##### 1a — Crate scaffolding + ort dep wiring

- `crates/photohelper-ai/Cargo.toml`:
  - `[dependencies] ort = { workspace = true }` (workspace dep added).
  - Re-add `photohelper-core` workspace dep (per existing commented
    note in lib.rs); also add `photohelper-raw` for `RawImage` input.
  - Feature flag `default = ["ai-culling"]` (per existing
    commented-out gate); `ai-denoise` / `ai-sharpen` /
    `tract-fallback` stay deferred.
- `crates/photohelper-ai/src/lib.rs`: replace stub with module
  declarations + crate-level rustdoc.
- New `crates/photohelper-ai/src/error.rs`: domain `Error` enum
  (`#[non_exhaustive]`, `thiserror::Error`-derived) with at minimum:
  `ModelLoadFailed { path, source }`, `InferenceFailed { source }`,
  `InvalidInputShape { expected, got }`,
  `ModelLicenseUnverified { path }` (defense-in-depth).
- Workspace `Cargo.toml` `[workspace.dependencies]` adds
  `ort = { version = "=2.0.X", default-features = false, features = [...] }`
  with version + features locked at Deliverable-0 pin time.

##### 1b — Model registry + loader

- `crates/photohelper-ai/src/registry.rs`: `ModelRegistry` trait with
  `fn load(name: &str) -> Result<LoadedModel>`. Single concrete impl
  `BundledModelRegistry` that resolves names to vendored model files
  at runtime via `<user-config>/photohelper/models/<name>.onnx`,
  copying from the bundled `include_bytes!` payload on first call if
  the file is absent. `--model-path` CLI flag on `cull` overrides
  for power users.
- `LoadedModel` newtype wraps `ort::Session` with the model metadata
  (input shape, output shape, opset, SHA-256). Private fields,
  fallible constructor verifying the SHA-256 matches the
  `models/manifest.toml` declaration.

##### 1c — NIMA scorer

- `crates/photohelper-ai/src/nima.rs`: `Nima` struct holds a
  `LoadedModel`; `Nima::score(raw: &RawImage) -> Result<NimaScore>`
  is the public entry. `NimaScore` newtype wraps `f32` constrained
  to `[1.0, 10.0]` (fallible constructor; reject NaN, ±∞,
  out-of-range).
- Preprocessing pipeline: `RawImage` → demosaic-to-RGB (use existing
  `WhiteBalance` + `CamRgbToXyzD65Matrix` if available; minimal
  bilinear demosaic if not — DN-022 will track the "real demosaic"
  scope) → bilinear downsample to 224×224 → normalize per NIMA's
  ImageNet stats (per ANL-002 § NIMA model provenance).
- Optional second module `nima_postproc.rs` for the score-distribution
  → single-scalar reduction (NIMA outputs a 10-bin distribution; the
  scalar score is the weighted mean).

##### 1d — Bundled model file

- `crates/photohelper-ai/models/nima_aesthetic_v1.onnx` — vendored
  via Git LFS (analog to session 02's `tests/fixtures/cr3/*.cr3`).
  `.gitattributes` extended to track `*.onnx`.
- `crates/photohelper-ai/models/manifest.toml` — per-model SHA-256 +
  license + provenance + source-URL + opset + input-shape, all
  string-keyed by model name. Cross-checked against ANL-002.
- `crates/photohelper-ai/build.rs` — verifies the bundled NIMA file's
  SHA-256 matches `manifest.toml` at build time (analog to session
  02's LibRaw tarball SHA-256 check).

#### Deliverable 2 — Catalog v1→v2 migration framework

##### 2a — Migration framework

- `crates/photohelper-catalog/src/migrations.rs` — new module:
  - `trait Migration { fn version(&self) -> u32; fn up(&self, tx: &Transaction) -> Result<()>; }`
  - `static MIGRATIONS: &[&dyn Migration]` registry, sorted by version,
    no gaps.
  - `pub(crate) fn apply_pending(conn: &mut Connection) -> Result<u32>`:
    reads `PRAGMA user_version`, applies each migration with version
    > current, bumps `user_version` per applied step, all wrapped in
    `BEGIN IMMEDIATE; ...; COMMIT;` per per-step idempotency.
- `Catalog::open` calls `apply_pending` after the schema-version gate
  check but before returning the handle. Existing schema-version
  mismatch error path is preserved.

##### 2b — v2 schema

- New `cull_scores` table:
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
  - Composite PK so a photo can have one row per scorer (NIMA-aesthetic
    now; ARNIQA-technical, face-quality, etc. later).
  - FK to `photos(id)` for referential integrity; `ON DELETE CASCADE`
    deliberately NOT added (v0.1 has no delete path; tracked in DN-022
    if surfaced).
  - `score REAL` (not INTEGER × 100) — preserve fractional precision
    for downstream sort/threshold operations.
- New `dup_groups` table:
  ```sql
  CREATE TABLE IF NOT EXISTS dup_groups (
      group_id INTEGER PRIMARY KEY AUTOINCREMENT,
      photo_id BLOB NOT NULL REFERENCES photos(id),
      embedding BLOB NOT NULL,
      created_at_unix_seconds INTEGER NOT NULL,
      UNIQUE (photo_id)
  );
  CREATE INDEX IF NOT EXISTS idx_dup_groups_group_id ON dup_groups(group_id);
  ```
  - Stores per-photo embeddings; the v0.1 cull session does NOT populate
    this table (no MobileCLIP yet); table exists so the v2 schema is the
    one v0.1 ships and v0.2 doesn't need another migration.
  - DN-022 captures the dup-detection compute step as a session 04+
    deliverable.
- `PRAGMA user_version = 2;` at the end of migration v1→v2.

##### 2c — Decision-doc 0002

- `docs/decisions/0002-catalog-schema-v2.md` — extends decision-doc
  0001 with the v2 additions: cull_scores rationale, dup_groups
  rationale, why no DELETE CASCADE, what's out of scope for v3+
  (per-cull-run audit trail, per-scorer config snapshot, etc.).
- Cross-link to ANL-002 (the NIMA model is what the cull_scores rows
  reference via `scorer`).

#### Deliverable 3 — Fixture additions

- Reuse session 02's two CC0 Canon R8 CR3 fixtures at
  `tests/fixtures/cr3/*.cr3` — no new image fixtures needed.
- New `crates/photohelper-ai/tests/fixtures/nima/` — small golden
  ONNX-output fixture (the aesthetic_score vector for one CR3) so
  inference-regression tests can pin the expected output without
  re-running the full forward pass.
- Extend `scripts/sanitize-check.sh` to scan the new NIMA model file
  for any embedded EXIF / GPS / surprising metadata before the
  build.rs SHA-256 check (defense in depth; the file is fetched from
  upstream and may carry unexpected metadata).

#### Deliverable 4 — `cull` subcommand rewire

- `crates/photohelper-cli/src/commands/cull.rs` — was stub
  `exit 69`; now real implementation:
  - `run_cull(catalog: &Catalog, scorer: &dyn Scorer, opts: CullOpts) -> Result<CullStats>`
  - Walks catalog rows (`SELECT id FROM photos WHERE id NOT IN (SELECT photo_id FROM cull_scores WHERE scorer = ?1)`)
    for the current scorer (NIMA aesthetic now).
  - Per row: `read_raw` → `nima.score` → `Catalog::insert_cull_score`.
  - Rayon `par_bridge` for parallel inference; ort sessions are
    reusable (Send + Sync), so one session shared across worker
    threads.
  - Heartbeat thread (reuse `HeartbeatStop` from session 02's
    `ingest`); summary line analogous to ingest.
  - `--strict` predicate extension (TBD per plan-review): cull
    failures `errored > 0` fail; `aesthetic_score_below_threshold`
    surfaces but does NOT fail (it's a feature, not an error).
- `crates/photohelper-cli/src/commands/mod.rs` — register `cull`
  module; CLI clap subcommand definition updated to wire real opts
  (catalog path inherits, `--scorer nima` default, `--model-path
  <override>`, `--threshold-warn <N>`).

#### Deliverable 5 — TD-010 full closure (Deliverable-6 test infrastructure)

Per `TECH-DEBT.md § TD-010`, ship every sub-item:

- **5a `poison_for_testing` knob** on `Catalog` (3 tests:
  `poison_propagates_as_catalog_poisoned_error`,
  `poison_rollback_discards_panicked_workers_partial_insert`,
  `poison_recovery_admits_subsequent_inserts`). `#[cfg(test)]`-gated
  public-shadowed-by-private approach to avoid R2-T15's dead-public-API
  anti-pattern.
- **5b R2-M8 silent-ROLLBACK fix** at `catalog.rs:281` — explicit
  match: "cannot rollback - no transaction is active" → silent
  ignore; every other rusqlite error → propagate with op tag.
- **5c Heartbeat panic-for-testing env-var** — wires the env-var
  read in `heartbeat_loop` but `cfg!(debug_assertions)`-gated +
  `#[allow(clippy::panic, reason = "test-only env-var")]` per R3-T3.
  **AND** retires TD-005 by adding the proper fix in lockstep:
  factor the panic site into a `photohelper-test-helpers` dev-deps
  crate so release builds carry zero panic surface. Subprocess
  integration test asserts `[heartbeat-death-WARN]` substring per
  R3-T7.
- **5d DN-008 6 rows** `{6, 17, 39, 42, 43, 49}`:
  - row 6: trybuild compile-fail test for `assert_send_sync!(Arc<Catalog>)`.
  - row 17: hardlink dedup integration test (two paths → same id →
    one row, second `ingest` returns `AlreadyCatalogued`).
  - row 39: --strict on CR3-only dir asserts exit 0 with all-EXIF-ok.
  - row 42: walker mtime-future + nested-dirs + broken-symlinks
    edge cases (one fixture per case).
  - row 43: mtime_anomalous flag round-trip — write photo with mtime
    > 2100, assert catalog row's flag is 1.
  - row 49: fatal exit codes (catalog locked / permission denied /
    disk full) → CLI exits with the correct EX_* code (EX_TEMPFAIL =
    75 for locked, EX_NOPERM = 77 for permission, EX_IOERR = 74 for
    disk).
- **5e R2-T18 4 WARN regression tests**:
  - `build_global already initialized` (run `ingest` twice in the
    same process; second call WARNs).
  - `wal_checkpoint recovered N frames` (write, kill, re-open; WARN
    fires with N>0).
  - `file-lock` op-tag (parent dir read-only on Unix; skip Windows;
    error WARN's with op="lock-file-create" per R2-T11).
  - heartbeat death via env-var (uses 5c's panic_for_testing knob;
    asserts WARN substring then summary still prints).
- **5f** no-op (R2-T19 already closed at session 01 R2 — see
  SESSION-STATE.md).

#### Deliverable 6 — DN-020 stub-message fix (first chore commit)

- For each stub subcommand in `crates/photohelper-cli/src/commands/`
  (`camera`, `cull`, `develop`, `export`, `run`, `models`), replace
  the stale `"<subcommand>: not yet implemented (planned for session
  02)"` message with `"<subcommand>: not yet implemented; see
  SESSION-STATE.md for the current roadmap"`. **Special case for
  `cull`**: this session DOES implement cull, so cull's stub
  message gets replaced with real impl in Deliverable 4 — no DN-020
  fix needed for that one.
- Commit shape: dedicated `chore(cli): refresh stub-subcommand
  messages (closes DN-020)` as session 03's FIRST commit after the
  plan-review remediation lands. Single ~6-line edit per file × 5
  files = ~30 LoC total.

#### Deliverable 7 — Documentation polish

- README: add a `Cull a catalog` quickstart section analogous to
  the session 02.5 sub-sessions' `Reset a catalog` + `List ingested
  photos` sections.
- README: add a one-paragraph "two-shell PATH drift footgun"
  callout closing DN-021 (informational; remind users to `git pull
  --ff-only origin main` after a sub-session merge if their
  terminal is in a separate working copy).
- `docs/discovery-notes.md` checkpoint at session end.
- `docs/decisions/0002-catalog-schema-v2.md` per Deliverable 2c.
- `docs/adr/0003-onnx-runtime-in-process-vs-subprocess.md` (DN-003
  closure; v0.1 decision is "in-process, defer subprocess to v0.5
  reassessment when crash-rate data exists" — codify the decision +
  trigger).

### What is out of scope (deferrals → `TECH-DEBT.md`)

- **ARNIQA technical-quality model** — session 04 (orthogonal scorer
  via the same `Scorer` trait + cull_scores composite PK).
- **Face / eye-state model** — session 04+ (different model architecture;
  bounding-box + per-face classification).
- **MobileCLIP dup-group computation** — session 04+. The `dup_groups`
  table SHAPE ships in v2 schema but unpopulated. New TD entry filed.
- **AI denoise** — session 05+ (SCUNet or replacement; per ANL-001 §
  out-of-scope, the original SCUNet plan is in flux).
- **AI sharpen** — session 06+ (Real-ESRGAN or replacement).
- **In-process vs subprocess inference for crash isolation** — DN-003;
  v0.5 reassessment per ADR-0003.
- **TD-002 full rusqlite bump** — still depends on MSRV bump (1.88 →
  1.92); separate session.
- **TD-001 GitHub Actions SHA pinning** — release-engineering session.
- **TD-004 LibRaw CVE monitoring (osv-scanner wiring)** — bundle with
  the release-engineering session.
- **TD-011 deferred session-02 8-agent multi-agent review** — separate
  review-only session before v0.1 release tag.
- **Real demosaic** for NIMA preprocessing — minimal bilinear is OK
  for v0.1 (the model is robust to demosaic quality); a high-quality
  demosaic (AMaZE, AAHD, VNG4) lands when the develop pipeline
  surfaces it as needed (session 04+). New TD entry filed if any
  real-image quality regression surfaces in plan-review.
- **Per-cull-run audit trail** in cull_scores (which run, which
  config) — defer to v0.3+ when users have run cull more than once
  and want diffs. New TD entry filed.
- **Cull-decision UI** (the actual "this is a keeper" vs "reject"
  rubric on top of scores) — session 05+ (probably a `cull suggest`
  subsubcommand or a sidecar XMP rating-write path). New TD entry
  filed.

### How each deliverable is tested

| Deliverable | Unit | Integration | End-to-end |
|-------------|------|-------------|------------|
| D0 pre-flight | n/a | n/a | manual + `ANL-002` artifact + commit-message gate |
| D1a scaffolding | trybuild for the new lints | `cargo test -p photohelper-ai --no-run` compiles | n/a |
| D1b registry | `BundledModelRegistry::load` returns Err on missing model; verifies SHA mismatch fails; round-trips on success | `cargo test -p photohelper-ai registry::tests` | n/a |
| D1c NIMA scorer | `NimaScore::new` rejects NaN/∞/out-of-range; preprocessing dimension assertions | inference against the CC0 CR3 fixtures (~1s/inference acceptable) | `cull` writes a row whose score matches the golden fixture vector |
| D1d model file | `models/manifest.toml` SHA-256 matches actual file (verified by build.rs) | sanitize-check passes on the model file | n/a |
| D2a migration framework | `apply_pending` idempotency (run twice, second is no-op); version-gap detection | `Catalog::open` on a v1-DB upgrades to v2; existing rows preserved | `photohelper ingest` on a v1-DB upgrades it transparently |
| D2b v2 schema | trybuild for FK type-mismatch; trybuild for `INSERT cull_scores` without matching `photos` row fails | `Catalog::insert_cull_score` round-trips via `Catalog::cull_scores_by_photo_id` | manual SQLite REPL inspection |
| D3 fixtures | sanitize-check on the new model file | golden-vector fixture exists at the right path | n/a |
| D4 `cull` real | `run_cull` skips photos that already have a score; rayon parallel inference | end-to-end against both CC0 CR3 fixtures: 2 rows in cull_scores after one cull-run | `photohelper cull --strict` exits 0 on the CC0 fixture set |
| D5a poison knob | 3 unit tests per the TD-010 spec | `cull` continues after a per-photo panic without poisoning the whole catalog | n/a |
| D5b ROLLBACK fix | unit test: cancellation-class error swallowed; real DB error propagated | n/a | n/a |
| D5c heartbeat panic | subprocess integration asserts WARN substring; release build has zero panic surface | `cargo build --release && objdump | grep panic` finds no heartbeat-related panic | n/a |
| D5d DN-008 rows | per-row unit/integration tests per the spec above | each test runs independently | n/a |
| D5e WARN regressions | 4 subprocess integration tests asserting WARN substring | n/a | n/a |
| D6 stub messages | `cargo test -p photohelper-cli --test cli` assert each stub's stderr matches the new format | n/a | n/a |
| D7 docs | proofread + lint pass (md formatter if any) | n/a | n/a |

### Which checkpoints fire this session

| When | Checkpoint | Agents | Artifact |
|------|-----------|--------|----------|
| Now (after plan v1 commit) | **Plan review** (Tier 5, full suite) | 8 + 9th verifier | `docs/code-reviews/session-03-plan-round{1,2,3?}.md` |
| After D1a + D1b + D1c land (photohelper-ai first non-stub public API) | Sub-component review | 3-5 agents (Cadence A Tier 4) | `docs/code-reviews/session-03-photohelper-ai-round{1,2}.md` |
| After D2a + D2b land (catalog migration framework + v2 schema) | Sub-component review | 3-5 agents | `docs/code-reviews/session-03-catalog-migration-round{1,2}.md` |
| After all deliverables land + `just ci` green | **Session-end review** (Tier 5, full suite) | 8 + 9th verifier | `docs/code-reviews/session-03-round{1,2,3?}.md` |

### Acceptance criteria (PR-merge gate)

1. `just ci` GREEN on apple-silicon (fmt + clippy zero-warnings +
   tests + audit + sanitize-check + unsafe-isolation).
2. `photohelper cull --strict` exits 0 against the existing
   `tests/fixtures/cr3/` set; cull_scores table has one row per
   fixture with `score ∈ [1.0, 10.0]`.
3. `photohelper ingest "$HOME/Pictures/tests"` followed by
   `photohelper cull "$HOME/Pictures/tests"` (the user's 370 CR3
   set) produces a non-zero count of cull_scores rows and
   wall-clock < 30 min on apple-silicon (sanity bound; actual
   target TBD per Deliverable-0 inference benchmark).
4. `cargo audit --deny warnings` clean on the bumped workspace
   (now includes ort).
5. ANL-002 records CVE-posture clean + NIMA license verified +
   2/2 fixture inference success.
6. TD-010 closed in `TECH-DEBT.md`; TD-005 closed in lockstep.
7. DN-005 + DN-020 closed in `docs/discovery-notes.md`.
8. Decision-doc 0002 lands authoritative for catalog v2.
9. ADR-0003 lands authoritative for the in-process inference
   decision.
10. `git log --first-parent main` shows one merge commit for this
    session.

### Discovery items expected

- **ort version + features finalize at D0**: `download-binaries`
  vs static linking is the most consequential choice; static
  linking eliminates the runtime-download step but balloons the
  binary. Expect plan-review to push back on whichever choice the
  plan locks.
- **NIMA model provenance + license**: the most cited "NIMA"
  weights come from `idealo/image-quality-assessment` (MIT) but the
  ONNX export quality varies; plan-review may surface an
  alternative model with cleaner provenance.
- **Demosaic quality**: bilinear-only at v0.1; if plan-review
  surfaces an obvious quality regression, escalate to use one of
  LibRaw's built-in demosaic algorithms via the existing FFI.
- **cull_scores schema details**: composite PK `(photo_id,
  scorer)` is the bet; plan-review may prefer `(photo_id, scorer,
  scored_at)` if multiple runs of the same scorer should be kept
  (precedent: photos table preserves supersede audit-trail).
- **--strict semantics for cull**: should low aesthetic scores
  fail strict? probably no (a 1.0 score is not an error, just a
  bad photo); but a missing-model error definitely should fail
  strict. Plan-review nails down.
- **Heartbeat across subcommands**: ingest's heartbeat is in
  `ingest.rs`; cull duplicates the scaffold. Should this be
  factored into a `photohelper-cli::heartbeat` module? Refactor
  may be in-scope or deferred per plan-review preference.

### Stop-gap declarations (per `CLAUDE.md § No Acceptable Trade-offs Policy`)

None known at plan-v1 time. Stop-gaps land via TD entries as the
implementation surfaces them (e.g. if NIMA preprocessing requires
a bilinear-demosaic stop-gap with a real-demosaic TD filed).

### Plan revisions log

- **v1** (this revision) — initial contract. Open: ort version +
  features, NIMA provenance, demosaic quality, cull_scores PK,
  cull --strict semantics, heartbeat refactor. Plan-review Round
  1 fires next.
