# Session 03 — plan-review Round 1

> Per `docs/quality-assurance.md § Plan-review protocol`.
> Cadence A → Tier 5 (plan stage), full 8-agent suite fired in parallel against
> `docs/plans/session-03.md` v1 (committed at `319a25d`).
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

## Triage summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 10 | Plan cannot advance to implementation until all 10 are remediated |
| **HIGH**      | 18 | Address in plan v2 before any code lands |
| **MEDIUM**    | 10 | Address in plan v2 or defer with binding-triggered TD |
| **LOW**       | 5  | When convenient; some defer OK to impl time |

Agent suite: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer` (type),
`pr-review-toolkit:silent-failure-hunter` (sfh), `pr-review-toolkit:comment-analyzer` (com),
`pr-review-toolkit:pr-test-analyzer` (test), `pr-review-toolkit:code-simplifier` (simp).

---

## CRITICAL

### PR1-T1 — D6 targets files that don't exist; stub-message premise is wrong (4-way convergence)

**Agents**: gp (CRITICAL), com (CRITICAL), test (CRITICAL), simp (MEDIUM)

`docs/plans/session-03.md:333-346` says "for each stub subcommand in
`crates/photohelper-cli/src/commands/` (`camera`, `cull`, `develop`, `export`, `run`,
`models`), replace the stale `"<subcommand>: not yet implemented (planned for session 02)"`
message … Single ~6-line edit per file × 5 files = ~30 LoC total."

Three layers of error:
1. **No such files exist.** Verified: `crates/photohelper-cli/src/commands/mod.rs:1`
   contains only `pub mod ingest;`. All stubs are inlined in
   `crates/photohelper-cli/src/main.rs:118-130` via a shared `stub(name, planned_in)` helper.
2. **The stale-message premise is wrong.** `main.rs:127` shows `stub()` emits
   `"photohelper {name}: not yet implemented (planned for {planned_in})"`. The
   `planned_in` string differs per subcommand: only `camera` says `"session 02"`. The
   others already say `"session 03"`, `"session 04"`, `"session 05"`, `"session 06"`. DN-020
   was filed on a wrong reading of the source.
3. **The ~30 LoC estimate is off by 5×.** The real change is one `stub()` function
   rewrite in one file (~6-8 LoC).

Also: the plan proposes pointing users at `SESSION-STATE.md` — an internal file not
present in any distributed binary. Operator running `photohelper develop` from a
release binary gets an actionless message.

**Remediation**: rewrite §D6 to target `main.rs:127-130` (the `stub()` function body +
the per-arm `planned_in` literals). New message should point at a public artifact (e.g.
`README.md § Roadmap`, not `SESSION-STATE.md`). Amend DN-020's `Observed` block to
reflect the actual source layout. Update LoC estimate to ~8. Since `cull` becomes
a real command in D4, remove it from the stub loop AND add a negative test asserting
`cull --help` doesn't emit the stub message.

---

### PR1-T2 — D5c introduces and retires the same panic site in one deliverable (3-way)

**Agents**: rev (CRITICAL), gp (HIGH), simp (MEDIUM)

`docs/plans/session-03.md:300-307` says D5c will:
(a) "wire the env-var read in `heartbeat_loop` but `cfg!(debug_assertions)`-gated +
`#[allow(clippy::panic)]` per R3-T3" AND
(b) "retires TD-005 by adding the proper fix in lockstep: factor the panic site into a
`photohelper-test-helpers` dev-deps crate".

Verified at `TECH-DEBT.md:89-91`: TD-005's Fundamental Fix says "The production
`heartbeat_loop` becomes panic-free … delete the env-var path in `heartbeat_loop`."
The plan simultaneously adds the env-var path (a) then removes it (b) in the same
deliverable — wasted work, plus the window between commits (a) and (b) carries the exact
`#[allow(clippy::panic)]` site TD-005 was filed to prevent. If context runs out after
(a) but before (b), the session ships the violation.

Deeper: the panic site doesn't currently exist in `heartbeat_loop` — TD-005 was filed
against a planned-but-never-landed stub. D5c adds a thing in order to remove it.

**Remediation**: structure D5c as two ordered sub-deliverables with no intermediate commit
carrying the panic site:
- **5c-i** Create `photohelper-test-helpers` (dev-deps only) with
  `force_heartbeat_panic_in_thread` helper. No env-var path ever added to production code.
- **5c-ii** Wire the regression test (D5e path 4) via the helper — subprocess test or
  in-process `is_finished()` predicate; NO `panic!()` macro in `heartbeat_loop`.
The `cfg!(debug_assertions)` env-var approach is dropped entirely; it's the pattern TD-005
was filed against.

---

### PR1-T3 — DN-022 cited 4 times in plan as if it exists; discovery-notes ends at DN-021 (2-way)

**Agents**: com (CRITICAL), gp (HIGH)

`docs/plans/session-03.md:171, 222, 240, 384` reference `DN-022` (real demosaic,
no-DELETE-CASCADE, dup-detection compute). Verified: `docs/discovery-notes.md` ends at
DN-021 (line 174). Zero DN-022 entries. This is the R3-T1 phantom-ID pattern from
session-02 plan-review — the plan author cited a discovery note that hasn't been filed
yet as if it already existed.

**Remediation**: file DN-022 / DN-023 / DN-024 (or consolidate under DN-022 with
sub-bullets) in the plan v2 remediation commit to cover the three scopes. Alternatively,
rephrase each site to "a new DN will be filed at impl time" — but provide the binding
trigger so the DN doesn't get lost. The plan's §Stop-gap declarations should enumerate
all three deferrals explicitly (see PR1-T9).

---

### PR1-T4 — ANL-001 cited as containing "SCUNet plan is in flux" — fabricated cross-reference (2-way)

**Agents**: com (CRITICAL), gp (CRITICAL)

`docs/plans/session-03.md:372-373`: "AI denoise — session 05+ (SCUNet or replacement;
per ANL-001 § out-of-scope, the original SCUNet plan is in flux)."

Verified: `docs/analysis/ANL-001-libraw-cr3-preflight.md:1` title is "LibRaw CR3
pre-flight (EXIF extraction + CVE posture)." ANL-001 contains zero SCUNet content.
This is fabrication-class — the same family as R3-T2 (phantom LibRaw symbol) and
session-01-R2-T7 (ADR-0001 fabricated API surface). Per `CLAUDE.md § No Acceptable
Trade-offs Policy`, ungoverned references in plan artifacts corrupt the audit trail.

**Remediation**: drop the `per ANL-001 § out-of-scope` clause entirely. The SCUNet
deferral stands on its own as a plan statement without citing a non-existent cross-ref.
If the original SCUNet rationale is in the bootstrap plan at
`~/.claude/plans/first-create-a-structure-warm-shell.md`, cite that (or just state
the deferral without a cross-ref).

