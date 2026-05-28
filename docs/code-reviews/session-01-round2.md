# session-01 session-end Round 2 (code review)

> Per `docs/quality-assurance.md § Session-end protocol § Double-review protocol`.
> Cadence A → Tier 5 (session end), full 8-agent suite fired in parallel against
> the R1 remediation diff `0f28627^..HEAD` (3 commits, 22 files, +1728/-360):
>   - `0f28627` fix(session-01): R1 remediation (session-end Round 1 findings)
>   - `02d43d1` chore(harness): sync AI-protocol improvements from fox/eng-protocol
>   - `3e33ccb` chore(session-01): save intermediate state for context refresh
>
> Findings grouped by **theme** (not by agent) per
> `docs/quality-assurance.md § Consolidation discipline`. When multiple agents
> flagged the same theme, agents cited in brackets. The R1 watch-list
> (`docs/code-reviews/session-01-round1.md § R2 watch-list`) was the acceptance
> checklist; R2 verified each item AND surfaced fresh regressions introduced
> by R1 remediation.

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
  cache_used: false
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

## Summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 13 | Includes 7 themes from R1 watch-list (4 closed, 3 with new regressions), 5 new bugs surfaced by R2, 1 production-trace bug surfaced by user. Block session-end until remediated. |
| **HIGH** | 14 | Includes regressions inside R1's own remediation commit. Must address before merge. |
| **MEDIUM** | 12 | Polish + small refactors. Some defer to session 02 with binding triggers; others fix now. |
| **LOW** | 7 | Hygiene + count drifts + minor doc-comment fixes. |
| **NOTES** (strengths preserved + bug classes) | 6 | Confirm in R3 if fired. |

Agent suite: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer` (type),
`pr-review-toolkit:silent-failure-hunter` (sfh),
`pr-review-toolkit:comment-analyzer` (com),
`pr-review-toolkit:pr-test-analyzer` (test),
`pr-review-toolkit:code-simplifier` (simp).

---

## CRITICAL

### R2-T1 — Magic-byte TOCTOU "deferral" is a false positive; SESSION-STATE is wrong

**Agents**: com (CRITICAL — definitive); gp + arch + rev + sfh + test (CRITICAL — said "ungoverned deferral")

Reading `crates/photohelper-catalog/src/catalog.rs:108-180` directly:
- Line 109: `File::create(&lock_path)` — creates the lock file (not the actual lock).
- Line 118-148: `loop { try_lock(&lock_file) → Ok(()) => break; ... }` — the actual lock acquisition.
- Line 151: `if catalog_path.exists()` — **fires AFTER the loop exits, i.e., while holding the lock.**

The R1.T10 sub-item 3 description ("line 109 locks `.photohelper/catalog.db.lock`, not catalog.db") was based on a misread: line 109 *creates* the lock file; `try_lock` is at line 120 inside the loop. The check at line 151 IS inside the lock window. SESSION-STATE.md:95-99 faithfully reproduced R1's misformulation and shipped a "carry-forward to R2 remediation" item that requires no code change.

Five other agents (gp/arch/rev/sfh/test) flagged this as "ungoverned deferral, file TD-003" per `CLAUDE.md § No Acceptable Trade-offs Policy` — they all assumed the deferral was real. Agent 6 verified the code and surfaced the simpler truth.

**Remediation (R2)**: update `SESSION-STATE.md:95-99` to read: *"Verified: lock IS acquired at `catalog.rs:121` BEFORE magic-byte check at `:151`. R1.T10 sub-item 3 was misformulated; closed-by-verification, no TD needed."* Update R2 watch-list item 10 to "resolved-by-verification, not by code change." Add a one-line in-code comment at `catalog.rs:150` confirming `// Step 5 runs AFTER Step 4 lock acquisition; magic-byte check is in-lock.`

### R2-T2 — `IngestOutcome::NoExifFields` dead variant + dead `apply_outcome` arm + lying docstring

**Agents**: gp + arch + type + sfh (4-way CRITICAL convergence)

`crates/photohelper-core/src/model.rs:611` defines `NoExifFields`; `crates/photohelper-cli/src/commands/ingest.rs:241-243` matches on it; but `ingest_one` (lines 257-364) increments `stats.no_exif` at line 311 directly and always returns `Inserted` / `SupersededPrevious` / `AlreadyCatalogued` (lines 357-363). `UpsertOutcome` has no `NoExifFields` mapping. So:
- The variant is defined but never constructed.
- The `apply_outcome` arm is unreachable.
- The `ExifMetadata::is_empty` doc-comment (`model.rs:447-448`) still claims the variant is "the signal `ingest_one` uses to route to `IngestOutcome::NoExifFields`" — false post-R1.T1.
- **Double-count footgun**: if a future contributor "completes" the refactor by making `ingest_one` return `IngestOutcome::NoExifFields`, the counter will bump TWICE (line 311 + line 242).

