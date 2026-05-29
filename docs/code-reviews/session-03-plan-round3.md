# Session 03 — plan review, Round 3

> Per `docs/quality-assurance.md § Plan-review protocol`.
> Cadence A → Tier 5 (plan stage), full 8-agent suite fired in parallel against
> `docs/plans/session-03.md` v3 (committed at `285675e`).
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

## R3 watch-list from Round 2 — all 11 items verified

| # | Item | Status |
|---|------|--------|
| 1 | T1: CI `pip install onnx` + `sanitize-check.sh` gate | PASS |
| 2 | T2: D5c drops `force_heartbeat_panic_in_thread`; `HeartbeatDeathTrigger` present | PASS (D5c-i/ii restructured correctly; D5e subprocess variant is T-β below) |
| 3 | T3: `CullStats` uses `AtomicU64` + `Arc<CullStats>` + `Ordering::Relaxed` | PASS |
| 4 | T4: DN-022 references corrected in plan body | PASS (TECH-DEBT.md residual is T-ι) |
| 5 | T5: `VerifiedModelBytes` wraps `Arc<[u8]>`; `from_verified` takes borrow | PASS |
| 6 | T6: `PhotoId::derive` + compare step BEFORE `read_raw_rgb` | PASS (pseudocode present; T-η flags the `?` compile error in it) |
| 7 | T7: FK violation dispatch row added | PASS (row present; T-ζ flags missing counter in `CullStats` spec) |
| 8 | T8: `open_schema_version_too_new` test update specified | PASS |
| 9 | T9: Decision-doc 0001 §Migration policy amendment specified in D2c | PASS |
| 10 | T10: D0 ABORT for license/SHA-256 + `license:` in commit template | PASS |
| 11 | T13: Fixture-construction table for all 6 per-case tests | PASS |

---

## Triage summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 3 | Implementation cannot begin until all 3 are remediated in plan v4 |
| **HIGH**      | 4 | Address in plan v4 |
| **MEDIUM**    | 2 | Address in plan v4 |
| **LOW**       | 1 | One-line fix |