---

### PR1-T5 — ort Session.run() requires `&mut self`; "one session shared across rayon workers" will not compile (arch, HIGH-compound-to-CRITICAL)

**Agents**: arch (CRITICAL)

`docs/plans/session-03.md:274-276`: "Rayon `par_bridge` for parallel inference; ort
sessions are reusable (Send + Sync), so one session shared across worker threads."

Per the pykeio/ort 2.x docs, `Session::run`, `run_with_options`, `run_binding`, and
`run_async` all take `&mut self`. `Session: Send + Sync` means it's safe to reference
across threads, NOT that `&mut`-receiver methods can be called from multiple workers
without external synchronization. The plan's "one session shared across rayon workers"
is a textbook `Sync` vs `&mut` confusion; it will not compile as a `par_bridge` closure.

Options:
- `Arc<Mutex<Session>>` — serializes inference, defeats parallelism.
- One session per rayon worker thread — N sessions, N× memory.
- `Session::run_async` + ort's internal intra-op threadpool — ort's recommended
  concurrency path.

**Remediation**: §D0 (ANL-002) MUST verify the ort threading semantics with a small
driver: spawn two rayon workers both calling `session.run()` on the same
`Arc<Session>` and record whether it compiles + is deterministic. §D4 must pick one
of the three options explicitly with rationale. "One session shared" cannot land as-is.
Add to D0 §Threading semantics block, with ABORT trigger if shared-session fails to
compile.

---

### PR1-T6 — `Scorer` trait referenced in D4 but never defined anywhere in the plan (2-way)

**Agents**: arch (HIGH, elevated), type (CRITICAL)

`docs/plans/session-03.md:270`: `run_cull(catalog: &Catalog, scorer: &dyn Scorer, opts: CullOpts) -> Result<CullStats>`.
The plan defines `Nima` (D1c) with `score(raw: &RawImage) -> Result<NimaScore>` but
never names the `Scorer` trait — no method signature, no object-safety analysis, no
relationship between `NimaScore` and a generic score type. D4's `&dyn Scorer` requires
an object-safe trait; `NimaScore` is NIMA-specific and can't be a generic return type.
Without the trait specification, D4 does not describe buildable code.

**Remediation**: add a Scorer trait definition to D1c (or a new D1e sub-deliverable):
- Method signature (likely `fn score(&self, raw: &RawImage) -> Result<f64, Error>` or
  a typed output enum);
- The `SLUG: &'static str` associated constant for scorer identity (parallel to
  `KnownCamera::slug()`);
- Object-safety confirmation;
- Which crate owns the trait (`photohelper-ai`);
- `impl Scorer for Nima` wiring.

Alternative (per simp T5): make D4 take a concrete `scorer: &Nima` parameter, defer
the trait to session 04 when ARNIQA lands as the second impl. A concrete type for one
impl is cleaner than a phantom trait.

---

### PR1-T7 — §Stop-gap declarations says "None" but the plan acknowledges ≥3 stop-gaps inline (2-way)

**Agents**: rev (CRITICAL), gp (via§Scope rationale)

`docs/plans/session-03.md:480`: "None known at plan-v1 time." But the plan body
acknowledges at minimum:
1. **D1c bilinear demosaic** (line 170-171): "minimal bilinear demosaic if not — DN-022
   will track the 'real demosaic' scope" — a stop-gap with a phantom-DN deferred.
2. **dup_groups table shape without populator** (lines 237-241): table ships with no
   writer; the value is only realized in session 04+.
3. **Per-cull-run audit trail absent from cull_scores** (lines 248-249): decision-doc
   0002 will note this as out-of-scope for v3+, without a filed TD.

Per `CLAUDE.md § No Acceptable Trade-offs Policy`: "every stop-gap commit MUST file a
TD entry … A deferral without a plan is a CRITICAL finding." §Stop-gap declarations
claiming "None" while the plan body acknowledges three is a policy violation.

**Remediation**: file TD entries for the three named stop-gaps IN the plan v2 commit
(not "at impl time"): `TD-NNN — Bilinear demosaic stop-gap`, `TD-NNN — dup_groups
ships unpopulated`, `TD-NNN — per-cull-run audit trail absent`. Each must carry the
5-field contract (location + fundamental fix + binding trigger + LoC/risk estimate +
consequence of inaction). Rewrite §Stop-gap declarations to enumerate all three.

---

### PR1-T8 — D2b "manual SQLite REPL inspection" is not a test; blocks merge per global testing standards (2-way)

**Agents**: test (CRITICAL), rev (HIGH)

`docs/plans/session-03.md:407` (End-to-end column for D2b): "manual SQLite REPL
inspection." Per `~/.claude/CLAUDE.md § Testing Standards § Code Review Policy`:
"BLOCK merge if tests don't verify actual behavior." A manual REPL inspection:
- Has no assertion CI can run;
- Does not fire on regression;
- Is exactly the R2-T6 / R2-T18 pattern this project already flagged as blocking.

**Remediation**: replace with an automated integration test (~40 LoC):
open a v1-catalog fixture (checked in as `tests/fixtures/catalogs/v1.db`), call
`Catalog::open`, query `PRAGMA user_version` (assert = 2), query
`SELECT name FROM sqlite_master WHERE type='table'` (assert `cull_scores` and
`dup_groups` present), assert an existing v1 `photos` row is preserved. This is the
same shape as `cli.rs:51-62` for v1 schema.

---

### PR1-T9 — D0 sequencing chicken-and-egg: model file committed before D0 validates it (2-way)

**Agents**: arch (CRITICAL), rev (CRITICAL)

`docs/plans/session-03.md:100`: "fires AFTER Deliverable 1's first scaffolding commit
(need ort dep wired) AND BEFORE Deliverable 4's cull rewire." But D1d (line 180)
commits `crates/photohelper-ai/models/nima_aesthetic_v1.onnx` via Git LFS. If D0
ABORTS (CVE found, inference fails), the model file is already in LFS history — wasted
work + branch contamination. Session-02's D0 avoided this: it produced a pre-flight
artifact (`ANL-001`) BEFORE any vendored code landed.

Also: D1d pre-names the file (finding T17 below) before D0 has chosen the model.

