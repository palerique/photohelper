# session-01 session-end Round 1 (code review)

> **Post-R2 reconciliation note (2026-05-28)** — R2 review
> (`docs/code-reviews/session-01-round2.md`) surfaced two count drifts
> in this artifact (R2-T22 + R2-T23) that the R1 author missed:
>
> 1. **Theme totals (R2-T23)**: the §Summary table reads "7C + 5H + 4M
>    + 3L = 19" but counting themes by their bracketed `### T*`
>    severity tag yields 7 CRITICAL (T1-T7) + 4 HIGH (T8-T11) +
>    3 MEDIUM (T12, T13, T14) + 1 LOW (T15) = 15. The "19" number
>    appears to count per-agent severity flags inside themes (e.g.,
>    T15's umbrella [LOW] contains 2 HIGH agent-flags + 1 LOW).
>    Canonical convention going forward (recorded in this note rather
>    than rewritten retroactively in the artifact): **count themes,
>    not agent-flags.**
> 2. **Uncovered-plan-row count (R2-T22)**: T7 title says "12 plan
>    rows uncovered"; T7 body enumerates 13 rows (`{6, 12, 13, 14, 17,
>    18, 19, 34, 39, 42, 43, 48, 49}`); DN-008 originally listed 11
>    entries omitting rows 17 + 48; SESSION-STATE said 12. The
>    canonical post-R2 row list — after R2-T6's deterministic
>    heartbeat-test closes row 48 — is **12 rows: `{6, 12, 13, 14, 17,
>    18, 19, 34, 39, 42, 43, 49}`**, now reconciled in DN-008 +
>    SESSION-STATE + `docs/plans/session-01.md § Post-R1 / Post-R2
>    amendments`.
>
> This artifact is preserved as-is for the historical record; the R2
> artifact + the plan-amendments table are authoritative for current
> state.

---


> Per `docs/quality-assurance.md § Session-end protocol`. Cadence A → Tier 5
> (session end), full 8-agent suite fired in parallel against the
> implementation commit `310f753` (4580 lines added across 8 crates + 16
> integration tests).
>
> Findings grouped by **theme** (not by agent) per
> `docs/quality-assurance.md § Consolidation discipline`. When multiple
> agents flagged the same theme, agents cited in brackets.

## Summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 7 | Real bugs + plan-vs-code drift + missing deliverables. Block session-end. |
| **HIGH** | 5 | Must address before R2 sweep. |
| **MEDIUM** | 4 | Polish + small refactors; partly defer to TECH-DEBT only with a binding trigger. |
| **LOW** | 3 | Hygiene. |
| **NOTES** (strengths preserved) | 7 | Confirm in R2. |

Agent suite: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer`
(type), `pr-review-toolkit:silent-failure-hunter` (sfh),
`pr-review-toolkit:comment-analyzer` (com),
`pr-review-toolkit:pr-test-analyzer` (test),
`pr-review-toolkit:code-simplifier` (simp).

---

## Findings (by theme)

### T1 — `no_exif` counter is dead code; `IngestOutcome::NoExifFields` never produced [CRITICAL]

**Agents**: gp (CRITICAL), sfh (CRITICAL)

`crates/photohelper-cli/src/commands/ingest.rs:273-278`: when
`exif.is_empty()` is true, the code only logs a WARN and falls through to
`Photo::from_filesystem` + `catalog.upsert`. The variant
`IngestOutcome::NoExifFields` is **never returned**, so the
`apply_outcome` arm at line 208 is unreachable, and the `no_exif`
`AtomicU64` counter at line 209 is permanently dead.

Consequence: the §Observability summary line always reads `no-exif: 0`
even when DN-006 fallback fires (which is exactly the condition this
counter was created to surface). The smoke test on `/tmp/ph_demo` proves
the bug — empty-EXIF CR3s ingest with `no-exif: 0`.

**Remediation (R2)**: bump `stats.no_exif` at the WARN point so the
counter reflects the reality observable in the WARN logs, even though
the insert still proceeds with NULL EXIF columns (per the §Observability
contract "Insert proceeds with NULLs" comment). Alternative: early-return
`IngestOutcome::NoExifFields` and skip the insert — but plan v5 says
"insert proceeds," so the counter-only fix is correct.

---

