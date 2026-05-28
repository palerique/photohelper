# session-01 plan-review Round 4 (focused)

> Per `docs/quality-assurance.md § Double-review protocol`. **Focused R4** —
> 5 agents (gp + arch + rev + type + simp) rather than full 8, chosen by the
> user as a tighter follow-up to R3 (full 8 would re-cover settled ground).
>
> Fired against v4 plan (`docs/plans/session-01.md` revision `705830a`,
> 596 lines). Focus: verify all 12 R3 watch-list items closed cleanly; catch
> any new regressions introduced by v4 remediation.

## Summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 0 | All R3 must-fix items confirmed CLEAN across all 5 lenses. |
| **REGRESSION** | 2 | Both are 1-line spec fixes (wording + clap range); v4→v5 inline. |
| **HIGH** | 2 | Polish/documentation; v4→v5 inline. |
| **MEDIUM** | 4 | Type-design + test-design refinements; v4→v5 inline. |
| **LOW** | 4 | Cosmetic; some fold into v5, rest tracked as session-end watch items. |
| **CLEAN** | 14 R3 items | All R3 watch-list closures verified. |

**Round-5 verdict (5/5 agents)**:
- general-purpose: SHIP. "No CRITICAL/HIGH/MEDIUM regressions detected."
- code-architect: SHIP. "v4 successfully landed all 12 R3 architectural fixes."
- code-reviewer: Remediate v4→v5 inline; *optional* R5 on the diff (2 REGRESSIONs are not compile-time).
- type-design-analyzer: "Do NOT fire R5." Inline 1-line edits.
- code-simplifier: SHIP. "Implementation pressure is now a strictly better forcing function than another plan-review cycle for the residual LOWs."

**Decision (per the planner)**: 4 of 5 agents explicitly recommend ship-without-R5; the 5th (rev) flags 2 REGRESSIONs that are mechanical inline fixes. Apply v5 targeted edits closing every R4 REGRESSION + HIGH + MEDIUM, then proceed to implementation. The remaining LOWs become session-end watch-list items.

---

## Findings (by theme)

### R4.T1 — Heartbeat "detached thread" terminology mismatch [REGRESSION]

**Agents**: arch (REGRESSION)

v4 §Deliverables 1 says heartbeat is a "detached `std::thread::spawn` thread" but ALSO says "Driver sets the flag at end-of-walk and checks `handle.is_finished()`". `std::thread::spawn` returns a `JoinHandle<T>`; a *detached* thread has no `JoinHandle`. To call `is_finished()` the driver must retain the handle. The thread is **spawned but not joined** (handle retained for status check) — not truly detached.

**Remediation (v5)**: replace "detached thread" with "spawned (handle retained for `is_finished()` check; never joined)" in §Deliverables 1 + §v3→v4 adjustments.

---

### R4.T2 — `--catalog-lock-timeout-seconds 0` edge case + no upper bound [REGRESSION / MEDIUM]

**Agents**: rev (REGRESSION R4.N1), arch (MEDIUM clap flag widening)

v4 declares the flag without a `value_parser` range. User-supplied `0` is accepted → loop runs 0 attempts → immediate `Error::CatalogLockHeld { attempts: 0, total_ms: 0 }` with awkward "exhausted lock budget after 0 attempts over 0ms" wording. Conversely, `u32::MAX` is also accepted → effectively infinite wait.

**Remediation (v5)**: §Deliverables 1 specifies `value_parser = clap::value_parser!(u32).range(1..=3600)` — minimum 1 second, maximum 1 hour. Range cap prevents user typos. Update the test row that covers clap parse failures to include `--catalog-lock-timeout-seconds 0` and `... 5000`.

---

### R4.T3 — Test row 32 assertion contradicts DN-006 fallback [REGRESSION]

**Agents**: rev (REGRESSION R4.N4)

Test row 32 asserts `camera_slug = 'canon-r8'` for a `.cr3` fixture. If pre-flight surfaces that `kamadak-exif 0.6` can't parse CR3 ISO-BMFF EXIF (DN-006 fallback), `make`/`model` will be NULL → `for_exif` lookup returns `None` → `camera_slug IS NULL`. The test row then fails despite the v4 plan claiming this fallback is acceptable session-01 behavior.

**Remediation (v5)**: split test row 32 into two paths — if kamadak-exif parses CR3 EXIF (default expectation), `camera_slug = 'canon-r8'`; if DN-006 fallback active (pre-flight verdict negative), `camera_slug IS NULL` AND `make`/`model` also NULL for CR3. Decided at implementation start by pre-flight outcome. Document this conditional in the row.

---

### R4.T4 — `ExifMetadata` type named but not specified [MEDIUM]

**Agents**: type (MEDIUM)

v4 §Deliverables 5 mentions `Photo::from_filesystem(canonical, file_size, clamped_mtime, exif: ExifMetadata)` and the `NoExifFields` IngestOutcome variant, but `ExifMetadata` itself is never spec'd. Implementer will invent the shape.

**Remediation (v5)**: spec it explicitly in §Deliverables 2 model module:
```rust
pub struct ExifMetadata {
    pub make: Option<String>,
    pub model: Option<String>,
    pub capture_time_unix_seconds: Option<i64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub orientation: Option<ExifOrientation>,
}

impl ExifMetadata {
    pub fn is_empty(&self) -> bool { /* all None */ }
}
```
The `is_empty()` method is the signal `ingest_one` uses to route to `NoExifFields`.