**Remediation**: restructure ordering as D0 → D1a (ort dep only) → D1b/c →
D1d (model file, after D0 validates ANL-002's chosen model). D0 should commit
`models/manifest.toml` skeleton with SHA-256 + source URL as the pre-flight output;
D1d populates the actual binary. If D0 ABORTs, no model binary was ever committed.

---

### PR1-T10 — `LoadedModel` SHA-256 verification trust boundary is inverted (type lens)

**Agents**: type (CRITICAL)

`docs/plans/session-03.md:159-162`: LoadedModel "fallible constructor verifying the
SHA-256 matches the `models/manifest.toml` declaration." But the bundled bytes from
`include_bytes!` cannot be tampered with at runtime — `build.rs` already verified the
SHA-256 at build time (D1d:184-188). The constructor cannot verify "these ort::Session
bytes came from SHA X" because the session was already constructed from those bytes;
verification must happen BEFORE `ort::Session` construction. Also, `--model-path` lets
users supply an arbitrary ONNX; its SHA-256 is NOT in `manifest.toml`, so the check
either fails (override is unusable) or is bypassed (security hole).

**Remediation**: restructure as a two-phase constructor:
1. `VerifiedModelBytes::from_manifest(name)` — reads `manifest.toml`, reads file,
   checks SHA-256, returns typed-verified bytes. This is the attestation step.
2. `LoadedModel::from_verified(bytes: VerifiedModelBytes)` — creates `ort::Session`,
   trusts the attestation. No re-check needed because the type system prevents
   unverified bytes reaching this constructor.
For `--model-path`: either supply both `--model-path` and `--model-sha256 <hex>` (the
user vouches), or drop `--model-path` from v0.1 scope (safest per PR1-T11-simp's
recommendation).

---

## HIGH

### PR1-T11 — `ort = { version = "=2.0.X", ... }` is not valid Cargo syntax; ort 2.0 is still RC (2-way)

**Agents**: arch (CRITICAL, here reclassified HIGH because D0 will fix), gp (HIGH)

`docs/plans/session-03.md:145`: literal `"=2.0.X"` with placeholder `X`. Per Cargo
SemVer docs, `X` is not a valid version component; `cargo build` rejects it. Compounding:
ort's latest published version as of 2026-05-28 is `2.0.0-rc.12` (release candidate,
not stable). The plan implies a stable 2.0 exists; it does not.

**Remediation**: §D1a dep line uses `ort = { version = "=2.0.0-rc.12" }` (exact RC pin
chosen by D0's ANL-002). ANL-002 must record: (a) we are knowingly on a release
candidate; (b) upgrade trigger to stable 2.0.0 when it ships; (c) a TD entry for this
upgrade path with binding trigger "ort 2.0.0 stable tag exists OR before v0.1 release
tag".

---

### PR1-T12 — cull_scores supersede semantics unspecified; superseded photos scored unnecessarily (arch, type)

**Agents**: arch (CRITICAL reclassified to HIGH), type (HIGH)

`docs/plans/session-03.md:271`: D4 SELECT is `WHERE id NOT IN (SELECT photo_id FROM
cull_scores WHERE scorer = ?1)`. This walks ALL `photos` rows including superseded ones
(where `superseded_at_unix_seconds IS NOT NULL`). NIMA inference on a superseded photo
(old bytes, different `photo_id`) wastes compute and pollutes `cull_scores` with rows
for photos no user will query. The v1 schema's supersede design (decision-doc 0001
§source_path) is explicit about preserving audit rows; the plan doesn't carry that
discipline into cull.