Agent suite: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer` (type),
`pr-review-toolkit:silent-failure-hunter` (sfh), `pr-review-toolkit:comment-analyzer`
(com), `pr-review-toolkit:pr-test-analyzer` (test), `pr-review-toolkit:code-simplifier`
(simp).

---

## CRITICAL

### T-α — `NimaScore: Ord` without `Eq` is a compile error; `Ord: Eq + PartialOrd` in Rust's type system (3-way)

**Agents**: gp (MEDIUM), arch (HIGH), type (CRITICAL)

`docs/plans/session-03.md:240` specifies `NimaScore` derives
`Copy + Clone + Debug + PartialOrd + Ord`. Line 244 says:
```
NOT `Eq` (f32 equality is floating-point, not natural equality).
```

Rust's trait hierarchy: `pub trait Ord: Eq + PartialOrd<Self>`. You cannot
implement `Ord` without implementing `Eq`. The compiler emits:
```
error[E0277]: the trait bound `NimaScore: Eq` is not satisfied
```

The rationale for omitting `Eq` does not hold for `NimaScore`: the constructor
rejects NaN (line 238: "reject NaN, ±∞, out-of-range"), so reflexivity holds
for all valid instances. Two `NimaScore` values with the same bit pattern are
equal; the [1.0, 10.0] range excludes -0.0. `Eq` is sound and required.

**Remediation**: Add `Eq` to the trait list at line 240:
```
Copy + Clone + Debug + PartialEq + Eq + PartialOrd + Ord
```
Remove the "NOT `Eq`" clause at line 244. Replace with: "NaN rejected at
construction guarantees reflexivity; `Eq` is required by `Ord` and is sound
for NaN-free f32 in [1.0, 10.0]."

---

### T-β — D5c/D5e HeartbeatDeathTrigger subprocess test structurally impossible; production binary has no `HeartbeatDeathTrigger` code (4-way)

**Agents**: gp (HIGH), rev (CRITICAL), test (CRITICAL), simp (HIGH)

`docs/plans/session-03.md:609-612`:
```
PHOTOHELPER_HEARTBEAT_POISON_TICKS=1 checked in the HeartbeatDeathTrigger
helper thread (NOT heartbeat_loop) ... The subprocess integration test spawns
`photohelper ingest` (or `cull`) with this env-var set.
```

`HeartbeatDeathTrigger` lives in `crates/photohelper-test-helpers` which is
`[dev-dependencies]`-only (confirmed by D5c E2E at lines 603-606 which verifies
this via `cargo metadata`). Dev-dependencies are NOT compiled into the
production binary. When the subprocess test spawns `photohelper ingest` with
`PHOTOHELPER_HEARTBEAT_POISON_TICKS=1`, the production binary has no code that
reads this env-var. The env-var is silently ignored. The `[heartbeat-death-WARN]`
never fires. The test fails or vacuously passes — either way, it does not
exercise the recovery path.

This is the same root tension that caused TD-005: any mechanism that causes the
production binary to respond to an env-var from test code is test-code-in-
production-code. The v3 plan avoids the anti-pattern for `heartbeat_loop` but
recreates it for the subprocess test integration approach.

The D5c-i/ii in-process approach IS correct and works (dev-dep code runs
inside `cargo test` test binaries). The D5e subprocess variant is the problem.

**Remediation**: Drop the subprocess variant for the heartbeat-death test.

- **D5c-ii** already specifies the correct in-process approach: spawn a
  `HeartbeatDeathTrigger` thread, signal it, verify via `JoinHandle::is_finished()`.
  Keep this.
- **D5e row 4**: change to an in-process test only (not subprocess). Verify
  `[heartbeat-death-WARN]` fires via a `tracing-subscriber` test layer capturing
  events, or by asserting on the `JoinHandle::is_finished()` state of the
  heartbeat `JoinHandle` after the trigger fires.
- The "parameterized over `[ingest, cull]`" means: two in-process tests (one
  calling `run_ingest` path's heartbeat scaffolding, one calling `run_cull`'s
  duplicate scaffolding). NOT subprocess.
- The three other D5e WARN regression tests (`build_global`, `wal_checkpoint`,
  `file-lock`) remain subprocess tests.
- Remove `PHOTOHELPER_HEARTBEAT_POISON_TICKS` from D5e (the env-var approach
  only makes sense for subprocess tests, which are now dropped for this case).

---

### T-γ — Per-worker ort Session construction frequency unspecified; risk of O(n_photos) × ~500ms overhead (3-way)

**Agents**: arch (HIGH), test (CRITICAL), sfh (HIGH)

`docs/plans/session-03.md:500-504`:
```
Each worker calls `LoadedModel::from_verified(&verified_bytes)` independently
to construct its own `ort::Session`
```

In rayon's `par_bridge().for_each(|row| { ... })`, the closure executes once
**per photo row**, not once per worker thread. Without an explicit `thread_local!`
or per-thread initialization mechanism, the natural reading of "each worker
calls `from_verified`" is one Session construction per photo. For 370 photos,
this is 370 × ~200-500ms = 74-185 seconds of pure Session-construction overhead
(model deserialization + graph optimization) — potentially dominating the entire
run. Acceptance criterion 3's 50% headroom SLO (line 773) may absorb this on
fast hardware, masking the regression.

Rayon does not expose a per-worker-thread lifecycle hook in `par_bridge`. The
standard Rust solution is `thread_local!` with lazy initialization:
```rust
thread_local! {
    static WORKER_NIMA: RefCell<Option<Nima>> = RefCell::new(None);
}
// Inside the par_bridge closure:
WORKER_NIMA.with(|cell| {
    let mut borrow = cell.borrow_mut();
    let nima = borrow.get_or_insert_with(|| {
        Nima::new(LoadedModel::from_verified(&verified_bytes)
            .expect("worker session init"))
    });
    nima.score(&rgb)
})
```

(The `expect` inside `get_or_insert_with` needs a plan-level decision — see T-η
for the `?`-in-closure issue.)

Additionally, simp notes that if `ort::Session::run` takes `&self` (as the type
agent found from the ort 2.x docs), then sharing one `Arc<Session>` across
workers is viable, eliminating per-worker construction entirely and making
`thread_local!` unnecessary. The plan should resolve this at D0 §Threading
semantics and propagate the decision to D1b + D4.

**Remediation**: Add to D4 (after line 504):
```
Per-worker Session construction uses `thread_local!` storage so construction
runs ONCE per rayon worker thread (O(num_rayon_workers)), NOT once per photo
(O(num_photos)). Construction cost: num_cpus × ~200-500ms ≈ 1-4 seconds total.
If D0 §Threading semantics confirms `Session::run` takes `&self`, simplify to
one shared `Arc<Nima>` (no thread_local! needed) and update the signature to
`scorer: &Nima` OR `scorer: Arc<Nima>`.
```

---

## HIGH

### T-δ — `NimaScore::cmp` uses `.expect()` — violates `expect_used = "warn"` escalated to CI error by `-D warnings` (2-way)

**Agents**: arch (HIGH), type (HIGH)

`docs/plans/session-03.md:242`:
```rust
fn cmp(&self, other: &Self) -> Ordering {
    self.0.partial_cmp(&other.0).expect("NimaScore is NaN-free")
}
```
The plan at line 243 claims this "avoids `unwrap_used = "warn"` clippy lint."
This is false: `expect_used = "warn"` is a distinct lint (same clippy group)
that is ALSO in the workspace configuration, escalated to an error by
`cargo clippy -D warnings` in CI.

The correct implementation uses `f32::total_cmp` (stable since Rust 1.62;
MSRV is 1.88):
```rust
fn cmp(&self, other: &Self) -> Ordering { self.0.total_cmp(&other.0) }
```
`total_cmp` returns `Ordering` directly (no `Option`), requires no `expect`,
and provides IEEE 754 totalOrder semantics (NaN > +Inf if NaN were ever
present — stronger safety guarantee than `expect`).

**Remediation**: Replace line 242's `Ord` implementation with `self.0.total_cmp(&other.0)`.
Remove line 243's incorrect lint-avoidance claim. Replace with: "uses
`f32::total_cmp` (stable Rust 1.62+; MSRV 1.88) — no `expect`/`unwrap` needed."

---

### T-ε — Plan's factual claim "`Session::run` takes `&mut self`" may be wrong; if `&self`, per-worker model is unnecessary and `scorer: &Nima` is correct (3-way)

**Agents**: gp (CRITICAL), arch (HIGH), type (MEDIUM-discovering `&self`)

`docs/plans/session-03.md:122-123`:
```
Session 03 picks option (b): one `Session` per rayon worker thread (no
async complexity, correct per `Session::run` `&mut self` receiver).
```

The type agent researched ort 2.0.0-rc.12 docs and found `Session::run` takes
`&self` (immutable), NOT `&mut self`. If true, this changes the design
significantly:

- With `Session::run(&self)`, sharing one `Session` behind `Arc<Session>`
  across workers is safe (no `Mutex` needed — `Sync` holds).
- The entire "per-worker Session construction via `thread_local!`" complexity
  (T-γ) becomes unnecessary.
- The `scorer: &Nima` parameter in D4's signature IS correct (shared immutable
  `Nima` wrapping one `Session`).
- The plan's factual claim at line 122-123 ("correct per `Session::run` `&mut
  self` receiver") is **wrong**.

This is a T-ε finding: the plan chose the correct-but-expensive concurrency
model (per-worker) for the wrong factual reason. D0's §Threading semantics is
designed to verify exactly this claim. If D0 confirms `Session::run` is `&self`,
the plan should simplify to one shared `Arc<Nima>` (dropping T-γ's complexity
entirely).

**Note**: This finding is `retain=yes-flag-for-human-triage` — it depends on the
actual ort API which only D0 can confirm. If D0 confirms `&self`: simplify to
`Arc<Nima>`; if D0 confirms `&mut self`: T-γ's `thread_local!` fix is correct.

**Remediation**: Amend D0 §Threading semantics to explicitly record the
`Session::run` receiver type as a binding D0 output. Add to the D0 §Threading
semantics bullet:
```
Record the Session::run receiver type (& or &mut self). If &self: simplify D4
to one Arc<Nima> shared across workers (drop per-worker construction). If &mut
self: use thread_local! per T-γ remediation. The implementation MUST match D0's
finding.
```
Also correct line 122-123 to read: "one `Session` per rayon worker thread (OR
one shared `Arc<Session>` if D0 confirms `Session::run` takes `&self` — see D0
§Threading semantics binding output)."

---

### T-ζ — `catalog_inconsistency` counter appears in dispatch table but is absent from `CullStats` field specification (2-way)

**Agents**: sfh (CRITICAL reclassified HIGH at plan stage), gp (via WL-7)

`docs/plans/session-03.md:534` dispatch table:
```
FK violation ... | `catalog_inconsistency` | warn, skip
```
`docs/plans/session-03.md:487-490` `CullStats` spec:
```
uses AtomicU64 for all per-photo counters (parallel to IngestStats at
ingest.rs:87). Shared via Arc<CullStats> across rayon workers.
```
The `CullStats` spec does not enumerate the individual counters. The dispatch
table adds a fifth counter (`catalog_inconsistency`) not mentioned in T3's
`AtomicU64` remediation. Implementer must infer the full counter list from
the dispatch table without a canonical field enumeration.

Additionally, the sfh agent notes that `CullStats` may need an `in_flight`
progress counter (like `IngestStats::in_flight`) for the cull heartbeat summary
line (D4 duplicates heartbeat scaffolding per TD-016). The plan does not specify
the heartbeat summary format for cull.

**Remediation**: Enumerate all `CullStats` fields explicitly in the D4 spec
(after line 490):
```
CullStats fields:
  in_flight: AtomicU64    -- photos currently being processed (for heartbeat)
  scored: AtomicU64       -- photos successfully scored
  inference_failed: AtomicU64
  decode_failed: AtomicU64
  file_missing: AtomicU64
  content_changed: AtomicU64
  catalog_inconsistency: AtomicU64