---

### R4.T5 — `Aspect` enum missing `#[non_exhaustive]` [MEDIUM]

**Agents**: type (MEDIUM)

Every other domain enum in v4 is `#[non_exhaustive]` (`KnownCamera`, `ExifOrientation`, `IngestOutcome`, `Error`). `Aspect { Landscape, Portrait, Square }` is the lone exception. Adding `Panoramic` later would break every downstream `match`.

**Remediation (v5)**: prepend `#[non_exhaustive]` to the `Aspect` declaration.

---

### R4.T6 — Test row 32 mtime flakiness risk [MEDIUM]

**Agents**: rev (MEDIUM R4.N3)

Row 32 asserts `mtime_anomalous = 0` for a "normal" file but doesn't set the fixture mtime explicitly. CI machines with clock drift, time-travel test bugs, or VM snapshots might end up with fixture mtimes > now+1d. (Note: the v4 ceiling is now static `2100-01-01`, so the failure mode is reduced but the `1995-01-01` lower bound is still relative.) Either way, explicit fixture mtime via `filetime::set_file_mtime` to a known in-range value (e.g. `2020-01-01`) eliminates the flakiness vector.

**Remediation (v5)**: add the explicit mtime-setting instruction to test row 32; add `filetime` to dev-dependencies if not present.

---

### R4.T7 — Lock retry WARN noise undocumented [HIGH]

**Agents**: rev (HIGH R4.N2)

12 WARNs over 60s during a real concurrent-ingest run. Acceptable (rare edge case; `-q` suppresses) but undocumented — a sub-component reviewer will flag it as log spam.

**Remediation (v5)**: add one sentence to §Deliverables 4 lock-retry bullet: "(12 WARNs over 60s is acceptable for the concurrent-ingest edge case; `-q` suppresses)."

---

### R4.T8 — Production lock retry timing too slow for tests not using override [HIGH]

**Agents**: arch (HIGH)

Real production = 5s × 12 = 60s. Any catalog-lock test that doesn't use the `LOCK_RETRY_DELAY_MS=50ms` `cfg(test)` override sleeps for real time. A single naive test could add 60s to `cargo test`.

**Remediation (v5)**: §Test infrastructure adds: "All catalog-lock-exercising tests MUST use the `LOCK_RETRY_DELAY_MS` test override; CI gates fail if any test takes >5s by default (a guard against forgotten overrides)."

---

### R4.T9 — Minor polish [LOW]

**Agents**: type (LOW ×2), simp (LOW ×2)

- `core::catalog_glue` module name direction-ambiguous; could be `core::for_catalog` or `core::catalog_reconstruction` (purpose-naming over relationship-naming). Defer.
- `Catalog::open(.., lock_timeout_seconds: u32)` bare integer vs `Duration`. CLI converts at the boundary; library could stay unit-agnostic. Defer to implementation.
- `cfg(test)` knob proliferation (4 knobs). `LOCK_RETRY_DELAY_MS` could plausibly be a `std::env::var` lookup (lighter). Defer.
- §v3→v4 adjustments paragraph (~43 lines) redundant with inline `(closes R3.T*)` annotations. Defer to a v6 prune post-implementation.

**Remediation**: track as session-end watch-list items; don't touch v5.

---

## CLEAN themes (R3 watch-list confirmed by R4)

All 12 R3 watch-list items confirmed CLEAN across all 5 agents:

1. **R3.T1** `fs4::FileExt::try_lock()` (no `_exclusive`) everywhere. Dep table explicit.
2. **R3.T2** `CatalogReconstructionToken` deleted; `core::catalog_glue::photo_id_from_row_bytes` is the bridge. No `core → catalog` cycle.
3. **R3.T3** Heartbeat via `eprintln!` (not tracing). Visible at default verbosity.
4. **R3.T4** Heartbeat thread `AtomicBool` shutdown signal scoped + reconciled with "no general cancellation" intent.
5. **R3.T5** ROLLBACK-on-poison sequence specified in both §v3→v4 adjustments AND §Deliverables 4.
6. **R3.T6** `camera_known` column dropped; `camera_slug IS NOT NULL` is canonical.
7. **R3.T7** mtime clamp ceiling pinned to static `2100-01-01`; run-independent.
8. **R3.T8** Lock retry budget 60s default + `--catalog-lock-timeout-seconds` flag.
9. **R3.T9** ExifOrientation slots 5/7 → `Transpose` / `Transverse` (EXIF canonical).
10. **R3.T10** `IngestOutcome::Inserted(PhotoId)` only; driver reads row columns for tallies.
11. **R3.T11** EXIF reader committed (`kamadak-exif 0.6`); DN-006 fallback documented.
12. **R3.T13** §Test infrastructure subsection naming all 4 `cfg(test)` knobs.

Plus: R1 deletions still preserved (Pipeline trait, PipelineCtx, Sidecar, CancellationToken, dedicated writer thread, migration framework). Plan length 596 ≤ R3 target of 620.

---

## Verdict

**Ship v5 (after applying R4.T1–T8 remediation) to implementation.** No Round 5.

The R4 REGRESSIONs and MEDIUMs are mechanical inline edits; collectively ~30 lines of diff. The remaining LOWs are session-end watch-list items that implementation pressure will surface more cheaply than another review cycle.

After v5 lands, the plan-review phase is complete; implementation begins per the four-step session protocol's step 2 ("Implement per remediated plan").