**Remediation**: D4's canonical SELECT adds `AND p.superseded_at_unix_seconds IS NULL`.
Separately: commit to one of:
- `cull_scores` references `(photo_id, scorer)` for current-only rows → scores
  auto-expire on supersede (users can't compare historical scores).
- `cull_scores.superseded_at_unix_seconds` column (parallel to `photos`) → audit trail.
Decision-doc 0002 (D2c) must record this choice.

---

### PR1-T13 — D4 cull per-photo error dispatch unspecified; all `RawDecodeCause` collapses to `errored` silently (sfh)

**Agents**: sfh (CRITICAL → plan-stage HIGH)

`docs/plans/session-03.md:273`: "Per row: `read_raw` → `nima.score` →
`Catalog::insert_cull_score`." On error from either step, behavior is unspecified.
TD-006 binding trigger reads "session 04+ when `decode::read_raw` gets a consumer" —
session 03's cull IS that consumer. The plan must either close TD-006 in lockstep or
file TD-006-cull-extension. Currently neither.

**Remediation**: add to §D4 an explicit dispatch table per error class (parallel to
ingest's per-counter semantics table from session-02):
- `ModelLoadFailed` → ABORT run (no per-photo retry).
- `InferenceFailed { source }` → per-photo WARN + `inference_failed` counter; strict
  fails if `inference_failed > 0`.
- `read_raw` → `Error::RawDecode*` → per-photo WARN + `decode_failed` counter.
File either TD-006-cull (if deferring the full per-cause counter table) or close TD-006
inline with `CullStats` counters per-cause.

---

### PR1-T14 — `--strict cull` semantics: 6 enumerated cases unresolved; "probably no" is not a commit (sfh)

**Agents**: sfh (CRITICAL → plan-stage HIGH)

`docs/plans/session-03.md:279-282` (TBD per plan-review) + §Discovery items 469-472
("probably no" for low aesthetic scores). Six enumerated cases lack disposition:
model-missing, SHA-mismatch, ort-version-mismatch, per-photo decode fail, per-photo
inference fail, `cull_scores` row already exists with corrupt prior score.

**Remediation**: §D4 enumerates ALL six cases with `fail | warn | skip` disposition.
Integration test per case. The R2-T12 precedent (`--strict` was fail-open on no-EXIF
for ingest) must not repeat for cull.

---

### PR1-T15 — Migration `apply_pending` ROLLBACK-of-ROLLBACK silent; partial-failure behavior unspecified (sfh, arch)

**Agents**: sfh (CRITICAL → plan-stage HIGH), arch (HIGH)

`docs/plans/session-03.md:198-204`: `apply_pending` wraps each step in
`BEGIN IMMEDIATE; ...; COMMIT;` but does not address:
- COMMIT fails mid-migration (disk full): catalog stays at user_version 1 with partial
  tables. Next `Catalog::open` re-runs the migration — are `CREATE TABLE IF NOT EXISTS`
  steps idempotent? (They are — but this must be stated explicitly as an invariant.)
- ROLLBACK itself fails (the R2-M8 class): `rusqlite::Error` from Drop's ROLLBACK
  swallowed; catalog in indeterminate state.
- Two concurrent `Catalog::open` calls both find user_version = 1: file-lock serializes
  them, but is a second migration attempt safe? (Should be yes — idempotent — but untested.)

**Remediation**: §D2a commits to: (a) every `Migration::up` MUST be replay-safe
(`CREATE ... IF NOT EXISTS`, no destructive `DROP`); (b) `apply_pending` propagates
ROLLBACK rusqlite errors as `Error::CatalogMigrationRollbackFailed` except
`ErrorCode::ApiMisuse` (= "no active transaction") which is silently ignored —
matching on the extended error CODE not the message string per PR1-T39 below; (c) add a
recovery integration test (half-applied migration → second open succeeds).

---

### PR1-T16 — D1d hardcodes model filename before D0 has chosen the model (2-way)

**Agents**: rev (CRITICAL → plan-stage HIGH), arch (HIGH)

`docs/plans/session-03.md:180`: `crates/photohelper-ai/models/nima_aesthetic_v1.onnx`
named explicitly. But D0's ANL-002 is the gate that chooses the model — §Discovery
items (line 459) explicitly says "plan-review may surface an alternative model with
cleaner provenance." If D0 picks a different model, the pre-named filename is wrong.

**Remediation**: D1d filename should read `<model_chosen_at_D0>.onnx` placeholder
until ANL-002 lands. Acceptance criterion 5 must reference the model name decided in
D0, not a pre-named string.

---

### PR1-T17 — `Catalog::open` schema-version gate logic incoherent after migration runner insertion (arch)

**Agents**: arch (HIGH)

`docs/plans/session-03.md:199-204`: D2a places `apply_pending` AFTER the existing
schema-version gate check. But the existing gate (verified at `catalog.rs:222-252`)
accepts only `user_version == 0` OR `user_version == SCHEMA_VERSION` (currently 1),
returning `Error::CatalogSchemaTooNew` for anything else. A v1 catalog opened by a
v2 binary hits `user_version = 1 < SCHEMA_VERSION = 2` → "too new" path fires before
migrations can run. The plan's "preserved" claim is incorrect.

**Remediation**: §D2a must specify the full state machine:
- `user_version == 0` → run init + all migrations;
- `0 < user_version < SCHEMA_VERSION` → run pending migrations;
- `user_version == SCHEMA_VERSION` → no-op;
- `user_version > SCHEMA_VERSION` → `Error::CatalogSchemaTooNew` (updated error message
  includes both ends: "found version N; this binary supports M; upgrade photohelper").
The gate check must move INSIDE `apply_pending`'s output, or `apply_pending` must run
BEFORE the gate check.

---

### PR1-T18 — D4 lacks photo path resolution spec; `SELECT id` returns blob, not path (arch)

**Agents**: arch (HIGH)

`docs/plans/session-03.md:271-273`: D4 SELECT returns `id` (BLOB PhotoId).
`read_raw(path: &Path)` takes a path. The plan never says how the path is obtained
per row. `photos.source_path` must be in the SELECT. Edge cases: superseded rows
(same path, different id — handled by PR1-T12), hardlinks (same id, multiple paths —
which path to pick?), deleted file (source_path no longer exists at cull time).

**Remediation**: D4 specifies the SELECT:
`SELECT id, source_path FROM photos WHERE superseded_at_unix_seconds IS NULL AND id NOT IN (SELECT photo_id FROM cull_scores WHERE scorer = ?1)`.
Plus: on `read_raw` `Err(NotFound)` — skip + `file_missing` counter; on content-changed
(re-derived PhotoId mismatch) — skip + WARN; these match ingest's handling pattern.

---

### PR1-T19 — Migration framework is one-user abstraction; decision-doc 0001 named this explicitly (2-way)

**Agents**: arch (HIGH), simp (CRITICAL-reclassified-HIGH)

`docs/plans/session-03.md:193-204`: proposes `Migration` trait + `static MIGRATIONS:
&[&dyn Migration]` + `apply_pending` for one migration step (v1→v2).
`docs/decisions/0001-catalog-schema-v1.md:129` verbatim: "A single-statement migration
doesn't justify framework overhead." The plan ships the framework for v1→v2 — a 2
`CREATE TABLE` + 2 `CREATE INDEX` + 1 `PRAGMA user_version` operation.

**Remediation**: replace with a `match` arm extension in `Catalog::open`:
```rust
match user_version {
    0 => { /* existing init */ }
    1 => { apply_v1_to_v2(conn)?; }
    n if n == SCHEMA_VERSION => {}
    _ => Err(CatalogSchemaTooNew { .. })
}
fn apply_v1_to_v2(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction_with_behavior(Immediate)?;
    tx.execute_batch("CREATE TABLE cull_scores ...; CREATE TABLE dup_groups ...; PRAGMA user_version = 2;")?;
    tx.commit()
}
```
File a TD: "promote to `Migration` trait + version-registry when v3 lands AND is
non-trivial (multi-step or data migration)." Saves ~100 LoC from session 03 scope.

---

### PR1-T20 — NIMA preprocessing via bilinear Rust code when LibRaw already does it (arch)

**Agents**: arch (HIGH)

`docs/plans/session-03.md:171-176`: "bilinear demosaic if not [available]…". LibRaw
exposes `libraw_dcraw_process()` + `libraw_dcraw_make_mem_image()` — session 02's FFI
already built the lifecycle bindings (unpack → RAII guard → accessor). Adding 2 FFI
bindings for a clean RGB output is ~100 LoC vs ~300 LoC of Rust bilinear demosaic that
produces lower quality. NIMA was trained on consumer JPEGs from camera-native demosaic;
feeding it bilinear-demosaiced data shifts the score distribution unpredictably.

**Remediation**: D1c re-scoped to add `read_raw_rgb(path: &Path) -> Result<RgbImage>`
to `photohelper-raw` via LibRaw's demosaic pipeline. New FFI bindings:
`libraw_dcraw_process` + `libraw_dcraw_make_mem_image`. The `RgbImage` type
(analogous to existing `RawImage`) exposes `width`, `height`, `pixels_rgb` for NIMA
preprocessing. File a TD: "demosaic algorithm choice (AAHD vs AMaZE vs default AHD) —
v0.1 uses LibRaw default; session 04+ develop pipeline may need explicit selector."

---

### PR1-T21 — SLO "wall-clock < 30 min" is 60× looser than expected; not calibrating anything (arch)

**Agents**: arch (HIGH)

`docs/plans/session-03.md:437`: Acceptance criterion 3 "wall-clock < 30 min on
apple-silicon." Rough math: 370 photos × ~600ms/photo (decode ~500ms + infer ~50ms +
downsample ~30ms) = 222s sequential ≈ 4 min; with 8-core rayon ≈ 30s. A "<30 min" SLO
would pass even if every photo took 5 seconds — not a useful gate.

**Remediation**: D0 measures actual per-photo wall-clock on the two CC0 fixtures.
Acceptance criterion 3 is rewritten as `wall-clock < (D0_measured × 370 / num_cpus)
× 1.5` (50% headroom). OR delete criterion 3 and replace with "D0 records per-photo
wall-clock in ANL-002; session-end commit message records actual 370-photo wall-clock."

---

### PR1-T22 — `PRAGMA foreign_keys = ON` absent from Catalog::open; v2 FK constraints are decorative (2-way)

**Agents**: test (HIGH), type (HIGH)

Verified at `crates/photohelper-catalog/src/catalog.rs:204-212`: the PRAGMA loop
contains `journal_mode = WAL`, `synchronous = NORMAL`, `busy_timeout = 5000` — no
`foreign_keys = ON`. SQLite FKs are OFF by default. The plan's D2b FK
`cull_scores.photo_id REFERENCES photos(id)` will silently allow orphan inserts.

**Remediation**: add `"PRAGMA foreign_keys = ON"` to the Step 7 PRAGMA loop at
`catalog.rs:204`. This covers both the new `cull_scores` and `dup_groups` FKs.
Add a regression test: attempt `Catalog::insert_cull_score` with a non-existent
`photo_id` and assert the FK violation error is returned (not silently ignored).

---

### PR1-T23 — D5b cites `catalog.rs:281` for R2-M8 silent-ROLLBACK; actual location is `:304` (com)

**Agents**: com (HIGH)

`docs/plans/session-03.md:297`: "5b R2-M8 silent-ROLLBACK fix at `catalog.rs:281`."
Verified: `catalog.rs:281` is `Ok(Self { conn: Mutex::new(conn), … })` — the Catalog
constructor return. The `let _ = conn.execute("ROLLBACK", [])` site is at line 304.

Also: D5b says match on the error MESSAGE STRING `"cannot rollback - no transaction is
active"` — fragile across rusqlite version bumps. Should match on
`rusqlite::ErrorCode::ApiMisuse` (extended code 21) per session-02 R2-M8 spec.

**Remediation**: correct to `catalog.rs:304`. Specify the match as:
```rust
Err(rusqlite::Error::SqliteFailure(e, _))
    if e.code == rusqlite::ErrorCode::ApiMisuse => {} // no active transaction — ignore
Err(e) => return Err(Error::CatalogTransaction { op: "rollback-after-worker-panic", source: Box::new(e) }),
```

---

### PR1-T24 — TD-006 and TD-007 binding-trigger status vs. session 03 scope unaddressed (com, gp)

**Agents**: com (HIGH), gp (MEDIUM-escalated)

TD-006 binding trigger (`TECH-DEBT.md:103`): "session 04+ when `decode::read_raw` gets
a consumer (the develop pipeline)." Session 03 D4 IS the consumer. The plan's §Out of
scope does not acknowledge TD-006's trigger firing.

TD-007 binding trigger: "the next session touching `photohelper-raw/src/decode.rs`".
Session 03 D1c consumes `RawImage` types from `decode.rs`. Whether that constitutes
"touching" is debatable; the plan should acknowledge the status explicitly.

**Remediation**: add explicit §Out of scope rows:
- "TD-006 dispatch surface: cull adds the first consumer of `decode::read_raw` (per
  PR1-T13 above). Either close TD-006 in lockstep with `CullStats` per-cause counters OR
  file `TD-NNN — cull error dispatch` with binding trigger 'session that adds second
  scorer' and update TD-006's trigger to 'has been fired by session 03 cull consumer;
  closed or re-triggered'."