### T2 — Heartbeat thread bug cluster [CRITICAL]

**Agents**: sfh (CRITICAL), arch (MEDIUM), com (HIGH), simp (MEDIUM)

Three interacting bugs in the heartbeat infrastructure
(`crates/photohelper-cli/src/commands/ingest.rs:108-151, 172-189`):

1. **Dead `if !heartbeat_handle.is_finished()` block** (lines 148-151):
   body is comment-only — no sleep, no warn, no join. Comment promises
   "Give it one tick to notice and exit" — code does nothing. Comment lies.
2. **Per plan v5 §Concurrency** "Driver `is_finished()` check + WARN if
   the heartbeat died before the walk" — the WARN never fires. If
   `heartbeat_loop` panics (e.g., `eprintln!` on closed stderr), the run
   continues with zero liveness signal AND no diagnostic.
3. **Heartbeat thread leaks past `run_ingest`**: up to 100ms of zombie
   heartbeat output continues on stderr after `summary_line` prints
   (granularity 100ms loop). Tests asserting exact stderr ordering can
   flake. In-process test runs accumulate one leaked thread per
   `run_ingest` call until process exit.

Also: granularity-counter pattern (100ms loop + tick counter) is
over-engineered for a fire-and-forget heartbeat. Simpler shape: `loop {
thread::sleep(HEARTBEAT_INTERVAL); if stop.load() { break; } eprintln!(...); }`.

**Remediation (R2)**: (a) drop the empty `if` block; (b) actually
implement the death-WARN by checking `is_finished()` BEFORE setting the
stop flag and emitting `tracing::warn!("heartbeat thread died early")`
if true; (c) join the heartbeat handle with `HEARTBEAT_INTERVAL +
50ms` timeout so the stderr ordering is deterministic. Item (c) is
optional — accept the leak with an explicit comment if you don't want
the latency.

---

### T3 — `PhotoId` hash window double-counts overlap for files 64KB–128KB [CRITICAL]

**Agents**: arch (CRITICAL)

`crates/photohelper-core/src/model.rs:85-127` `derive_with_clamped_mtime`:
for a 100KB file, `head_len = 65536` reads bytes `[0..65536)`, `tail_len
= 65536` seeks to `End(-65536)` → absolute offset `36864`, reads bytes
`[36864..102400)`. **The overlap region `[36864..65536)` — 28,672 bytes
— is hashed twice.** For a file exactly 128KB the head and tail are
perfectly adjacent (no overlap and no gap); below 128KB overlap; above
128KB a gap.

PhotoId remains unique (file_size prefix distinguishes same-content-window
files per test at model.rs:712-729), but the hash incorporates duplicate
data, violating the spec's "first 64KB + last 64KB" intent. The
collision space is still safe by entropy — but the docstring lies about
the bytes that actually contribute to the hash.

**Remediation (R2)**: compute `tail_start_offset =
max(file_size - HASH_WINDOW_BYTES, head_len)` so head and tail are
disjoint. For 100KB: `max(36864, 65536) = 65536` → tail reads
`[65536..100KB)` with no overlap. Update the §PhotoId derivation docstring
to reflect the disjoint-window invariant. Add a unit test that asserts
two 100KB files differing ONLY in bytes `[40000..50000)` produce
different PhotoIds (currently they MIGHT collide if the differing bytes
fall entirely in the double-hashed overlap window — though BLAKE3's
strength makes practical collision negligible, the test pins the
invariant).

---

### T4 — MSRV bump 1.85 → 1.88 ships with zero ADR and 3 stale governance files [CRITICAL]

**Agents**: gp (CRITICAL), com (LOW carry-forward)

`rust-toolchain.toml:2` and `Cargo.toml:17` declare `1.88`. `CLAUDE.md §
Quality gates` (line 83) is the binding contract: "don't bump without a
`docs/adr/` entry." `docs/adr/` contains only `.gitkeep`.

Three governance files still claim 1.85.0:
- `CLAUDE.md:83` ("Toolchain pin: `rust-toolchain.toml` (channel `1.85.0`)")
- `HANDOFF_REPORT.md:31` ("Toolchain pinned in `rust-toolchain.toml` (channel `1.85.0`)")
- `stacks/rust.md:15` + `:28-29` (`rustup install 1.85.0` commands that
  silently install the wrong toolchain for any new contributor)