```
Add one line specifying the heartbeat summary format for cull (analogous to
ingest's `[heartbeat] walked {}, ingested {}, in-flight {}`).

---

### T-η — `PhotoId::derive` pseudocode uses `?` inside rayon `for_each` closure; compile error + no dispatch row for derive failures (2-way)

**Agents**: sfh (CRITICAL reclassified HIGH at plan stage), test (implied)

`docs/plans/session-03.md:514`:
```rust
let current_id = PhotoId::derive(&source_path)?;
```
Rayon's `par_bridge().for_each(|row| { ... })` closures return `()` — the `?`
operator cannot propagate `Result` out of a `for_each` closure. This is a
compile error. The existing `ingest.rs` pattern handles this correctly: `ingest_one()`
is called inside a `match` and errors are mapped to per-counter increments.

Additionally, the dispatch table (lines 525-534) has no row for the case where
`PhotoId::derive` itself fails (`Error::Io { ... }` from permission denied or
zero-byte file). The current rows cover: decode fails, file not found, content
changed, FK violation, already scored. A zero-byte CR3 (corrupted download) that
IS present on disk (not `file_missing`) falls through with no handler.

**Remediation**:
1. Rewrite pseudocode at line 514 to use `match` (following `ingest.rs:199-218`):
   ```rust
   let current_id = match PhotoId::derive(&source_path) {
       Ok(id) => id,
       Err(_) => {
           stats.derive_failed.fetch_add(1, Relaxed);
           tracing::warn!(...);
           continue;
       }
   };
   ```
2. Add `derive_failed: AtomicU64` to `CullStats` field list (T-ζ remediation).
3. Add dispatch table row: `PhotoId::derive IO/parse failure | derive_failed | WARN, skip; FAIL if derive_failed > 0 under --strict (or: WARN only — decide)`.

---

## MEDIUM

### T-θ — Plan header metadata says "v2 (R1 remediation)" but plan is at v3 (2-way)

**Agents**: gp (MEDIUM), com (HIGH)

`docs/plans/session-03.md:8`:
```
> **Plan revisions**: v2 (R1 remediation)
```
The plan body has a complete v3 entry at lines 872-912. But the header metadata
— the first thing a reviewer reads — is stale. Not a functionality issue but
creates reviewer confusion.

**Remediation**: Change line 8 to `> **Plan revisions**: v3 (R2 remediation)`.

---

### T-κ — `HeartbeatDeathTrigger` struct over-engineered for what it tests; eliminable with inline `spawn + panic` (2-way)

**Agents**: simp (HIGH), rev (implied via T-β restructuring)

The T-β remediation (change to in-process test) enables further simplification.
Once the subprocess variant is dropped, the `HeartbeatDeathTrigger` struct
(`Arc<AtomicBool>` + dedicated thread) is wrapping a 6-line pattern that can
be inlined as a private test helper. The `photohelper-test-helpers` crate exists
solely for this struct. Once the in-process approach is adopted, the crate may
be unnecessary.

**Remediation** (dependent on T-β closure): If the heartbeat-death test is
in-process only (T-β fix), assess whether `HeartbeatDeathTrigger` needs to be a
named struct in a separate crate, or can be an inline helper in
`photohelper-cli/src/commands/` test modules. If the crate has no other purpose,
drop it entirely and inline the test logic. This decision can be made at D5c
implementation time — record it in the plan as: "If `photohelper-test-helpers`
has only one consumer after T-β restructuring, collapse into an inline test
helper to avoid the one-consumer crate anti-pattern."

---

## LOW

### T-ι — TECH-DEBT.md TD-012 still cross-references DN-023; partial T4 closure (2-way)

**Agents**: gp (WL-4 residual), com (MEDIUM)

`TECH-DEBT.md:222`: `Cross-reference DN-022 + DN-023.`

DN-023 (ON DELETE CASCADE absent from cull_scores) is unrelated to the AHD
demosaic stop-gap that TD-012 tracks. This is a residual from the pre-T4 state
where DN-022 and DN-023 numbering was swapped. The plan body is correct; only
the TECH-DEBT.md cross-reference is stale.

**Remediation**: Change `TECH-DEBT.md:222` from `Cross-reference DN-022 + DN-023` to `Cross-reference DN-022`.

---

## Disposition summary

| Disposition | Count | Action |
|-------------|------:|--------|
| **Fix in plan v4 (CRITICAL)** | 3 | T-α, T-β, T-γ — must close before implementation |
| **Fix in plan v4 (HIGH)** | 4 | T-δ, T-ε, T-ζ, T-η |
| **Fix in plan v4 (MEDIUM)** | 2 | T-θ, T-κ |
| **One-line fix (LOW)** | 1 | T-ι (TECH-DEBT.md) |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 7
  verified: 5
  drifted: 2
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    All 7 CRITICAL+HIGH findings verified by 9th-agent with orchestrator
    post-hoc substring-grep. Two findings are drifted (line numbers off by 1-2
    lines). Zero hallucinations. MEDIUM/LOW findings verified via reading.
  details:
    - {finding_id: T-α, file: docs/plans/session-03.md, line: 244, present: yes,     retain: yes,                    evidence_snippet: "NOT `Eq` (f32 equality is floating-point, not natural equality)."}
    - {finding_id: T-β, file: docs/plans/session-03.md, line: 612, present: yes,     retain: yes,                    evidence_snippet: "spawns `photohelper ingest` (or `cull`) with this env-var set."}
    - {finding_id: T-γ, file: docs/plans/session-03.md, line: 502, present: drifted, retain: yes-with-corrected-line, evidence_snippet: "Each worker calls `LoadedModel::from_verified(&verified_bytes)` independently to construct"}
    - {finding_id: T-δ, file: docs/plans/session-03.md, line: 242, present: yes,     retain: yes,                    evidence_snippet: "self.0.partial_cmp(&other.0).expect(\"NimaScore is NaN-free\")"}
    - {finding_id: T-ε, file: docs/plans/session-03.md, line: 123, present: drifted, retain: yes-flag-for-human-triage, evidence_snippet: "`&mut self` receiver). ABORT if option (b) fails for a structural"}
    - {finding_id: T-ζ, file: docs/plans/session-03.md, line: 534, present: yes,     retain: yes,                    evidence_snippet: "catalog_inconsistency` | warn, skip (not a strict failure)"}
    - {finding_id: T-η, file: docs/plans/session-03.md, line: 514, present: yes,     retain: yes,                    evidence_snippet: "let current_id = PhotoId::derive(&source_path)?;"}
```

---

## R4 watch-list (must verify in Round 4 after plan v4 remediation)

1. T-α: `NimaScore` derives `Eq` alongside `Ord`; "NOT `Eq`" clause removed.
2. T-β: D5e heartbeat-death test changed to in-process only; subprocess variant removed; `PHOTOHELPER_HEARTBEAT_POISON_TICKS` dropped from D5e.
3. T-γ: D4 ort concurrency model specifies `thread_local!` for once-per-thread construction OR declares that D0 §Threading confirms `Session::run` is `&self` and simplifies to `Arc<Nima>`.
4. T-δ: `NimaScore::cmp` uses `f32::total_cmp(&other.0)` (no `expect()`).
5. T-ζ: `CullStats` field list explicitly enumerates all counters including `catalog_inconsistency` and `derive_failed`.
6. T-η: `PhotoId::derive` pseudocode uses `match` (not `?`); `derive_failed` dispatch row added to D4 table.
7. T-ι: TECH-DEBT.md TD-012 cross-reference corrected to `DN-022` only.