- "TD-007 empty-path PathBuf: session 03 D1c USES `decode.rs` types but adds no new
  constructors; TD-007's 'next session touching decode.rs' trigger does NOT fire here.
  Calendar trigger 2026-08-01 remains operative."

---

### PR1-T25 — TD-011 3-session bound silent; session 03 is 1 of 3 (com)

**Agents**: com (HIGH)

TD-011 binding trigger: "before the first GitHub Release tag is cut OR within the next
3 sessions" (from session 02 filing). Session 03 is session 1 of 3. The plan's §Out of
scope row for TD-011 says only "separate review-only session before v0.1 release tag"
— which is the first clause but silently ignores the 3-session bound. If sessions 04
and 05 also defer, the trigger fires regardless and creates a CRITICAL finding at
session-05 plan-review.

**Remediation**: revise §Out of scope row to: "TD-011 — deferred to session 05 AT
LATEST (TD-011's 3-session bound from session 02 = sessions 03/04/05; if not closed by
session-05 session-end, escalate to CRITICAL)."

---

### PR1-T26 — NIMA golden-vector test: no tolerance, no generation procedure, no cross-platform policy (test)

**Agents**: test (HIGH)

`docs/plans/session-03.md:404` promises E2E "score matches the golden fixture vector."
Gaps: (a) ort CPU inference is deterministic per binary but NOT across compiler/arch
(apple-silicon vs Linux x86_64 may differ by ~1e-3). No tolerance specified. (b) No
`just nima-regenerate-golden` recipe — a failing test has no authoritative recovery
path. (c) Cross-platform CI: golden generated on apple-silicon, checked on Linux x86_64,
may flake.

**Remediation**: amend D1c E2E: "score is within ±1e-3 of golden (CPU-deterministic
tolerance; platform of record: apple-silicon). `just nima-regenerate-golden` recipe
re-runs inference, overwrites the binary fixture, prints the new value. Linux x86_64 CI
uses a band assertion `score ∈ [3.0, 9.0]`." File DN-NNN: "NIMA cross-platform
tolerance — v0.1 uses band on Linux; future: align with model card's expected
distribution."

---

### PR1-T27 — `ModelRegistry` trait with one impl + `--model-path` power-user flag: both premature abstractions (2-way)

**Agents**: arch (HIGH), simp (HIGH)

`ModelRegistry` trait (`docs/plans/session-03.md:150-156`) has one concrete impl
`BundledModelRegistry`. Per CLAUDE.md § Doing tasks: "Don't design for hypothetical
future requirements." The trait adds vtable overhead + maintenance surface for zero
current benefit.

`--model-path` CLI flag (lines 155, 285) expands the test matrix (bundled vs override
× SHA-check × missing-file) without any user request, and bypasses the SHA verification
in ways the plan doesn't resolve (see PR1-T10).

**Remediation**: (a) drop the trait; ship `pub struct ModelRegistry` concrete type with
a `#[doc(hidden)] fn with_test_model_dir(path: PathBuf) -> Self` constructor for
tests — same pattern as `Catalog::open_with_retry_delay`. (b) Drop `--model-path` from
v0.1; add to §Out of scope with a TD.

---

### PR1-T28 — D3 sanitize-check extension on ONNX via exiftool is technically wrong (2-way)

**Agents**: arch (MEDIUM), rev (MEDIUM-compound-HIGH in context)