**Remediation (R2)**: delete `IngestOutcome::NoExifFields` from `model.rs:611`, delete the matching arm at `ingest.rs:241-243`, fix `model.rs:447-448` docstring. Also drop `#[non_exhaustive]` on `IngestOutcome` (`model.rs:591`) since the enum + driver ship in the same workspace — the wildcard at `ingest.rs:247-249` then becomes a compile error on a new variant (stricter than today's runtime WARN).

### R2-T3 — `Catalog::upsert` `query_row(...).ok()` swallows all SQLite errors as "row missing" — new bug class

**Agents**: sfh (CRITICAL — only agent who found this)

`crates/photohelper-catalog/src/catalog.rs:295-301` (`existing_by_id`) and `:312-319` (`existing_at_path`) both call `tx.query_row(...).ok()`. This collapses every `rusqlite::Error` to `None`, including `SqliteFailure`, `InvalidColumnType`, disk-full mid-read, schema-mismatch after a concurrent ALTER. The pattern is indistinguishable from `Error::QueryReturnedNoRows` — the only error this code should treat as "no row." A real lookup failure now silently falls into the "insert new row" branch, where `do_insert` will likely also fail and bubble a misattributed `Error::CatalogInsert` — masking the true error in the lookup.

R1.T10 enumerated 5 silent-failure spots and missed both of these on the very file it was auditing.

**Remediation (R2)**: replace each `.ok()` call with an explicit `match` mapping only `QueryReturnedNoRows` → `None` and propagating every other error via `insert_error(pid, e)`. Pattern:
```rust
let existing_by_id = match tx.query_row(..., |_| Ok(())) {
    Ok(v) => Some(v),
    Err(rusqlite::Error::QueryReturnedNoRows) => None,
    Err(e) => return Err(insert_error(pid, e)),
};
```

### R2-T4 — Heartbeat env-override is mathematically broken below 100ms granularity

**Agents**: gp + arch + sfh (3-way CRITICAL)

`crates/photohelper-cli/src/commands/ingest.rs:32-39` accepts `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` with floor `ms.max(10)` — advertising 10ms minimum. But `heartbeat_loop` at lines 205-211 uses `granularity = Duration::from_millis(100)` then computes `ticks = (interval.as_millis() / 100).max(1)`. For requested interval=50ms (the value the test uses): ticks=1, but the first iteration still sleeps `granularity = 100ms` before any heartbeat fires. **Effective minimum heartbeat latency is 100ms regardless of env var** — the test's `PHOTOHELPER_HEARTBEAT_INTERVAL_MS=50` is silently re-mapped.

Also: `PHOTOHELPER_HEARTBEAT_INTERVAL_MS=fast` / `=-1` / `=""` / overflow values all silently fall through to the 10s default with no log.

**Remediation (R2)**: change `granularity = interval.min(Duration::from_millis(100))` so sub-100ms env values are honored AND the responsive-to-stop-flag property is preserved. Also log the resolved interval at INFO at startup so operators can observe the actual cadence. Optionally: extract the env-var parsing to a pure function `parse_heartbeat_ms` + unit-test the 5 edge cases.

### R2-T5 — EXIF "parse succeeded with zero fields" WARN lies on the failure path (regression from R1.T1)

**Agents**: sfh (HIGH); USER (production trace at `/Users/ph/Pictures/tests`, 371 CR3 files, 2026-05-28 15:32:52). Severity escalated to CRITICAL by convergence with user-observed prod behavior.

`crates/photohelper-cli/src/commands/ingest.rs:301-313`. Flow:
1. `parse_exif()` returns `Err(InvalidFormat)` for CR3 ISO-BMFF.
2. `.unwrap_or_else` (line 301) catches, logs `"EXIF parse failed"` ✓, substitutes `ExifMetadata::default()` (empty).
3. `if exif.is_empty()` (line 306) unconditionally fires → logs `"EXIF parse succeeded with zero fields"` ✗ — **the parse did NOT succeed**.

User's prod run produced TWO contradictory WARN lines per file × 370 files = 740 misleading WARN lines.

**Remediation (R2)**: gate the second WARN on parse-actually-succeeded:
```rust
let (exif, parse_failed) = match parse_exif(canonical.as_path()) {
    Ok(e) => (e, false),
    Err(err) => {
        tracing::warn!(error = %err, path = ..., "EXIF parse failed");
        (ExifMetadata::default(), true)
    }
};
if exif.is_empty() {
    stats.no_exif.fetch_add(1, Ordering::Relaxed);
    if !parse_failed {
        tracing::warn!(path = ..., "EXIF parse succeeded but yielded zero fields");
    }
}
```

### R2-T6 — Heartbeat env-override test is `expect(true).toBe(true)` — blocks merge per global testing standards

**Agents**: gp + rev + sfh + test (4-way CRITICAL)

`crates/photohelper-cli/tests/cli.rs:429-449`. Test name `heartbeat_appears_at_default_verbosity_via_env_override` promises plan-row-48 behavioral coverage; the assertion at line 448 is `assert.stderr(contains("walked: 1"))` — the **unconditional summary line** at `ingest.rs:186`, hit whether or not the heartbeat fires, whether the env var is read, whether `heartbeat_loop` exists at all.

Per `~/.claude/CLAUDE.md § Testing Standards § Code Review Policy`: *"BLOCK merge if any meaningless assertions are found. BLOCK merge if tests don't verify actual behavior."* Per `docs/testing-standards.md § Be specific`: *"If removing the test subject doesn't break the test, the test is invalid."* Removing the entire `heartbeat_loop` body would leave this test green.

The test's own inline comment self-admits the issue (lines 440-447): *"if walk finishes faster than 50ms, the heartbeat may not have ticked. Assert weakly: stderr should contain SOMETHING."* That admission is the bug, not a defense.

**Remediation (R2)**: assert on a heartbeat-specific substring (e.g., `stderr(contains("[heartbeat]"))`) AND make the test deterministic by either (a) raising the granularity-vs-interval coupling per R2-T4 + adding a fixture that takes >50ms to walk (e.g., 100 files), or (b) renaming to `heartbeat_env_override_does_not_panic` + explicitly listing row 48 as deferred in DN-008 with a session-02 binding trigger. **Do NOT ship as-is** — global testing standards block merge.

### R2-T7 — ADR-0001 misattributes the vulnerable `time` API surface (audit-trail corruption)

**Agents**: com (CRITICAL — only agent who checked the advisory)

`docs/adr/0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md:11-12` claims RUSTSEC-2026-0009 is *"a stack-exhaustion denial-of-service in `time::format_description::parse` that affected versions < 0.3.47."* The advisory describes a DoS in the **value-parsing entry points** (`Date::parse`, `OffsetDateTime::parse`, `PrimitiveDateTime::parse`, `Time::parse`, `UtcDateTime::parse`, `UtcOffset::parse`, `parsing::Parsed::parse_item`) when fed maliciously crafted **RFC-2822 input** — not in `time::format_description::parse` (which parses format strings, not values).

The MSRV-bump conclusion is otherwise correct (`time 0.3.47` does require rustc 1.88.0), but the load-bearing technical claim in an "Accepted" ADR is wrong, corrupting the audit trail for future reviewers chasing the CVE.

**Remediation (R2)**: replace the function name with *"the `time::*::parse` value-parsing entry points (RFC-2822 path)"* or quote the advisory's vulnerable-function list verbatim.

### R2-T8 — Decision doc 0001 enshrines a fabricated `BEGIN IMMEDIATE` invariant

**Agents**: com (CRITICAL — only agent who reconciled doc vs code)

`docs/decisions/0001-catalog-schema-v1.md:18-22` reads *"Wrap the init in `BEGIN IMMEDIATE; ...; COMMIT;` so partial init ... cannot leave the database in an ambiguous half-initialized state."* The actual init code at `crates/photohelper-catalog/src/catalog.rs:209` uses `conn.transaction()` — which defaults to `BEGIN DEFERRED` per `rusqlite`. The explicit-immediate form `transaction_with_behavior(Immediate)` IS used elsewhere (`catalog.rs:291` in `upsert`), so the omission at line 209 is structural, not a typo.

The new "authoritative" decision doc enshrines a fabricated invariant. Same false claim already exists in `schema.rs:8-9` from the initial implementation; the decision doc was the chance to reconcile and instead doubled down.

**Remediation (R2)**: either (a) rewrite the prose to say `BEGIN DEFERRED` and explain that init contention is impossible because the file lock serialises openers (so DEFERRED is acceptable here), or (b) change the init code to `conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)` and keep the prose. Pick one; do not ship the contradiction.

### R2-T9 — `ExifOrientation::from_tag` rustdoc lies about which error variant it returns

**Agents**: gp + com (2-way CRITICAL — converged)

`crates/photohelper-core/src/model.rs:374-378` rustdoc `# Errors`: *"`Error::Exif` for tag values outside 1..=8."* The R1.T11 remediation changed the return at line 388 to `Error::InvalidExifOrientationTag { tag: other }`. The `Error::Exif` variant still exists, so a caller writing `match err { Error::Exif { .. } => ... }` based on this docstring would silently miss the orientation-tag case. R1.T11's own remediation note (`session-01-round1.md:113-114`) said to update the docstring; that edit didn't land.

**Remediation (R2)**: change to `/// - Error::InvalidExifOrientationTag { tag } for values outside 1..=8.`

### R2-T10 — Plan v5 not amended for the R1 scope-tightenings (indicatif, rusqlite, with_context, MtimeFacts) — plan-vs-code drift unresolved

**Agents**: gp (CRITICAL — only agent on this axis)

`docs/plans/session-01.md:116-117` still commits *"`indicatif` spinner (not progress bar — `par_bridge` is lazy)"* as a session-01 deliverable. Plan dep table at line 596 lists `indicatif | 0.17`. R1.T8 remediation removed the dep + the deliverable; the plan was NOT amended. Similarly: plan line 601 commits `rusqlite | 0.40` (shipped 0.32, tracked TD-002 ✓); plan's `with_context` discipline implied by R1.T10 changes no longer applies (`ContextForPath` was deleted). A future session reading "what was promised in session 01" sees an outdated contract.

**Remediation (R2)**: add a "Post-R1 amendments" section at the end of `docs/plans/session-01.md` listing (a) indicatif dropped (T8), (b) rusqlite 0.32 instead of 0.40 (T5 / TD-002), (c) per-photo `with_context` dropped via `ContextForPath` deletion (T10), (d) MtimeFacts deferred (T13 / DN-011), (e) ~12 plan test rows deferred per DN-008. Cross-link ADR-0001 for MSRV.

### R2-T11 — `op: "mkdir-p"` misnaming at `catalog.rs:111` (sibling of R1.T10's already-fixed `op: "stat"` mis-tag)

**Agents**: test (CRITICAL — only agent who found the sibling)

`crates/photohelper-catalog/src/catalog.rs:109-113`: when `File::create(&lock_path)` fails (read-only parent, permission denied), the `op:` tag at line 111 reads `"mkdir-p"` — that's the tag for the `create_dir_all` at line 96, NOT the lock-file create. R1.T10 fixed `op: "stat"` → `op: "file-lock"` for the `try_lock` failure path at line 143 but missed this sibling at line 111. Operators debugging a lock-file-create failure would be misdirected to look at directory-creation logic.

**Remediation (R2)**: change `op: "mkdir-p"` → `op: "lock-file-create"` at line 111. Add a regression test (parent dir read-only on Unix; skip on Windows) so the tag is pinned. **Bonus**: all FOUR R1.T10 WARN paths (`build_global`, `wal_checkpoint`, heartbeat death, `file-lock`) currently have **zero test coverage** — see R2-T18.

### R2-T12 — `--strict` is fail-open when EXIF is entirely missing

**Agents**: USER (production trace; flagged by Claude during R2)

User ran `photohelper ingest /Users/ph/Pictures/tests --strict` with 371 real Canon R8 CR3s. Summary: `unknown-camera: 0, errored: 0`. `--strict` is supposed to fail on unknown camera — but when EXIF is entirely missing, `camera_id` ends up `None` (`ingest.rs:333`), the `unknown_camera` branch at line 319 is bypassed, and `--strict` never triggers. The strict check catches *"EXIF parsed, but Make/Model don't match a profile"* and silently allows *"no EXIF at all, no idea what camera this is."* This is operationally identical to the situation `--strict` was meant to catch, but with a worse failure mode (silent success).

**Remediation (R2)**: in `--strict` mode, treat `camera_id == None` AND `exif.is_empty()` as a strict failure too — the user explicitly asked for "fail on unrouted photos," and a photo with no EXIF is the maximally-unrouted case. Add an integration test asserting `--strict` exits non-zero on a no-EXIF fixture.

### R2-T13 — DN-006 extends to REAL Canon R8 CR3s (not just synthetic fixtures)

**Agents**: USER (production trace; flagged by Claude during R2)

DN-006 was scoped to *"synthetic 0xCC-byte test fixtures cannot be parsed by kamadak-exif."* User's prod run shows **371/371 real Canon R8 CR3s also fail** with `"Unknown image format"`. This means:
- Session 02's LibRaw FFI is not optional — it's the only path to working EXIF for any CR3.
- DN-006's binding trigger ("real CR3 fixtures via git-lfs in session 02") needs upgrading: even with real fixtures, kamadak-exif will fail; we need LibRaw EXIF *first*.
- Plan v5's "DN-006 fallback" assumption — baked into ~23 places — quietly stops being a "best-effort fallback" and becomes "the actual production behavior for all CR3 files" until LibRaw lands.
- The DN-006 fallback is implicit blanket coverage; production usability of `ingest` for CR3 is bounded by LibRaw delivery.

**Remediation (R2)**: file `DN-011 — DN-006 extends to all real Canon R8 CR3s` cross-referencing this prod trace. Upgrade DN-006's binding trigger to *"session 02 must ship LibRaw EXIF extraction; kamadak-exif fallback is non-functional for CR3 in v0.1."* Update `docs/plans/session-02.md` (when authored) to elevate LibRaw EXIF from "RAW pixel decode" to "EXIF read + RAW pixel decode."

---

## HIGH

### R2-T14 — Heartbeat test is meaningless (now CRITICAL R2-T6); leave HIGH placeholder removed.

*(consolidated into R2-T6 above)*

### R2-T15 — `Catalog::open_with_retry_delay` is dead public API; zero tests use it

**Agents**: gp + test (2-way HIGH)

`crates/photohelper-catalog/src/catalog.rs:82-87` exposes `pub fn open_with_retry_delay(...)` behind `#[doc(hidden)]`. The only caller is the production `Catalog::open` at line 77 with the production `LOCK_RETRY_DELAY` constant. **Zero tests use the helper.** Plan v5 row 13 (cross-process file-lock test) is what this helper exists to enable; that test is on DN-008's deferral list.

The doc-comment at `catalog.rs:33-34` LIES: *"Tests override via the public-but-`#[cfg(test)]`-only `with_retry_delay` constructor helper."* It is `pub`-but-`#[doc(hidden)]`, NOT `#[cfg(test)]`-only, AND the function name is `open_with_retry_delay` not `with_retry_delay`. R1's R2 watch-list claim of "2 of 4 knobs landed" is misleading: this knob exists but is unused (and the env-var override per R2-T6 has meaningless coverage).

**Remediation (R2)**: either (a) write the missing cross-process file-lock test (row 13) using `open_with_retry_delay` with a short delay, OR (b) delete the helper from the public API and amend DN-008 to acknowledge row 13 is unimplemented. The current state is dead-code-shipped-as-if-it-fixed-something.

### R2-T16 — DN-008 cites the deleted `.with_context()` boundary as load-bearing

**Agents**: gp + test (2-way HIGH)

`docs/discovery-notes.md:76` claims *"the per-photo `.with_context()` boundary all rely on convention — a future refactor that drops the ROLLBACK or the with_context will not fail any test."* Code reality: `grep with_context crates/photohelper-cli/src/commands/ingest.rs` returns only TWO sites — root canonicalize (`:95`) and catalog open (`:104`). NO per-photo `.with_context` exists; R1.T10 deleted `ContextForPath`. DN-008's "boundary" doesn't exist; the document misleads about what session 02 inherits.

DN-008 also misrepresents row-48 status: claims `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` "closes test row 48 in a weakly-deterministic way." Per R2-T6, that test does not exercise heartbeat behavior at all. Row 48 belongs on the deferred list; the binding trigger row enumeration (`{6, 12, 13, 14, 17 missing, 18, 19, 34, 39, 42, 43, 49}`) is also internally inconsistent (see R2-T22).

**Remediation (R2)**: rewrite DN-008's "Why it matters" to say *"per-photo errors rely on structured `Error::Io { path }` + `Error::CatalogInsert { photo_id }` variants for context (no `.with_context` is attached post-T10 ContextForPath deletion)."* Add row 48 to the binding-trigger row list and acknowledge the helper-without-consumer status.

### R2-T17 — T13 / T15 / heartbeat-join deferrals shipped without DN/TD entries (policy violation)

**Agents**: gp + type + test + simp (4-way HIGH)

Three deferrals lack the TD/DN entries `CLAUDE.md § No Acceptable Trade-offs Policy` mandates:
- **T13 (`MtimeFacts` newtype)**: SESSION-STATE.md:88-89 says "deferred to session-02 watch (small)." No DN, no TD. The 7-param `Photo::from_filesystem` constructor at `model.rs:484-492` retains the transposable `(i64, bool)` args.
- **T15 (minor polish)**: SESSION-STATE.md:93. KnownCamera Display impl, workspace clippy allow-list comments, Windows case-sensitivity, `UpsertOutcome` `#[non_exhaustive]` — all silently dropped.
- **Heartbeat-join (R1.T2 sub-(c))**: `ingest.rs:181-184` documents the deferred join with comment that under-specifies the three consequences R1.T2 named (zombie stderr output, test-flake risk, in-process thread accumulation).

**Remediation (R2)**: file `DN-011` (T13 + MtimeFacts), `DN-012` (T15 polish), and `TD-003` (heartbeat-join leak) each with binding triggers (e.g., "before session 02 lands new callers" or "by 2026-08-01"). Extend the heartbeat comment to enumerate the three consequences.

### R2-T18 — All 4 R1.T10 remediation WARN paths have zero test coverage

**Agents**: test (HIGH — only agent who counted)

R1.T10 added four new `tracing::warn!` arms that are runtime-observable but completely untested:
- `ingest.rs:117-122` — rayon `build_global()` Ok/Err
- `ingest.rs:175-180` — heartbeat death-WARN
- `catalog.rs:240-251` — `wal_checkpoint` recovered/clean/Err branches
- `catalog.rs:141-145` — `op: "file-lock"` tag (and per R2-T11, also the sibling `op: "lock-file-create"` once renamed)

Each was a CRITICAL silent-failure cluster in R1. Each is now "fixed" by adding a WARN call. A refactor that drops any of these (or reverts to silent `let _ = ...` or `unwrap_or(0)`) would not fail any test. The R1.T10 fixes are unprotected from regression.

**Remediation (R2)**: at minimum, add a regression test for `build_global` already-initialized (run `ingest` twice in the same process) AND `wal_checkpoint` recovered (write, kill, re-open). For heartbeat death, the cleanest shape is a `cfg(test)`-only `panic_for_testing` knob (DN-008 deferred). Negative assertions on the existing idempotency test can pin the WARN-doesn't-fire-spuriously invariant.

### R2-T19 — 128KB PhotoId test does NOT discriminate the R1.T3 fix

**Agents**: arch + test (2-way HIGH)

`crates/photohelper-core/src/model.rs:762-783` (`photoid_derive_window_disjoint_for_files_exactly_128k`) uses 128KB all-`0xAA` content. Both the BUGGY pre-R1.T3 and the FIXED post-R1.T3 code feed identical bytes to BLAKE3 for this size:
- OLD overlap logic: head [0..65536), tail seeks End(-65536) at offset 65536 → [65536..131072). No overlap, no gap.
- NEW disjoint logic: head [0..65536), `tail_start = max(65536, 65536) = 65536`, tail [65536..131072). Same.

The test passes under both implementations. **Only the 100KB test actually discriminates** the regression. The 128KB test's comment claims it "exercises the boundary" — but 128KB is the ONE size at which there is no boundary difference.

**Remediation (R2)**: replace the 128KB test with a 96KB test where the byte sequences differ between the buggy and fixed paths (e.g., fill bytes `[60KB..68KB)` with a distinct pattern so the buggy code's double-hash of `[36864..65536)` produces a different digest from the disjoint code's single-hash). OR rename the existing test to `photoid_derive_at_128k_boundary_does_not_panic` and add an explicit comment that it does NOT pin the disjoint invariant.

### R2-T20 — `Error::InvalidExifOrientationTag` is unreachable from any operator-visible surface

**Agents**: type + sfh (2-way HIGH)

The R1.T11 fix added the dedicated variant carrying the offending tag (no more `PathBuf::new()` sentinel). But the sole production caller at `ingest.rs:418` discards it via `if let Ok(orientation) = ExifOrientation::from_tag(...)`. If a real CR3 contains orientation tag 9, no WARN fires, no counter increments, no diagnostic is emitted. R1.T11's "actionable diagnostic context" is operator-invisible. **Net effect of R1.T11 on operator-visible behavior: zero.**

**Remediation (R2)**: replace the `if let Ok` with an explicit match that logs the failure path:
```rust
exif::Tag::Orientation => {
    if let Some(tag) = field.value.get_uint(0) {
        match ExifOrientation::from_tag(i64::from(tag)) {
            Ok(o) => out.orientation = Some(o),
            Err(Error::InvalidExifOrientationTag { tag }) => {
                tracing::warn!(tag, path = %path.display(),
                    "EXIF orientation tag out of range; treating as missing");
            }
            Err(e) => return Err(e),
        }
    }
}
```

### R2-T21 — `Photo::from_filesystem` accepts unverified `(photo_id, file_size, clamped_mtime)` triples — silent PhotoId-forgery surface

**Agents**: type (HIGH — only agent)

`crates/photohelper-core/src/model.rs:484-512` enforces `file_size > 0` plus path canonicality, but does NOT verify that the supplied `PhotoId` was actually derived from the supplied `(canonical, file_size, clamped_mtime)`. The single in-tree caller (`ingest_one`) currently passes correct values, but a future second caller (or a transposition of args 3/4) can mint a `Photo` whose `photo_id` doesn't match its content. That `Photo` flows into `Catalog::upsert`, writing a row whose PK has nothing to do with the file's bytes, silently breaking de-duplication and the supersede invariant.

**Remediation (R2)**: at minimum add a `debug_assert_eq!(photo_id, PhotoId::derive_with_clamped_mtime(canonical.as_path(), file_size, clamped_mtime_unix_seconds).unwrap_or(photo_id))` so transpositions are caught in dev builds. Better: gate the unsafe variant behind `pub(crate) fn from_validated_facts` only callable inside the workspace; expose `pub fn from_filesystem(canonical, exif, camera_id)` that re-derives internally.

### R2-T22 — Triple count drift on "uncovered plan rows" (R1=12, R1 body=13, DN-008=10/11)

**Agents**: com (HIGH — only agent who counted)

Three documents disagree on the same scalar:
- `docs/code-reviews/session-01-round1.md:192` (T7 title) says "12 plan rows uncovered"; body lines 209-217 enumerate rows `{6, 12, 13, 14, 17, 18, 19, 34, 39, 42, 43, 48, 49}` = **13 rows**; remediation says "the 13 missing test rows."
- `docs/discovery-notes.md:73` (DN-008 title) lists `{6, 12, 13, 14, 18, 19, 34, 39, 42 partial, 43 partial, 49}` = **11 entries** (row 17 hardlink and row 48 omitted); body says "9 other plan rows" = 10; binding trigger lists `{6/12/13/14/18/19/34/39/42/43/49}` = 11.
- `SESSION-STATE.md:74` says "remaining 12 uncovered plan rows tracked in DN-008."

Row 17 (hardlink) is in R1 but absent from DN-008; row 48 is in R1 but should be removed if env-var override is accepted as coverage (yet per R2-T6 it isn't).

**Remediation (R2)**: pick a canonical row list, update all three documents in one commit, explain the row-17 + row-48 disposition explicitly.

### R2-T23 — R1 summary table count "7C+5H+4M+3L=19" non-reconstructible from body

**Agents**: com (HIGH — only agent who recounted)

`session-01-round1.md:14-20` claims 7 CRITICAL + 5 HIGH + 4 MEDIUM + 3 LOW. Counting themes by their bracketed `### T*` headers: T1-T7=7 CRITICAL ✓; T8-T11=**4** HIGH (not 5); T12+T13=2 MEDIUM, T14="MEDIUM/LOW"=3-ish (not 4); T15=1 LOW (not 3). The summary appears to count per-agent severity flags inside themes (e.g., T15 cites "type HIGH KnownCamera + rev HIGH op tag + simp LOW") rolled into a `[LOW]` umbrella — making totals non-reproducible without re-reading every theme. HANDOFF Checkpoint 2:165 + SESSION-STATE:61-62 propagate the drift.

**Remediation (R2)**: decide whether to count by theme or by agent-flag; rewrite the summary table with one consistent rule; reconcile T15 (its two HIGH findings should arguably be their own [HIGH] theme rather than a [LOW] umbrella).

### R2-T24 — `eight-agent-review/SKILL.md` `allowed-tools` does NOT include `AskUserQuestion`

**Agents**: com (HIGH — only agent who diffed frontmatter vs body)

`.claude/skills/eight-agent-review/SKILL.md:5` declares `allowed-tools: Read Grep Glob Agent Write Edit Bash(test *) Bash(cat *) Bash(mkdir *)`. The skill body instructs Claude to fire `AskUserQuestion` at §0.b, §1, and §6 — but the tool is NOT in the allow-list. The harness-sync commit message markets §0 as the headline upgrade. *(Note: the gate DID fire successfully in this very session via harness fallback, so the skill works in practice — but the frontmatter contract is wrong.)*

**Remediation (R2)**: add `AskUserQuestion` to the `allowed-tools` frontmatter line. Spot-check the other skills (`session-start`, `session-end`, `plan-review`, `session-pause`) for the same omission.

### R2-T25 — HANDOFF Checkpoint 1 overstates `photohelper-core::model` test count (33 claimed, 30 actual)

**Agents**: com (HIGH — only agent who counted)

`HANDOFF_REPORT.md:69-70` reads *"`photohelper-core::model` (~1000 lines, 33 unit tests)."* Actual: `wc -l model.rs` = 989 (acceptable), `grep -c '#[test]' model.rs` = **30** unit tests in `model.rs` (32 in the whole crate including error.rs).

**Remediation (R2)**: change to *"30 unit tests in model.rs (32 across the crate)."*

### R2-T26 — `photohelper-core` declares unused `kamadak-exif` + `tracing` deps; breaks "core → ⊥" strength claim

**Agents**: arch (HIGH — only agent on this axis)

`crates/photohelper-core/Cargo.toml:19-20` lists `kamadak-exif.workspace = true` and `tracing.workspace = true`. `Grep` for `use exif|exif::|use tracing|tracing::` in `crates/photohelper-core/src/` returns **zero matches**. The R1 strengths NOTE ("Crate DAG is clean ... `core → ⊥`. The `BoxedSourceError` keeps `core` storage-agnostic; `rusqlite` does NOT leak into `core`'s deps") implicitly relied on `core` being dependency-minimal. `kamadak-exif` is an encoding-format-specific parser — pulling it into `core` couples the domain crate to today's specific EXIF library.

**Remediation (R2)**: delete both lines from `crates/photohelper-core/Cargo.toml`. Add `unused_crate_dependencies = "warn"` to the workspace lints so the next inadvertent leak is caught at compile time.

### R2-T27 — `Error::Io` doc-comment enumerates op tags but omits new `"file-lock"` tag

**Agents**: gp (HIGH — drift-from-fix)

`crates/photohelper-core/src/error.rs:23-24`: *"IO failure with structured context. `op` tags include `\"canonicalize\"`, `\"canonicalize-nul-check\"`, `\"read-prefix\"`, `\"stat\"`, `\"mkdir-p\"`."* R1.T10 introduced the `"file-lock"` tag at `catalog.rs:143`. The doc was not updated. Same fix-but-don't-update-the-docs pattern as R2-T9.

**Remediation (R2)**: extend to `... "stat", "mkdir-p", "file-lock", "lock-file-create"` (the latter after R2-T11 lands).

---

## MEDIUM

| ID | Theme | Agents | Citation | Remediation summary |
|----|-------|--------|----------|---------------------|
| R2-M1 | `do_insert` closure shadows outer `tx` param name | arch | `catalog.rs:335-356` | Rename closure param `tx` → `txn`, or hoist to free fn `insert_row` |
| R2-M2 | `Catalog::Send + Sync` `static_assertions!` only in `#[cfg(test)]` | arch | `catalog.rs:442-446` | Hoist assertion to module scope (zero-cost) |
| R2-M3 | Hook `detect-eng-protocol.sh` dumps unbounded worktree list + no timeout | arch | `.claude/hooks/detect-eng-protocol.sh:22-29` | Cap at 10 entries + `timeout 2s` |
| R2-M4 | `hook` missing `set -o pipefail` (defensive future-proofing) | rev | `.claude/hooks/detect-eng-protocol.sh:13` | `set -uo pipefail` |
| R2-M5 | `PhotoRow.mtime_anomalous: i64` + `exif_orientation: Option<i64>` leak wire-format types | type | `crates/photohelper-catalog/src/row.rs:22,36` | Convert at `from_row` boundary to `bool` + `Option<ExifOrientation>` |
| R2-M6 | `IngestStats` 11 `pub` `AtomicU64` fields — no encapsulation | type | `ingest.rs:42-54` | Make `pub(self)` + add `pub(crate) fn incr_*` methods |
| R2-M7 | `metadata.modified().ok()` silently maps to 0 → spurious anomalous flag | sfh | `ingest.rs:282-287` + `model.rs:67-74` | Explicit match logging the FS-error before fallback |
| R2-M8 | `let _ = conn.execute("ROLLBACK", [])` swallows DB-corruption signal | sfh | `catalog.rs:281` | Pattern-match to discard only `no-active-transaction`; log others |
| R2-M9 | Per-file WARN uses `%err` Display — loses error chain | sfh | `ingest.rs:162` | `error = ?err` (Debug, shows chain) |
| R2-M10 | `PhotoId` struct docstring still claims `last 64KB` post-T3 fix | com + test | `model.rs:32-44` | Reword to disjoint-window invariant |
| R2-M11 | `MtimeFacts` deferred without binding trigger (T13 sub) | type + simp | SESSION-STATE.md:88 | File DN-011 with binding trigger; OR land the 10-line cleanup now |
| R2-M12 | Decision doc 0001 ownership prose internally contradictory (v1→v2 ownership unclear) | com | `docs/decisions/0001-catalog-schema-v1.md:4 vs :122-132` | Add one-sentence clarification |

---

## LOW

| ID | Theme | Agents | Citation | Note |
|----|-------|--------|----------|------|
| R2-L1 | `_is_known: bool` destructured-then-discarded at catalog.rs:321 | simp | `catalog.rs:321-324` | Drop the tuple wrapper; return `Option<String>` directly |
| R2-L2 | `INSERT_PHOTO_SQL` doc-comment includes implementation-history noise ("Extracted in R1.T14") | simp | `catalog.rs:22-31` | Trim doc-comment; `git blame` already tells the story |
| R2-L3 | `.claudeignore` 28 lines duplicates `.gitignore` patterns | simp | `.claudeignore` | Keep only 5 new patterns + leading comment "extends `.gitignore`; do not duplicate" |
| R2-L4 | `Cargo.toml` commented-out `# indicatif = "0.18"` invites uncomment | simp | `Cargo.toml:28-33` | Delete commented line; keep past-tense rationale |
| R2-L5 | `usize::try_from(...).unwrap_or(HASH_WINDOW_BYTES)` is dead defensive code | arch | `model.rs:104, 110` | Either `.expect()` with rationale OR add explaining comment |
| R2-L6 | TD-002 binding trigger date-only (2026-08-01) is "too loose" given CVE surface | simp | `TECH-DEBT.md:51` | Tighten to "first session-02 commit touching catalog crate" OR bump rusqlite now |
| R2-L7 | TD-002 line ref `Cargo.toml:49` drifted to `:54` post-R1.T8 | com | `TECH-DEBT.md:49` | Add `(line 54 at HEAD)` parenthetical, OR drop the line number entirely |

---

## Strengths preserved [NOTES]

Confirmed by R2 — must not regress in any R3:
- **Crate DAG is clean (modulo R2-T26)** [arch]: `cli → core, catalog, cameras; catalog → core; cameras → core; core → ⊥`. No back-edges in the diff. Once R2-T26 lands (delete unused `kamadak-exif`+`tracing` from core), the "core → ⊥" claim becomes true again.
- **`catalog_glue::photo_id_from_row_bytes` remains THE sole public route** for raw-byte `PhotoId` reconstruction [type]. Forgery surface from R3.T2 still closed.
- **No production `unwrap`/`expect`/`panic`** [rev]: every `.unwrap()` is `#[cfg(test)]`-gated; workspace lints enforce; `-D warnings` in CI escalates. R1 remediation did not introduce new panic sites.
- **No SQL injection** [rev]: every `execute`/`query_row` uses `params![...]`; `INSERT_PHOTO_SQL` const is static literal.
- **Error discipline holds** [rev]: no `#[from]` derives; `Error::InvalidExifOrientationTag` follows explicit `.map_err`/return pattern.
- **PhotoId disjoint-window math is correct** [arch, rev, test]: traced through `<64KB`, `=64KB`, `100KB`, `=128KB`, `>128KB` regimes. The 100KB regression test pins the invariant correctly (the 128KB test does NOT — see R2-T19, but the math itself is sound).
- **Schema decision doc SQL matches `schema.rs` byte-for-byte** [com]: column list, defaults, indexes, `PRAGMA user_version = 1` all consistent. The drift is in the prose around transaction behavior (R2-T8), not the SQL.
- **MSRV ADR + governance files consistent at 1.88** [com]: `rust-toolchain.toml`, `Cargo.toml:17`, `CLAUDE.md:83-86`, `stacks/rust.md:15,32-33` all reference 1.88.0 with cross-links. The drift is in the ADR's technical claim about *which* `time::*` function (R2-T7), not the version.
- **TD-002 + DN-007 cross-reference policy-compliant** [com]: all required fields present.

---

## New bug classes surfaced [NOTES]

R2 surfaced six structural patterns worth recording for future plan-reviews and session-end reviews:

1. **R1-remediation-introduced regressions inside the very files being fixed** [gp, com, sfh, test]: the remediation commit `0f28627` introduced new bugs in the files it was fixing — fresh `query_row(...).ok()` calls (R2-T3), 3+ doc-vs-code drifts (R2-T9, R2-T16, R2-T20, R2-T27), the 128KB test that doesn't discriminate (R2-T19), the lying WARN (R2-T5). Mitigation: future R2 watch-lists should include "for each fix, check the surrounding doc-comments named, used, or referenced by the changed symbol" as a sub-step.

2. **Dead-by-default typed-error / outcome variants** [arch, type, sfh]: `Error::InvalidExifOrientationTag` (R1.T11 added, no caller routes on it) and `IngestOutcome::NoExifFields` (defined, exhaustively matched, never constructed) share the anti-pattern. Future plan-reviews should ask, for every newly-introduced enum variant: *"name at least one production caller that pattern-matches on it; if none, the variant is documentation-only and should be either consumed or removed."* Worth a one-line addition to `docs/quality-assurance.md § Type-design checklist`.

3. **"Unconditional-summary-line as substitute coverage"** [test]: integration tests assert on a substring the binary prints regardless of whether the path executed (R2-T6). New convention worth recording in `docs/testing-standards.md`: *integration tests targeting added-stderr-output features MUST assert on a substring unique to the new output (`[heartbeat]`, `[progress]`, JSON event keys), not the unconditional summary.*

4. **`query_row(...).ok()` as SQL-error coalescer** [sfh]: `rusqlite` has no `query_row_optional` so the idiomatic-but-wrong shortcut is `.ok()`. Two of three production sites use it. Future audits should sweep for `query_row(...).ok()` and `execute(...).ok()` as their own category. Candidate: workspace clippy custom-lint or `xtask` grep gate.

5. **"Decision-doc enshrines wrong invariant"** [com]: when a session is asked to write the authoritative spec for code that already exists, the temptation is to write what the code *should* do rather than what it *does*. Decision doc 0001 was supposed to close DN-005 by discovering truth from `schema.rs`; instead it propagated `schema.rs`'s pre-existing wrong `BEGIN IMMEDIATE` claim into a higher-authority document. Future plan-reviews of `docs/decisions/` artifacts should require *every claim about runtime behavior to be substantiated by `// SAFETY:`-style cross-reference to the exact line + function in the implementation, and the reviewer must read both ends.*

6. **"Discipline-via-skill-prose inflation"** [simp]: the harness sync inflated `eight-agent-review/SKILL.md` to ~250 lines of policy across 3 YAML schemas + 5 error-handling cases. The skill body is now larger than `ingest_one` (the thing it governs). Photohelper now owes itself the `scripts/verify-review-artifact.sh` enforcer (DN-009) just to make the new markers load-bearing. Treat DN-009 as a hard prerequisite for adding any FURTHER YAML schema to the skill, OR collapse §0+§1 into a single `preflight` block (see R2-T17's H1 from Agent 8).

---

## Disposition summary

| Disposition | Count | Notes |
|-------------|------:|-------|
| **Fix inline in R2 remediation** | 12 CRITICAL + 11 HIGH + 6 MEDIUM | All code-bearing + load-bearing doc fixes |
| **Verify-and-close (no code change)** | 1 CRITICAL (R2-T1) | Magic-byte TOCTOU is a false positive |
| **File DN/TD with binding trigger; remediate later** | 1 CRITICAL (R2-T13) + 3 HIGH (R2-T17) + 4 MEDIUM | Includes DN-011 for DN-006-extends-to-real-CR3 + TD-003 for heartbeat-join |
| **Defer to session 02 with explicit DN cross-ref** | 1 HIGH (R2-T18 if not landed inline) + 2 MEDIUM + 4 LOW | Test infrastructure knobs, type-design polish |
| **Accept as-is with explicit comment** | 3 LOW | Walker case-sensitivity (v0.2), TD-002 line-ref drift |

If R2 remediation surfaces CRITICAL-class regressions → fire R3.

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 22
  verified: 21
  drifted: 1
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    22 high-impact CRITICAL+HIGH findings verified by 9th-agent. MEDIUM/LOW
    findings (~19 items) NOT individually 9th-agent-verified for cost reasons;
    they carry direct file:line citations from the original agent reports and
    can be spot-checked manually during R2 remediation. The single drift was
    R2-T25 (HANDOFF "33 unit tests" claim) — the citation line is correct;
    only the asserted count is wrong (actual 30). Magic-byte TOCTOU finding
    (R2-T1) was additionally verified by the orchestrator via direct Read of
    catalog.rs:108-180 since it reframes 5 prior-agent CRITICAL claims.
  details:
    - {finding_id: A, file: crates/photohelper-catalog/src/catalog.rs, line: 151, present: yes, retain: yes, reason: "Lock acquired via Ok(()) => break at line 121 BEFORE if catalog_path.exists() at line 151; Agent 6's not-a-bug determination is correct.", evidence_snippet: "if catalog_path.exists() {"}
    - {finding_id: B-model, file: crates/photohelper-core/src/model.rs, line: 611, present: yes, retain: yes, reason: "Variant NoExifFields defined as cited.", evidence_snippet: "NoExifFields,"}
    - {finding_id: B-ingest, file: crates/photohelper-cli/src/commands/ingest.rs, line: 241, present: yes, retain: yes, reason: "Dispatch arm exists but ingest_one never returns NoExifFields; arm is dead.", evidence_snippet: "IngestOutcome::NoExifFields => {"}
    - {finding_id: C, file: crates/photohelper-catalog/src/catalog.rs, line: 295, present: yes, retain: yes, reason: ".ok() at both lookup sites swallows all rusqlite::Error as NotFound semantics.", evidence_snippet: ".ok();"}
    - {finding_id: D, file: crates/photohelper-cli/src/commands/ingest.rs, line: 205, present: yes, retain: yes, reason: "100ms granularity hardcoded; env-var floor 10ms ineffective below granularity.", evidence_snippet: "let granularity = Duration::from_millis(100);"}
    - {finding_id: E, file: crates/photohelper-cli/src/commands/ingest.rs, line: 306, present: yes, retain: yes, reason: "is_empty() WARN fires unconditionally; on parse-failed path the WARN message lies.", evidence_snippet: "if exif.is_empty() {"}
    - {finding_id: F, file: crates/photohelper-cli/tests/cli.rs, line: 448, present: yes, retain: yes, reason: "Test name promises heartbeat coverage; assertion verifies unconditional summary line.", evidence_snippet: "assert.stderr(contains(\"walked: 1\"));"}
    - {finding_id: G, file: docs/adr/0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md, line: 11, present: yes, retain: yes, reason: "ADR cites time::format_description::parse; advisory is in value-parsing entry points (RFC-2822 path).", evidence_snippet: "denial-of-service in `time::format_description::parse` that affected"}
    - {finding_id: H-doc, file: docs/decisions/0001-catalog-schema-v1.md, line: 18, present: yes, retain: yes, reason: "Decision doc enshrines BEGIN IMMEDIATE for init.", evidence_snippet: "Ship the schema below as `PRAGMA user_version = 1`. Wrap the init in"}
    - {finding_id: H-code, file: crates/photohelper-catalog/src/catalog.rs, line: 209, present: yes, retain: yes, reason: "Init uses conn.transaction() = default BEGIN DEFERRED; contradicts decision doc.", evidence_snippet: "let tx = conn.transaction().map_err(|e| Error::CatalogOpen {"}
    - {finding_id: I, file: crates/photohelper-core/src/model.rs, line: 374, present: yes, retain: yes, reason: "Docstring says Error::Exif; impl returns Error::InvalidExifOrientationTag post-R1.T11.", evidence_snippet: "/// - `Error::Exif` for tag values outside 1..=8."}
    - {finding_id: J, file: docs/plans/session-01.md, line: 116, present: yes, retain: yes, reason: "Plan v5 still lists indicatif as deliverable; not amended for R1.T8 scope tightening.", evidence_snippet: "`indicatif` spinner (not progress bar — `par_bridge` is lazy)."}
    - {finding_id: K, file: crates/photohelper-catalog/src/catalog.rs, line: 111, present: yes, retain: yes, reason: "File::create lock-file failure tagged op:\"mkdir-p\" — sibling of R1.T10's already-fixed file-lock tag.", evidence_snippet: "op: \"mkdir-p\","}
    - {finding_id: L, file: crates/photohelper-cli/src/commands/ingest.rs, line: 418, present: yes, retain: yes, reason: "if let Ok(orientation) silently discards Error::InvalidExifOrientationTag.", evidence_snippet: "if let Ok(orientation) = ExifOrientation::from_tag(i64::from(tag)) {"}
    - {finding_id: M, file: crates/photohelper-core/src/model.rs, line: 762, present: yes, retain: yes, reason: "128KB test with all-0xAA bytes; buggy and fixed code produce identical hash at this size.", evidence_snippet: "fn photoid_derive_window_disjoint_for_files_exactly_128k() {"}
    - {finding_id: N, file: crates/photohelper-catalog/src/catalog.rs, line: 82, present: yes, retain: yes, reason: "pub fn open_with_retry_delay declared with #[doc(hidden)]; Grep finds zero test callers.", evidence_snippet: "    pub fn open_with_retry_delay("}
    - {finding_id: O, file: docs/discovery-notes.md, line: 76, present: yes, retain: yes, reason: "DN-008 cites .with_context() boundary; ContextForPath was deleted in R1.T10; no per-photo .with_context exists in ingest.rs.", evidence_snippet: "the per-photo `.with_context()` boundary all rely on convention"}
    - {finding_id: P, file: crates/photohelper-core/src/error.rs, line: 23, present: yes, retain: yes, reason: "Error::Io doc op-tag list omits the new file-lock tag added in R1.T10.", evidence_snippet: "/// IO failure with structured context. `op` tags include `\"canonicalize\"`,"}
    - {finding_id: Q, file: crates/photohelper-core/Cargo.toml, line: 19, present: yes, retain: yes, reason: "kamadak-exif declared but Grep across crates/photohelper-core/src/ returns zero use sites; same for tracing.", evidence_snippet: "kamadak-exif.workspace = true"}
    - {finding_id: R, file: HANDOFF_REPORT.md, line: 69, present: drifted, retain: yes-with-corrected-line, reason: "Line ref correct; the COUNT is wrong — HANDOFF says 33 unit tests, actual grep returns 30 in model.rs (32 across crate).", evidence_snippet: "`photohelper-core::model` (~1000 lines, 33 unit tests): `PhotoId`"}
    - {finding_id: S, file: .claude/skills/eight-agent-review/SKILL.md, line: 5, present: yes, retain: yes, reason: "Frontmatter allowed-tools does NOT include AskUserQuestion; skill body invokes it at §0.b, §1, §6.", evidence_snippet: "allowed-tools: Read Grep Glob Agent Write Edit Bash(test *) Bash(cat *) Bash(mkdir *)"}
    - {finding_id: T, file: TECH-DEBT.md, line: 51, present: yes, retain: yes, reason: "TD-002 binding trigger 2026-08-01 OR before session 02 schema columns; Agent 8 calls date-fallback too loose.", evidence_snippet: "- **Binding trigger**: bump by **2026-08-01** OR before session 02 introduces new catalog schema columns (whichever first)."}
```

---

## R3 trigger

Per `docs/quality-assurance.md § Double-review protocol`: **fire R3 if R2 remediation surfaces a CRITICAL-class regression** (e.g., the `BEGIN IMMEDIATE` fix breaks an integration test, the `query_row(...).ok()` cleanup deletes a needed `.ok()` elsewhere, the `IngestOutcome::NoExifFields` removal silently re-routes some code). For MEDIUM/LOW-only regressions, ship Round 2 remediation and merge.

---

## Acknowledgements

The most valuable single finding of R2 came from the **user's production run** against 371 real Canon R8 CR3s (`/Users/ph/Pictures/tests`, 2026-05-28 15:32:52). That run independently surfaced R2-T5 (lying WARN), R2-T12 (--strict fail-open), and R2-T13 (DN-006 extends to real CR3s) — all of which the silent-failure-hunter then independently confirmed. The single-best lesson is the third: kamadak-exif is non-functional for ANY CR3 in v0.1, not just synthetic fixtures, which makes LibRaw EXIF a session-02 critical-path dependency rather than an optional enhancement.
