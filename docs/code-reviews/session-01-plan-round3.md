# session-01 plan-review Round 3

> Per `docs/quality-assurance.md § Double-review protocol`. Cadence A → Tier 5
> (plan review), full 8-agent suite re-fired against the v3 plan
> (`docs/plans/session-01.md` revision `0b76aff`). R3 was triggered by R2's
> 4 CRITICAL + 2 REGRESSION findings.
>
> Findings grouped by theme.

## Summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 5 | Address before any code lands. Several are mechanical 1-line fixes; one (token pattern) is a design flaw caught at compile. |
| **REGRESSION** | 3 | R2 fixes that didn't actually land cleanly (fs4 API name; ExifOrientation naming clarity; enum+atomics duplication worse than v2). |
| **HIGH** | 4 | Should address before session-end. |
| **MEDIUM** | 5 | Polish; simplifier argues defer to sub-component review. |
| **LOW** | 3 | Hygiene; fold in. |
| **CLEAN** themes (R2 fixes preserved) | 14 of 16 watch-list items |

**Round-4 verdict (split among 8 agents):** 5 vote R4 (gp, arch, rev, type, test); 3 vote ship-to-implementation (sfh, com, simp). Most of the R3 CRITICALs are mechanical (won't compile; one-line spec fixes). The simplifier makes a strong case that implementation pressure is a better forcing function for the remaining MEDIUMs than another plan-review cycle.

**Decision (per the planner)**: remediate v3 → v4 addressing every CRITICAL + REGRESSION + HIGH inline; then **ask the user** whether to fire R4 (focused on the v4 diff) or proceed to implementation with the residual MEDIUMs tracked as session-end watch-list items.

---

## Findings (by theme)

### R3.T1 — `fs4::FileExt::try_lock_exclusive` method name doesn't exist [REGRESSION]

**Agents**: gp (REGRESSION), arch (HIGH — closes R2.T1 incompletely)

R2 T1 swapped `fs2 0.4` → `fs4 1`, but the v3 plan kept the `_exclusive` suffix from the `fs2` API. **`fs4 1.x` renames the method to `FileExt::try_lock()` returning `Result<bool>`** (true = acquired, false = held elsewhere). Code written against `fs4::FileExt::try_lock_exclusive` will not compile.

**Remediation**: rename to `fs4::fs_std::FileExt::try_lock()` everywhere in §Deliverables 4 + §v3 adjustments. Document the `Result<bool>` shape.

---

### R3.T2 — `CatalogReconstructionToken` sealed pattern does not compile / creates DAG cycle [CRITICAL → REGRESSION of R2.T2]

**Agents**: type (REGRESSION-CRITICAL), arch (CRITICAL), rev (CRITICAL), gp (HIGH), test (REGRESSION row #6 stale)

Plan v3 §Deliverables 2 specifies: token defined in `photohelper-catalog` with `pub(crate)` constructor; `core::PhotoId::reconstruct_from_catalog(token, raw)` accepts it. **This doesn't work**: for `core` to name the token type, `core` needs a dependency on `catalog` — re-creating the cycle that R2 T7 explicitly closed. The "sealed trait" trick doesn't translate to sealed constructors.

**Three viable fixes**:
1. **Recommended**: `pub(crate) fn from_db_bytes` stays inside `core`; catalog row reconstruction lives in `core::catalog_glue` as `pub(crate)`. Catalog calls a tiny public function in `core` that takes the row fields and returns the PhotoId. No new type.
2. Sealed trait properly: `trait CatalogToken` with hidden supertrait in `core`, implemented by a unit struct in `catalog`. More code, same outcome.
3. Accept `pub fn from_db_bytes_unchecked` with a Safety doc — pragmatic, weaker.

**Remediation**: adopt Fix 1. Drop `CatalogReconstructionToken` entirely. Update §Deliverables 2 + test row 6.

---

### R3.T3 — Heartbeat thread fires at INFO but default verbosity is WARN — silently defeats its purpose [CRITICAL]

**Agents**: gp (CRITICAL)

§Observability tracing-level table lists "Heartbeat | INFO". §Deliverables 1 commits to the heartbeat. But the default `-v=0` filter pins to WARN — so the heartbeat is **invisible at default verbosity**, exactly when the user most needs it (no flags, running the tool casually).

**Remediation**: either (a) elevate heartbeat to WARN, or (b) print via direct `eprintln!` like the summary line. Pick (b) for consistency with the §Observability "summary always prints" pattern.

---

### R3.T4 — Heartbeat thread vs §Concurrency "no cancellation primitives" internal contradiction [CRITICAL]

**Agents**: com (CRITICAL), arch (CRITICAL lifetime), sfh (MEDIUM)

§Concurrency says "no `CancellationToken` this session" then commits to "A separate heartbeat thread — owned by the driver, joined at end-of-walk." If the driver short-circuits (panic, fatal error), `join()` waits up to 10s for the heartbeat's next sleep wakeup. If the heartbeat itself panics, the user loses the only liveness signal.

**Remediation**: specify the heartbeat as a *detached* thread that reads an `Arc<AtomicBool>` stop flag set by the driver at end-of-walk. Driver panic → flag stays false but process unwinds anyway (acceptable). Heartbeat panic → driver checks `handle.is_finished()` at join and logs WARN. Update §Concurrency to acknowledge the `AtomicBool` is a small concession that doesn't constitute "cancellation" in the CancellationToken sense.

---

### R3.T5 — `BEGIN IMMEDIATE` rollback-on-poison missing [CRITICAL]

**Agents**: rev (CRITICAL)

v3 commits to `BEGIN IMMEDIATE; ...; COMMIT;` per insert + `std::sync::Mutex` panic poison. But: on `PoisonError`, the next worker calls `.into_inner()` to recover the `Connection` — that connection still has an open transaction. Without an explicit `ROLLBACK;` on poison recovery, the next `BEGIN IMMEDIATE` returns `SQLITE_ERROR` ("cannot start a transaction within a transaction") and the catalog appears dead despite v3's "fail-loud" intent. Test row 18 doesn't assert post-poison DB consistency.

**Remediation**: §Deliverables 4 must specify: "On `PoisonError`, recover the connection via `.into_inner()`, issue `ROLLBACK;` (ignoring errors), then return `Error::CatalogPoisoned`." Update test row 18 to also assert `SELECT COUNT(*)` matches the pre-panic expected count (no partial rows).

---

### R3.T6 — `camera_known` + `camera_slug` redundant columns [CRITICAL/HIGH]

**Agents**: arch (CRITICAL), type (HIGH), sfh (LOW), simp (MEDIUM)

Two columns encoding one bit: `camera_known INTEGER NOT NULL DEFAULT 0` + `camera_slug TEXT` where the invariant `camera_slug IS NULL ⇔ camera_known = 0` is enforced nowhere. Future code can silently drift; queries on one vs the other disagree.

**Remediation**: drop `camera_known`; canonical "is known camera" predicate is `camera_slug IS NOT NULL`. Update schema, insert path, summary tally derivation. Add CHECK constraint `CHECK (camera_slug IS NULL OR camera_slug GLOB '*')` to enforce non-empty when set.

---

### R3.T7 — `mtime` clamp non-determinism across runs → silent re-ingest [HIGH]

**Agents**: sfh (HIGH)

v3 hashes `clamped_mtime` after clamping to `[1995-01-01, now() + 1 day]`. A file with a *future* mtime clamps to `now() + 1d` today, producing PhotoId A. Tomorrow's run with `now()` advanced may put the same file within range (different clamped value) → different PhotoId → silent re-ingest as a new row + supersede.

**Remediation**: pin the upper clamp ceiling to a static epoch (`2100-01-01`) rather than `now() + 1 day`. This makes the hash input run-independent. Lower ceiling stays at `1995-01-01`. Document the change in §PhotoId derivation. Update test row 28.

---

### R3.T8 — Lock retry budget 2.5s is wrong duration for the failure modes it claims to handle [HIGH/REGRESSION]

**Agents**: sfh (HIGH), arch (REGRESSION rationale weak), test (CI-time concern)

5×500ms = 2.5s total. For *stale lock* (crashed prior process on NFS/SMB), retrying for 2.5s and then giving up is the right call. For *legitimate concurrent ingest* (another `photohelper` running on 50k photos for minutes), 2.5s is far too short — user gets `Error::CatalogLockHeld`, assumes the first instance crashed.

**Remediation**: increase budget to 60s default (e.g., 12×5s) with WARN every retry; expose a `--catalog-lock-timeout-seconds <N>` flag for users who want to fail fast (CI scenarios). Update §Deliverables 4 + test row 13 + §Dependencies if a small new dep is needed.

---

### R3.T9 — `ExifOrientation` slot-5/7 names verbose + potentially confusing [REGRESSION]

**Agents**: rev (REGRESSION), type (CLEAN)

v3 fixed the variant *order* (slot 5 is now correctly `MirrorHorizontalRotate90Cw` = transpose). But the compound name is awkward and could be misread (mirror-then-rotate vs rotate-then-mirror are not commutative). EXIF spec uses the canonical names `Transpose` (slot 5) and `Transverse` (slot 7).

**Remediation**: rename slot 5 to `Transpose` and slot 7 to `Transverse` (per EXIF spec). Update §Deliverables 2 + test rows 26 + 27.

---

### R3.T10 — `IngestOutcome::Inserted` carries 4 flag fields + `IngestStats` 10 atomics — worst of both worlds [REGRESSION]

**Agents**: simp (REGRESSION), type (MEDIUM)

R2 simplifier MEDIUM was rejected with rationale "variants carry semantics." v3 then enriched `IngestOutcome::Inserted` with three boolean flags (`camera_known`, `no_exif_fields`, `mtime_anomalous`) **AND** added `IngestStats { 10 AtomicU64 }`. Now both layers exist and the flags exist solely to route signals to the atomics. Strictly worse than either pre-R2 shape.

**Remediation**: drop the boolean flags from `Inserted` (`Inserted(PhotoId)` only). The driver writes the catalog row, then reads the same row's columns (`camera_slug IS NOT NULL`, `mtime_anomalous`) to increment the right atomics. Single source of truth for each fact. Adds 3 atomic counter increments based on row content, removes ~4 fields from the enum payload.

---

### R3.T11 — EXIF reader still "TBD" in deliverable contract [HIGH]

**Agents**: com (HIGH), arch (CRITICAL — pre-flight not yet done)

§Dependencies row "EXIF reader | TBD" is a contradiction in a deliverable contract. Plan says pre-flight runs "at start of implementation" — but if it fails, 30+ tests + the whole `IngestStats`/`IngestOutcome` shape have to be reworked.

**Remediation**: commit to `kamadak-exif 0.6` as the v0.1 default. Document the fallback (DN-006: if CR3 ISO-BMFF parsing fails, the EXIF source moves to LibRaw in session 02, and session 01 ships with all `make`/`model`/`capture_time` columns NULL for CR3s — degraded but functional). Drop "TBD" from the deps table. The pre-flight becomes a session-01 *risk*, not a *blocker*.

---

### R3.T12 — Plan length 660 lines — 180 over R2 watch-list target [MEDIUM]

**Agents**: simp (REGRESSION), com (MEDIUM), gp (MEDIUM)

R2 watch-list item #16 asked for ≤480 lines. v3 ships 660 (+37%). Most growth is load-bearing (49-row test plan from R2 demands; observability tables), but ~15-30 lines are condensable (verbose tracing-level table; v2→v3 adjustments paragraph duplicates inline annotations).

**Remediation**: light prune — fold v2→v3 adjustments into the inline "(closes Round 2 T#)" annotations; condense tracing-level table by merging same-event-class rows. Target: ≤620 lines after the v4 fixes above land.

---

### R3.T13 — Test plan gaps + flakiness risks [MEDIUM]

**Agents**: test (multiple severities)

- **Row #48 heartbeat** non-deterministic — needs an injectable interval (e.g. `HEARTBEAT_INTERVAL_MS` `cfg(test)` const at 100ms).
- **Rows #12 / #18** need explicit test-hook mechanism (the plan owes the implementer the shape — `rusqlite::trace` callback? `cfg(test)` knob?).
- **Row #13** at 2.5s real-time will be longer (60s) after R3.T8 fix — make `LOCK_RETRY_DELAY_MS` injectable.
- **Row #14** SIGKILL flaky on CI macOS — add `cfg_attr(target_os = "macos", ignore)` note or document the flake.
- **Row #8** symlink test needs `cfg(unix)` — Windows is out of scope but the test row is unconditional.
- **Row #32** doesn't assert `camera_known = 1` AND `camera_slug = 'canon-r8'` for the Canon R8 fixture — explicit column assertions needed for the v3 schema change (this also becomes simpler after R3.T6 drops `camera_known`).
- **`BEGIN IMMEDIATE → SQLITE_BUSY`** path no test.

**Remediation**: add a new §Test infrastructure subsection naming the `cfg(test)` knobs (heartbeat interval, lock retry delay, init failure injection). Add 2-3 new test rows for the column assertions + the BUSY path.

---

### R3.T14 — Other hygiene findings [LOW]

**Agents**: com (LOW ×2), sfh (LOW), arch (LOW), gp (LOW)

- `IngestStats` declaration site not named (lives in `cli::commands::ingest`; should say so).
- `Scope expansions` table has 9 rows now (vs the "7" the body claims).
- Session-end housekeeping listed under §Tech-debt — wrong section.
- Exit-code 1 row doesn't cite the convention (POSIX generic failure).
- `Error::CatalogSchemaTooNew` no remediation hint.
- `time` feature on rusqlite — deliberately off; document.

**Remediation**: batched copy-edits.

---

## CLEAN themes (R2 fixes preserved) — confirmed by R3

1. **R2.T1 dep swap** — fs4 + rusqlite 0.40 chosen (modulo R3.T1 method-name bug).
2. **R2.T2 PhotoId forgery** — direction right (modulo R3.T2 cycle bug).
3. **R2.T3 file-lock try-lock** — try-lock pattern adopted (modulo R3.T8 budget length).
4. **R2.T4 std::sync::Mutex + BEGIN IMMEDIATE** — mostly clean (modulo R3.T5 rollback-on-poison gap).
5. **R2.T5 path-escape** — `AbsPath::canonicalize_within` + `Error::PathEscapesRoot` + test row 8.
6. **R2.T6 Catalog Send+Sync** — explicit fields + compile-time assertion test row 20.
7. **R2.T7 ingest_one placement** — moved to `photohelper-cli::commands::ingest`.
8. **R2.T8 observability** — summary survives `-q`, mtime-anomalous slot, NoExifFields, exit-code deconflict (modulo R3.T3+T4 heartbeat issues).
9. **R2.T9 type design** — ExifOrientation variant order correct, encoding ambiguity fixed (modulo R3.T6 column redundancy + R3.T9 naming + R3.T10 enum+atomics).
10. **R2.T10 stub exit code** — 69 EX_UNAVAILABLE.
11. **R2.T11 test gaps** — 49 rows, exact count (modulo R3.T13 flakiness).
12. **R2.T12 plan hygiene** — heading parentheticals dropped, decision-doc as Deliverable 8, schema init transactional, tense standardized.
13. **R2.T13 simplicity** — Canonicalize + NulByteInPath collapsed into Io.
14. **`parking_lot` removal** — completely removed.

---

## Round-4 trigger assessment

R3 has 5 CRITICAL + 3 REGRESSION findings — by the literal protocol ("CRITICAL-class regressions needing another cycle → add Round N+1"), R4 fires.

But: 4 of the 5 CRITICALs are mechanical fixes (1-line spec changes), 2 of the 3 REGRESSIONs are compile-time issues that won't survive `cargo build`, and the simplifier flags genuine "review-cycle hell" risk (660 lines + 198 over R2's target; adding fixes without removing complexity).

After v4 remediation lands, the planner will ask the user via AskUserQuestion whether to fire focused R4 against the v4 diff or proceed to implementation with the residual MEDIUMs as session-end watch-list items.

**R4 watch-list (if it fires)**:
1. `fs4::FileExt::try_lock()` (no `_exclusive`) everywhere.
2. `CatalogReconstructionToken` deleted; `pub(crate) fn from_db_bytes` is the only path; catalog calls via `core::catalog_glue` module.
3. Heartbeat prints via `eprintln!` (not tracing); detached thread + `AtomicBool` stop flag.
4. `Catalog` insert path explicit: lock → BEGIN IMMEDIATE → execute → COMMIT; on poison: `.into_inner()` → ROLLBACK → return `Error::CatalogPoisoned`.
5. `camera_known` column dropped; `camera_slug IS NOT NULL` is the predicate.
6. mtime clamp ceiling pinned to `2100-01-01` (run-independent).
7. Lock retry budget extended to 60s default + `--catalog-lock-timeout-seconds` flag.
8. ExifOrientation slot 5/7 → `Transpose` / `Transverse` (EXIF canonical names).
9. `IngestOutcome::Inserted(PhotoId)` only; driver computes flags from the written row.
10. EXIF reader committed to `kamadak-exif 0.6`; DN-006 fallback documented.
11. §Test infrastructure subsection naming `cfg(test)` knobs.
12. Plan length under ~620 lines.