`docs/plans/session-03.md:261-264`: "Extend `scripts/sanitize-check.sh` to scan the
new NIMA model file for any embedded EXIF / GPS / surprising metadata." ONNX files are
Protobuf-encoded; they contain no EXIF/GPS fields that `exiftool` knows. What ONNX
files DO carry: `producer_name`, `doc_string`, `metadata_props` key-value strings which
may leak training-environment absolute paths or internal identifiers. `exiftool` will
report nothing useful on ONNX.

**Remediation**: D3 sanitize-check extension specifies an ONNX-aware check:
`python3 -c "import onnx; m = onnx.load('...'); print(m.producer_name, m.doc_string,
[p for p in m.metadata_props])"` (or a Rust `prost`-based reader). Allow-list:
`producer_name` = known frameworks (PyTorch, ONNX exporter); reject absolute paths in
`doc_string` or `metadata_props`. Update `scripts/sanitize-check.sh` accordingly.

---

## MEDIUM

### PR1-T29 — DN-008 count drift: plan says "6 rows", TD-010 says "6", DN-008 says "12" (com)

**Agents**: com (HIGH)

DN-008 binding trigger (`docs/discovery-notes.md:93`): lists 12 rows
`{6, 12, 13, 14, 17, 18, 19, 34, 39, 42, 43, 49}`. TD-010 §6d narrowed to 6 rows
without explaining the 6-of-12 selection. Plan §D5d inherits TD-010's 6 without
acknowledging the discrepancy. Three documents disagree; the plan labels them "DN-008 6
rows" implying DN-008 endorses 6.

**Remediation**: relabel to "TD-010's 6-of-12-row subset of DN-008 (rows 12, 13, 14,
18, 19, 34 deferred)." File a companion TD entry for the deferred 6 rows with a binding
trigger. Amend DN-008 to reflect that TD-010 narrowed the owner.

---

### PR1-T30 — dup_groups embedding BLOB under-specified; ships forward-declared with zero consumer (arch, type)

**Agents**: arch (HIGH), type (MEDIUM)

`docs/plans/session-03.md:228-234`: `embedding BLOB NOT NULL` with no dimension spec,
no float-format, no model-identity column. MobileCLIP-S0 is 256-d float32 (1024 bytes);
MobileCLIP-L is 768-d. A future consumer cannot know what dimension the stored BLOBs
carry. Also: `UNIQUE(photo_id)` forecloses hierarchical/per-region embeddings.

**Remediation**: either (a) drop `dup_groups` from the v2 schema entirely — ship only
`cull_scores` in v2, add `dup_embeddings` when the MobileCLIP consumer arrives and can
validate the shape — or (b) add `model_slug TEXT NOT NULL`, `dim INTEGER NOT NULL`,
`quantization TEXT NOT NULL` columns and rename to `dup_embeddings` (per-photo
embeddings, not per-group), deferring `dup_clusters` (group assignment) to the same
session that adds the compute. Option (a) is preferred per PR1-T19's "one migration,
one value" principle.

---

### PR1-T31 — NimaScore invariant set: equality/Copy decision missing; round-trip via SQLite REAL not addressed (type)

**Agents**: type (HIGH)

`docs/plans/session-03.md:166-168`: NimaScore rejects NaN/±∞/out-of-range but doesn't
address: (a) `Copy` (4 bytes, no destructor — yes, should be `Copy`); (b) `PartialEq`
vs `Eq + Hash` (NaN rejected so `Eq` is sound, but quantization for HashMap keys is
not decided); (c) REAL → f64 → f32 round-trip may push boundary values (1.0 stored as
0.9999... rounds outside the invariant on read-back). A `from_catalog_f64` saturating
constructor covers (c).

**Remediation**: add to D1c: "NimaScore: Copy + Clone + Debug + PartialOrd (not Eq —
f32 equality is floating-point, not natural equality). `NimaScore::from_catalog_f64(f64)
-> Result<Self>` is a separate constructor that saturates to [1.0, 10.0] for
rounding-error tolerance (analogous to `clamp_mtime` at `model.rs:206`)."

---

### PR1-T32 — `Migration` trait `up(&self, tx: &Transaction)` exposes commit/rollback surface (type)

**Agents**: type (HIGH reclassified MEDIUM given PR1-T19 recommends dropping the trait)