**Remediation (R2)**: file `docs/adr/0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md`
(name + content: rationale = time 0.3.47+ requires 1.88 to consume the
fix for the stack-exhaustion DoS). Sweep-update all three governance
files in a single commit. This is the textbook ADR trigger from
`CLAUDE.md`.

---

### T5 — `rusqlite` stayed at 0.32 (R2.T1 must-fix not landed); no TD filed [CRITICAL]

**Agents**: gp (CRITICAL), com (LOW carry-forward)

Round 2 T1 was a CRITICAL finding requiring `rusqlite 0.32 → 0.40`
because 0.32 bundles a ~14-month-old SQLite that will trip CVE
advisories. Plan v5's dep table committed `rusqlite 0.40`. `Cargo.toml:49`
ships `0.32`. The commit message acknowledges the drift ("stayed at
0.32") but `TECH-DEBT.md` contains only TD-001, and `docs/discovery-notes.md`
has no DN-007.

Per `CLAUDE.md § No Acceptable Trade-offs Policy` (line 137): "deferral
without a plan is a CRITICAL finding on its own." Shipping the drift
unannounced violates the policy.

**Remediation (R2)**: file `TD-002 — rusqlite pinned at 0.32 (CVE
exposure)` with a binding trigger ("bump to 0.40 by 2026-08-01 OR before
session 02 adds new schema columns, whichever first") AND a `DN-007`
cross-reference. OR bump now and verify `just ci` stays green —
rusqlite 0.40 is API-compatible for the operations we use.

---

### T6 — Decision artifact `docs/decisions/0001-catalog-schema-v1.md` missing [CRITICAL]

**Agents**: gp (CRITICAL), com (CRITICAL)

Plan v5 §Deliverables 8 (line 370) names it as a session-01 deliverable;
`crates/photohelper-catalog/src/schema.rs:3` doc-comment claims it is
"Authoritative reference"; `docs/plans/session-01.md` references it
twice. `ls docs/decisions/` shows only `.gitkeep`. Two broken
cross-references in shipped code + plan.

Per DN-005 (`docs/discovery-notes.md:54-57`) this session is the OWNER
of the schema decision. Skipping the decision doc means DN-005 cannot
be "partially resolved" — the partial-resolution claim in plan v5 is
itself unfulfilled.

**Remediation (R2)**: write `docs/decisions/0001-catalog-schema-v1.md`
with the exact `CREATE TABLE photos (...)` SQL from `schema.rs:8-30`,
the index rationale (idx_photos_source_path, idx_photos_camera_slug),
the supersede semantics + `camera_known` removal in favor of
`camera_slug IS NOT NULL`, and the v1→v2 migration policy (session 02
introduces the framework alongside cull-score + dup-group tables).

---

### T7 — Plan-promised `cfg(test)` test infrastructure entirely missing → 12 plan rows uncovered [CRITICAL]

**Agents**: test (CRITICAL ×2 + HIGH ×6), gp (HIGH overlap)

Plan v5 §Test infrastructure (`docs/plans/session-01.md:344-352`)
committed FOUR `cfg(test)` knobs:
1. `LOCK_RETRY_DELAY_MS` — **partially implemented** as
   `Catalog::open_with_retry_delay` (`catalog.rs:72`) but the helper is
   never called from any test.
2. `HEARTBEAT_INTERVAL_MS` — **MISSING** (`ingest.rs:30` hard-codes
   `Duration::from_secs(10)`); blocks plan row 48 entirely.
3. `poison_for_testing` on `Catalog` — **MISSING**; blocks row 18.
4. `fail_init_after_create_table` — **MISSING**; blocks row 12.

Plus `trybuild` dep declared (`crates/photohelper-core/Cargo.toml:24`)
without any `tests/ui/` directory or test driver — blocks row 6.

Plan rows missing entirely from the 16-row integration suite: **6**
(`PhotoId::from_db_bytes` visibility), **12** (transactional init
injection), **13** (cross-process file-lock), **14** (WAL-checkpoint
warn via SIGKILL — defer-able per plan), **17** (hardlink), **18**
(mutex poison + ROLLBACK), **19** (insert transactional rollback),
**34** (per-photo `.with_context()` boundary), **39** (`--strict` with
unknown camera), **42** (per-event tracing-level mapping), **43**
(parameterized fatal exit codes — only 1 of 3 sub-cases tested), **48**
(heartbeat at default verbosity), **49** (BEGIN IMMEDIATE SQLITE_BUSY).

The commit message "59 tests pass" is true but the **behavioral
coverage is ~36/50 plan rows = 72%** — meaningful test debt accumulated
silently.

**Remediation (R2)**: implement the three missing `cfg(test)` knobs +
the 13 missing test rows. OR explicitly defer specific rows to session
02 by listing each in `SESSION-STATE.md § Open Round-2 items` with a
binding trigger ("session 02 lands real CR3 fixtures + the row-14 + row-39
tests it enables"). The current silent-skip violates the
no-stop-gaps-without-trigger policy.

---

### T8 — `indicatif` spinner deliverable silently dropped [HIGH]

**Agents**: gp (HIGH)

Plan v5 §Deliverables 1 (line 116) commits to "`indicatif` spinner (not
progress bar — `par_bridge` is lazy)." `crates/photohelper-cli/Cargo.toml:28`
declares `indicatif.workspace = true` as a dependency. `grep -rn indicatif
crates/photohelper-cli/src/` returns zero hits. The spinner was a
deliverable, not optional polish.

**Remediation (R2)**: either wire a `ProgressBar::new_spinner()` in the
ingest driver with a 100ms tick (showing `walked` count) OR remove the
dep + amend the plan. Current state is silent deletion-by-omission.

---

### T9 — Governance file drift (HANDOFF Checkpoint 1; SESSION-STATE component table; DN-006) [HIGH]

**Agents**: gp (HIGH), com (MEDIUM ×3)

The session-end skill requires these to be checkpointed BEFORE the PR
opens; they are currently stale:

- **`SESSION-STATE.md`** (lines 10-11, 38-48): says "Last session: 0
  (bootstrap)" and lists **7 crates**. Session 01 added the 8th
  (photohelper-catalog) and the file should list 8 with the 4
  implemented (core/cameras/catalog/cli) flipped from "scaffolded" to
  "implemented (session 01)".
- **`HANDOFF_REPORT.md`**: last block is Checkpoint 0 bootstrap. The
  file's own contract (line 5: "Each session appends a checkpoint
  block") requires Checkpoint 1. Should capture MSRV bump rationale,
  rusqlite 0.32-vs-0.40 drift, and link to plan v5 + the four
  plan-review rounds.
- **`docs/discovery-notes.md`**: ends at DN-005. Code references
  `DN-006` by name in `crates/photohelper-cli/tests/cli.rs:65-75` and
  `ingest.rs:275-278` as if filed. Plan v5 explicitly says "if pre-flight
  shows CR3 ISO-BMFF parsing fails, file DN-006." The smoke test
  confirms it failed. DN-006 should be filed with owner = session 02 +
  real-CR3 fixtures.

**Remediation (R2)**: update all three at session-end (which is now).
This is plan v5 §Session-end housekeeping — explicit in the contract
and not a discretionary item.

---

### T10 — Silent error-swallowing cluster [HIGH]

**Agents**: sfh (HIGH ×3), arch (HIGH TOCTOU), rev (HIGH op-tag)

Five spots where errors are dropped without user signal:

1. **`rayon::ThreadPoolBuilder::build_global()`** swallowed via `let _`
   (`ingest.rs:99-101`): if the pool was already initialized (e.g., a
   prior test in the same process), the user's explicit `--threads N` is
   silently ignored. No log, no summary mention.
2. **`PRAGMA wal_checkpoint(TRUNCATE)`** error treated as "clean
   shutdown" (`catalog.rs:218-226`): `unwrap_or(0)` masks any future
   schema mismatch, busy, or column-count change as `recovered = 0`. The
   WARN at line 222 never fires for the unknown-status case.
3. **Magic-byte check TOCTOU** (`catalog.rs:137-166`): the
   `if catalog_path.exists()` check at line 137 happens BEFORE the lock
   is acquired (line 109 locks `.photohelper/catalog.db.lock`, not
   `catalog.db`). Process A deletes `catalog.db` between B's `exists()`
   and B's `File::open()` → B sees `Error::Io` instead of "first-run
   init." Rare in practice (writers don't delete the catalog) but
   violates lock-then-verify ordering.
4. **`Error::Io { op: "stat", ... }`** misnamed for file-lock failure
   path (`catalog.rs:129`): when `fs4::try_lock` returns
   `TryLockError::Error(e)`, we tag the op as "stat" — should be
   `"file-lock"` or `"lock-acquire"`. Operators debugging lock failures
   will be misdirected.
5. **`ContextForPath::with_context_for_path`** is a no-op trait
   (`ingest.rs:329-340`): the method returns `self` unchanged. The
   per-photo `.with_context(|| format!("ingesting {}", path.display()))`
   the plan committed to is NOT actually attached. Errors bubble to
   `main` with their raw `Error::Io { path }` context only — no
   per-photo enrichment from the driver.

**Remediation (R2)**:
1. `match build_global() { Ok(()) => INFO; Err(e) => WARN "--threads ignored, pool already initialized" }`.
2. `match wal_checkpoint { Ok(n) => …; Err(e) => WARN "could not check WAL recovery: {e}"; }` instead of silent `unwrap_or(0)`.
3. Move magic-byte check INSIDE the lock (after step 4 in `Catalog::open`).
4. Change `op: "stat"` → `op: "file-lock"`.
5. Either delete `ContextForPath` entirely (rely on `Error::Io { path }`
   structured context) OR actually implement `.with_context(|| format!(...))`.

---

### T11 — `ExifOrientation::from_tag` uses `PathBuf::new()` as sentinel [HIGH]

**Agents**: type (HIGH)

`crates/photohelper-core/src/model.rs:362-368`: when tag is outside
1..=8, returns `Error::Exif { path: PathBuf::new(), source: ... }` — an
empty PathBuf is a sentinel. Downstream `Display` renders `"EXIF parse
error at : invalid EXIF orientation tag: 9"` (empty between "at " and
":"). Today the only caller (`parse_exif`, `ingest.rs:392-397`) silently
discards via `if let Ok(orientation)`, so this never reaches users — but
the API invites misuse.

**Remediation (R2)**: either (a) add a dedicated
`Error::InvalidOrientationTag { tag: i64 }` variant with no path field,
or (b) take `path: &Path` in `from_tag(tag, path)`. Option (a) is
cheaper and preserves the invariant on `Error::Exif::path` (which
should always identify the offending file).

---

### T12 — Plan v5 row 32 assertion too loose (DN-006 fallback verdict not pinned) [MEDIUM]

**Agents**: test (MEDIUM)

`crates/photohelper-cli/tests/cli.rs:73`:
`assert!(camera_slug.is_none() || camera_slug.as_deref() == Some("canon-r8"))`
— passes for both branches. Per `docs/testing-standards.md § Be
specific`: this is the equivalent of `is_some_or_none()`. The DN-006
verdict was effectively made at implementation time (synthetic CR3
fixtures with `0xCC` bytes cannot be parsed by kamadak-exif → the
deterministic expectation is `camera_slug IS NULL`). Pin the test to
the deterministic branch with a comment recording the verdict.

**Remediation (R2)**: change to
`assert!(camera_slug.is_none(), "DN-006 fallback: kamadak-exif cannot parse synthetic 0xCC-byte CR3, so camera_slug must be NULL; session 02 with real CR3 fixtures will flip this to Some('canon-r8')")`.

---

### T13 — Type-design refinements [MEDIUM]

**Agents**: type (MEDIUM ×4)

Cluster of small type-design improvements:

- **`Photo::from_filesystem`** 7-param constructor
  (`model.rs:464-492`): params 3/4/5 (`clamped_mtime_unix_seconds: i64`,
  `mtime_anomalous: bool`, `photo_id: PhotoId`) are easy to transpose;
  no compile-time defense. Single callsite is currently correct.
  Suggest a small `MtimeFacts { clamped, anomalous }` newtype returned
  by `clamp_mtime` directly (`(i64, bool)` → `MtimeFacts`) to collapse
  two args into one with semantic meaning.
- **`PhotoRow`** stores `i64` for boolean columns
  (`mtime_anomalous`, `exif_orientation as i64` — though the latter is
  a real ordinal): convert at row-read boundary
  (`mtime_anomalous: bool = row.get::<_, i64>("...")? != 0`;
  `orientation: Option<ExifOrientation> = row.get::<_, Option<i64>>("...")?.map(ExifOrientation::from_tag).transpose()?`).
- **`AbsPath`** has no round-trip from stored catalog string: row's
  `source_path: String` never becomes `AbsPath` again. Acceptable for
  v0.1 (no consumer needs it yet); flag for session-02 if `cli camera`
  / dup-group commands want to operate on row paths.
- **`Error::Exif::source: BoxedSourceError`** erases the kamadak-exif
  error type: downstream can't `match err { kamadak::InvalidFormat =>
  skip }`. The `BoxedSourceError` rationale (storage-agnosticism)
  applies to `CatalogOpen`/`CatalogInsert` — EXIF is parsed by a fixed
  crate inside the CLI worker. Note for session 02.

**Remediation (R2)**: do the `MtimeFacts` cleanup (small, one-place
change). Defer the others as session-02 watch-list items in
`SESSION-STATE.md`.

---

### T14 — Simplification opportunities [MEDIUM/LOW]

**Agents**: simp (MEDIUM ×5, LOW ×4)

- **Duplicate 13-column INSERT in `Catalog::upsert`** (`catalog.rs:308-332,
  353-377`): same column list and `params![]` block twice. Extract `fn
  insert_row(tx, photo, pid, ingested_at) -> Result<(), Error>` helper.
  ~25 LoC saved, drift risk eliminated.
- **`UpsertOutcome` ↔ `IngestOutcome` variant remapping** (`catalog.rs:27-38`
  + `ingest.rs:320-326`): 3-variant near-clone of `IngestOutcome`'s
  catalog-side cases. The remapping exists purely because catalog has
  its own outcome type. Could either (a) move `IngestOutcome` to
  `photohelper-core::model` (already there!) and have `Catalog::upsert`
  return it directly; or (b) accept the cost. Option (a) deletes the
  enum + 10 LoC.
- **`apply_outcome` wildcard arm + `IngestOutcome::#[non_exhaustive]`**:
  the wildcard logs `tracing::warn!` if a new variant is added without a
  counter (`ingest.rs:214-216`). For a single-workspace crate boundary
  where driver + enum ship together, an exhaustive match (compile error
  on new variant) is stricter and better. Drop `#[non_exhaustive]` on
  `IngestOutcome` if cross-crate exhaustiveness isn't needed.
- **Dead code suppression**: `_suppress_unused_warnings` in `ingest.rs:437`
  and `_ensure_exif_metadata_compiles` in `catalog.rs:441` exist solely
  to placate the compiler. Delete the unused imports they reference
  AND the suppression functions.
- **`parse_exif_datetime`** (`ingest.rs:412-435`): 21 lines of manual
  split/parse. Use `time::macros::format_description!("[year]:[month]:[day] [hour]:[minute]:[second]")` + `PrimitiveDateTime::parse`.
  ~6 lines, type-safe. `time` is already a workspace dep with `macros`.
- **`PhotoRow::insert_error` helper** (`row.rs:76-81`): 6-line free
  function used only from `catalog.rs`. Inline as a closure.

**Remediation (R2)**: tackle the duplicate INSERT + the dead-code
suppression in this round (real cleanup wins). Defer the others as
session-02 candidates or accept as-is.

---

### T15 — Minor polish [LOW]

**Agents**: type (HIGH KnownCamera), rev (HIGH op tag), simp (LOW workspace allows)

- **`KnownCamera`** has no `Display` impl (only `Debug`): if any future
  log line prints `KnownCamera` directly it'll get `Debug` formatting
  (`"CanonR8"` not `"canon-r8"`). Add a one-line `impl Display`
  delegating to `slug()`. Today safe because `CameraId::Display` routes
  through `k.slug()`.
- **Workspace clippy allow list** in `Cargo.toml:86-98`: 11 pedantic
  allows in a block. Add a 1-line comment per allow rationalizing it
  (or split into "always justified" vs "audit periodically"). Not
  blocking.
- **Walker filter case-sensitivity on Windows**:
  `e.file_name() != ".photohelper"` uses byte-exact OsStr comparison.
  On case-insensitive Windows filesystems a `.PHOTOHELPER` dir slips
  through. v0.1 doesn't target Windows — flag for v0.2.
- **`UpsertOutcome` not `#[non_exhaustive]`**: minor uniformity issue
  with the rest of the codebase. Add the attribute for one-line
  consistency.

---

## Strengths to preserve [NOTES]

Confirmed by multiple agents — must not regress in R2:

- **Crate DAG is clean** (arch NOTE): `cli → core, catalog, cameras`;
  `catalog → core`; `cameras → core`; `core → ⊥`. No back-edges. The
  `BoxedSourceError` keeps `core` storage-agnostic; `rusqlite` does NOT
  leak into `core`'s deps.
- **`catalog_glue::photo_id_from_row_bytes` is THE sole public route**
  for raw-byte PhotoId reconstruction (type NOTE): `from_db_bytes` is
  `pub(crate)` to `core`; the bridge is the only `pub fn` minting from
  bytes; sole external caller is `PhotoRow::from_row`. Forgery surface
  closed exactly as plan v3→v4 mandated.
- **No production `unwrap`/`expect`/`panic`** (rev NOTE): every
  `.unwrap()` call is `#[cfg(test)]`-gated via crate-root `cfg_attr`.
  Workspace lints enforce; `-D warnings` in CI escalates. Spot-checked
  5 sites.
- **No SQL injection** (rev NOTE): every `execute`/`query_row` uses
  `params![...]`. The single `format!` in `all_rows` interpolates a
  const string only.
- **Error discipline**: no `#[from]` derives anywhere; every site uses
  explicit `.map_err(|e| Error::Io { path, op: "...", source: e })`.
  This is the discipline R3.T9 mandated and it landed cleanly.
- **`Mutex<Connection>` poison recovery** (rev NOTE): `let _ =
  conn.execute("ROLLBACK", []);` correctly discards because the txn
  might not be open after a panic. Then surfaces `Error::CatalogPoisoned`.
  Exactly the R3.T5 fix.
- **R1 deletions still preserved** (simp NOTE): zero hits for `Pipeline
  trait` / `PipelineCtx` / `Sidecar` / `CancellationToken` /
  dedicated-writer-thread / migration-framework in code. The R4 vote
  to ship held.

---

## R2 watch-list

After R1 remediation lands, R2 should specifically re-check:

1. **`no_exif` counter increments** on `exif.is_empty()` paths; smoke
   test confirms summary shows `no-exif: 1` for synthetic CR3.
2. **Heartbeat thread**: dead `if` removed; death-WARN actually fires
   if heartbeat dies early; no zombie output after `summary_line`.
3. **PhotoId hash window disjoint**: 100KB file's hash uses
   `[0..64KB)` + `[64KB..100KB)`, not overlapping. Unit test added.
4. **MSRV ADR exists** + all 3 governance files say `1.88.0`.
5. **TD-002 / DN-007** for rusqlite 0.32 (or actual bump to 0.40 with
   `just ci` green).
6. **`docs/decisions/0001-catalog-schema-v1.md` exists** with SQL +
   index rationale + supersede + camera_slug-not-known.
7. **Test infrastructure knobs**: at least 2 of 4 (`HEARTBEAT_INTERVAL_MS`
   + `poison_for_testing`) land + their corresponding test rows. The
   others may be deferred to session 02 only if explicitly tracked in
   `SESSION-STATE.md § Open Round-2 items` with a binding trigger.
8. **`indicatif` spinner**: wired OR dep removed + plan amended.
9. **HANDOFF Checkpoint 1**, SESSION-STATE update (8 crates,
   implemented status), DN-006 filed.
10. **Silent error swallowing** fixed for `build_global` + `wal_checkpoint`
    + magic-byte TOCTOU + error op tag.
11. **`ContextForPath`** trait either deleted or actually attaches
    context.
12. **`ExifOrientation::from_tag`** doesn't use `PathBuf::new()` sentinel.
13. **Test row 32** assertion pinned to deterministic branch with
    DN-006 comment.
14. **Duplicate INSERT** in `Catalog::upsert` extracted to helper.
15. **Dead code suppressors** removed (`_suppress_unused_warnings`,
    `_ensure_exif_metadata_compiles`).

If R2 surfaces CRITICAL-class regressions, fire R3.