If the trait is kept (contra PR1-T19's recommendation), `&Transaction` exposes
`commit()`, `rollback()`, `set_drop_behavior()` to implementors. A Migration impl can
call `tx.commit()` mid-step, defeating per-step atomicity. Should be sealed or narrowed
to an `MigrationExecutor` that exposes only `execute(&str, params)`.

**Remediation**: if trait kept: seal it + expose only `fn execute_batch(&str)` to
impls. If trait dropped per PR1-T19: moot.

---

### PR1-T33 — Heartbeat refactor: plan defers to plan-review preference without a default; decide now (2-way)

**Agents**: arch (HIGH), simp (HIGH)

`docs/plans/session-03.md:473-476`: heartbeat factoring is an open question with no
default. D4 requires a decision.

**Decision for plan v2**: duplicate `HeartbeatStop` + `heartbeat_loop` scaffolding into
`cull.rs` for v0.1 (two consumers is not the threshold for refactoring — three is);
file a TD: "factor heartbeat into `photohelper-cli::heartbeat` when the third
subcommand adds a heartbeat; trigger: session that adds a heartbeat to `develop`,
`export`, or `run`."

---

### PR1-T34 — D5c E2E "objdump | grep panic" is not CI-runnable; broken on macOS strip=symbols (2-way)

**Agents**: rev (HIGH), test (HIGH)

`docs/plans/session-03.md:412`: E2E column for D5c says "`cargo build --release &&
objdump | grep panic` finds no heartbeat-related panic." Three problems: (a) `objdump`
is GNU-only; macOS build uses `otool`; (b) `strip = "symbols"` (`Cargo.toml:96`) removes
all symbols from the release binary on darwin — grep returns nothing, test vacuously
passes; (c) `core::panicking` symbols appear in every binary regardless.

**Remediation**: per PR1-T2's D5c restructuring, once the panic site never lands in
`heartbeat_loop`, this test reduces to "verify `photohelper-test-helpers` is
`[dev-dependencies]` only, not `[dependencies]`" — a `cargo metadata` grep. No
`objdump` needed.

---

### PR1-T35 — D6 message points at SESSION-STATE.md — not visible to release-binary users (sfh)

**Agents**: sfh (HIGH)

`docs/plans/session-03.md:337-339`: new stub message "see SESSION-STATE.md for the
current roadmap." `SESSION-STATE.md` is not a published artifact; users running a
downloaded release binary have no access to it.

**Remediation**: new message points at a public artifact:
`"photohelper {name}: not yet implemented in v0.1 (ingest + cull only); see README.md for the current scope."` Also add a README § Roadmap section in D7.

---

### PR1-T36 — DN-005 still names session 02 as migration-framework owner; plan closes DN-005 without updating it (com)

**Agents**: com (HIGH)

`docs/discovery-notes.md:56` (drifted from cited line 52): "session 02 (full schema once
`cull` adds dup-group and culling-score tables)" — stale; decision-doc 0001 § Amendments
explicitly moved this to session 03. The plan closes DN-005 (Acceptance criterion 7) but
doesn't propose amending DN-005's status or owner text.

**Remediation**: plan v2 remediation must include an amend to DN-005 (append-only
update line: "session 03 owns the v1→v2 migration + cull_scores + dup_groups tables per
decision-doc 0001 § Amendments 2026-05-28; session 02 owner crossed out").

---

### PR1-T37 — Cull heartbeat-death WARN test must cover BOTH ingest AND cull drivers (sfh)

**Agents**: sfh (CRITICAL → plan-stage MEDIUM, addressable in plan v2)

D5e row 4 promises heartbeat-death WARN test but is written against a single driver.
D4 spawns cull's own heartbeat thread — if the cull copy wires `is_finished()` check
in the wrong order, the WARN never fires and no test catches it.

**Remediation**: D5e row 4 explicitly tests BOTH drivers: "heartbeat-death WARN test
parameterized over `[ingest, cull]` drivers." The test shape: trigger heartbeat death
via D5c's test-helper, assert WARN substring fires, assert summary line still prints.

---

## LOW

### PR1-T38 — "the user's 370 CR3 set" (Acceptance criterion 3): SESSION-STATE says 371 walked (com)

**Agents**: com (LOW)

`docs/plans/session-03.md:435`: "the user's 370 CR3 set." SESSION-STATE.md:42: "walked:
371, ingested: 370." The directory has 371 entries (370 CR3 + 1 `.photohelper` dir).

**Remediation**: "the user's 371-entry test directory (370 CR3 + 1 .photohelper catalog
dir, per session-02 production trace)."

---

### PR1-T39 — ai-culling feature gate default-on makes the gate decorative (rev, arch LOW)

**Agents**: arch (LOW), rev (MEDIUM)

`docs/plans/session-03.md:135`: `default = ["ai-culling"]`. If the feature is
always-on with no opt-out consumer, the gate is documentation, not enforcement.

**Remediation**: document the gate's purpose explicitly: "downstream crates wanting to
skip ort linkage can set `default-features = false`; no v0.1 downstream consumer exists
yet." Or drop the feature gate entirely and make ort a hard dep — simpler.

---

### PR1-T40 — ADR-0003 in D7 doubles DN-003; one-sentence defer doesn't earn ADR-level audit (simp)

**Agents**: simp (MEDIUM)

`docs/plans/session-03.md:359-362`: ADR-0003 "ONNX Runtime in-process vs subprocess"
for a one-sentence "defer subprocess to v0.5 reassessment." DN-003 already captures the
question. An ADR is for binding architectural decisions (CLAUDE.md: "When a decision is
binding"); a deferral that carries no implementation commitment is not.

**Remediation**: close DN-003 with an append-only addendum (citing ANL-002's inference
decision + v0.5 reassessment trigger with a concrete mechanism — e.g., "5+ GitHub
issues tagged `crash:ort-inference` OR 2026-12-01, whichever first"). Drop ADR-0003
from D7. Decision-doc 0002 (D2c) covers the schema decisions; no separate ADR needed.

---

### PR1-T41 — R3-T3 misattributed; plan merges R3-T3 finding with R3-T3's option (c) remedy (com)

**Agents**: com (MEDIUM)

`docs/plans/session-03.md:302`: "per R3-T3." R3-T3 was the bug report (heartbeat panic
site violates workspace lint); option (c) was one of three remediation alternatives the
agent offered. The plan adopts option (c) but attributes it as if R3-T3 prescribed it.

**Remediation**: change to "per R3-T3 remediation option (c)."

---

### PR1-T42 — Empty §Plan revisions log section at v1 is noise; add when v2 lands (simp)

**Agents**: simp (LOW)

`docs/plans/session-03.md:484-489`: section exists with one bullet (v1 itself) at plan
v1. Reader convention implies a log of changes; at v1 there are none.

**Remediation**: delete the section; add it in plan v2 with the v1→v2 entry as first
bullet.

---

## Disposition summary

| Disposition | Count | Action |
|-------------|------:|--------|
| **Fix in plan v2 remediation** | 10 CRITICAL + 15 HIGH | All code-bearing + design fixes |
| **File TD/DN with binding trigger; remediate later** | 3 MEDIUM (T29, T30 partial, T33) | Alongside v2 remediation commit |
| **Accept with explicit documentation** | 5 LOW (T38-T42) | When convenient; most are one-line edits |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 34
  verified: 26
  drifted: 8
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    All 34 CRITICAL+HIGH findings verified by 9th-agent. Eight findings are
    "drifted" (line numbers off by 1-5 lines due to plan reformatting); all
    retain=yes-with-corrected-line. Zero hallucinations. MEDIUM/LOW findings
    carry direct file:line citations from the original agent reports and can be
    spot-checked at remediation. Orchestrator post-hoc substring-grep confirmed
    all evidence_snippets substring-match their cited files; zero compromised.
  details:
    - {finding_id: T1,   file: crates/photohelper-cli/src/commands/mod.rs,  line: 1,   present: yes,     retain: yes,                      evidence_snippet: "pub mod ingest;"}
    - {finding_id: T1b,  file: crates/photohelper-cli/src/main.rs,          line: 118, present: yes,     retain: yes,                      evidence_snippet: "Command::Cull => stub(\"cull\", \"session 03\")"}
    - {finding_id: T1c,  file: crates/photohelper-cli/src/main.rs,          line: 127, present: yes,     retain: yes,                      evidence_snippet: "fn stub(name: &str, planned_in: &str) -> ExitCode"}
    - {finding_id: T2,   file: docs/plans/session-03.md,                    line: 171, present: yes,     retain: yes,                      evidence_snippet: "DN-022 will track the \"real demosaic\""}
    - {finding_id: T2b,  file: docs/discovery-notes.md,                     line: 174, present: yes,     retain: yes,                      evidence_snippet: "### DN-021 — Two-shell PATH drift"}
    - {finding_id: T3,   file: docs/plans/session-03.md,                    line: 372, present: yes,     retain: yes,                      evidence_snippet: "per ANL-001 § out-of-scope, the original SCUNet plan is in flux"}
    - {finding_id: T3b,  file: docs/analysis/ANL-001-libraw-cr3-preflight.md, line: 1, present: yes,    retain: yes,                      evidence_snippet: "# ANL-001 — LibRaw CR3 pre-flight"}
    - {finding_id: T4,   file: docs/plans/session-03.md,                    line: 300, present: yes,     retain: yes,                      evidence_snippet: "AND retires TD-005 by adding the proper fix in lockstep"}
    - {finding_id: T4b,  file: TECH-DEBT.md,                                line: 89,  present: yes,     retain: yes,                      evidence_snippet: "production heartbeat_loop becomes panic-free"}
    - {finding_id: T5,   file: docs/plans/session-03.md,                    line: 274, present: yes,     retain: yes,                      evidence_snippet: "ort sessions are reusable (Send + Sync), so one session shared across worker threads"}
    - {finding_id: T6,   file: docs/plans/session-03.md,                    line: 145, present: yes,     retain: yes,                      evidence_snippet: "ort = { version = \"=2.0.X\", default-features = false, features = [...] }"}
    - {finding_id: T7,   file: docs/plans/session-03.md,                    line: 180, present: yes,     retain: yes,                      evidence_snippet: "crates/photohelper-ai/models/nima_aesthetic_v1.onnx"}
    - {finding_id: T7b,  file: docs/plans/session-03.md,                    line: 100, present: yes,     retain: yes,                      evidence_snippet: "fires AFTER Deliverable 1's first scaffolding commit"}
    - {finding_id: T8,   file: docs/decisions/0001-catalog-schema-v1.md,    line: 129, present: yes,     retain: yes,                      evidence_snippet: "A single-statement migration doesn't justify framework overhead"}
    - {finding_id: T9,   file: TECH-DEBT.md,                                line: 103, present: yes,     retain: yes,                      evidence_snippet: "session 04+ when decode::read_raw gets a consumer (the develop pipeline)"}
    - {finding_id: T10,  file: docs/plans/session-03.md,                    line: 279, present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "--strict predicate extension (TBD per plan-review): cull"}
    - {finding_id: T11,  file: docs/plans/session-03.md,                    line: 273, present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "Per row: read_raw → nima.score → Catalog::insert_cull_score"}
    - {finding_id: T13,  file: docs/plans/session-03.md,                    line: 198, present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "BEGIN IMMEDIATE; ...; COMMIT; per per-step idempotency"}
    - {finding_id: T15,  file: docs/plans/session-03.md,                    line: 407, present: yes,     retain: yes,                      evidence_snippet: "manual SQLite REPL inspection"}
    - {finding_id: T16,  file: docs/plans/session-03.md,                    line: 480, present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "None known at plan-v1 time"}
    - {finding_id: T17,  file: docs/plans/session-03.md,                    line: 159, present: yes,     retain: yes,                      evidence_snippet: "fallible constructor verifying the SHA-256 matches the models/manifest.toml declaration"}
    - {finding_id: T18,  file: docs/plans/session-03.md,                    line: 270, present: yes,     retain: yes,                      evidence_snippet: "run_cull(catalog: &Catalog, scorer: &dyn Scorer"}
    - {finding_id: T20,  file: crates/photohelper-catalog/src/catalog.rs,   line: 281, present: yes,     retain: yes,                      evidence_snippet: "Ok(Self { conn: Mutex::new(conn)"}
    - {finding_id: T20b, file: crates/photohelper-catalog/src/catalog.rs,   line: 304, present: yes,     retain: yes,                      evidence_snippet: "let _ = conn.execute(\"ROLLBACK\", [])"}
    - {finding_id: T25,  file: crates/photohelper-catalog/src/catalog.rs,   line: 204, present: yes,     retain: yes,                      evidence_snippet: "PRAGMA journal_mode = WAL"}
    - {finding_id: T27,  file: docs/plans/session-03.md,                    line: 437, present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "wall-clock < 30 min on apple-silicon"}
    - {finding_id: T29,  file: docs/discovery-notes.md,                     line: 56,  present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "session 02 (full schema once cull adds dup-group and culling-score tables)"}
    - {finding_id: T30,  file: docs/discovery-notes.md,                     line: 93,  present: yes,     retain: yes,                      evidence_snippet: "session 02 lands poison_for_testing + tests {6, 12, 13, 14, 17, 18, 19, 34, 39, 42, 43, 49} (12 rows"}
    - {finding_id: T35,  file: docs/plans/session-03.md,                    line: 212, present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "scorer TEXT NOT NULL,           -- 'nima-aesthetic-v1' for now"}
    - {finding_id: T38,  file: docs/code-reviews/session-01-round2.md,      line: 313, present: yes,     retain: yes,                      evidence_snippet: "R2-T21 — Photo::from_filesystem accepts unverified"}
    - {finding_id: T43,  file: docs/plans/session-03.md,                    line: 337, present: drifted, retain: yes-with-corrected-line,   evidence_snippet: "see SESSION-STATE.md for the current roadmap"}
    - {finding_id: T49,  file: docs/plans/session-03.md,                    line: 412, present: yes,     retain: yes,                      evidence_snippet: "cargo build --release && objdump | grep panic"}
    - {finding_id: T52,  file: docs/plans/session-03.md,                    line: 473, present: yes,     retain: yes,                      evidence_snippet: "Refactor may be in-scope or deferred per plan-review preference"}
    - {finding_id: T64,  file: docs/plans/session-03.md,                    line: 435, present: yes,     retain: yes,                      evidence_snippet: "the user's 370 CR3 set"}
```

---

## R2 watch-list (must verify in Round 2)

1. PR1-T1 remediation: D6 now edits `main.rs:127-130` correctly; stub count = 5 (not
   `cull`); message points at README not SESSION-STATE.md.
2. PR1-T2 remediation: D5c now shows only 5c-i + 5c-ii; no panic site in
   `heartbeat_loop`; `cfg!(debug_assertions)` block absent.
3. PR1-T3 / PR1-T4: DN-022/023/024 now exist OR phantom cites removed; ANL-001 citation
   removed.
4. PR1-T5: D4 concurrency model explicitly picks one of three options (Mutex / per-worker
   / run_async).
5. PR1-T6: Scorer trait defined in D1c/D1e OR D4 takes concrete `&Nima`.
6. PR1-T7: §Stop-gap declarations now enumerates ≥3 items with TD entries filed.
7. PR1-T8: D2b E2E column is an automated integration test, not "manual REPL inspection."
8. PR1-T9: D0 sequencing is D0 → D1a → D1b/c → D1d; model binary committed after D0.
9. PR1-T17: schema-version gate state machine fully specified in D2a.
10. PR1-T19: migration framework replaced with `match` arm + `apply_v1_to_v2()` function;
    or plan justifies the framework with a new argument not refuted by PR1-T19.
