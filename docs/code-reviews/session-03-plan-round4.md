# Session 03 — plan review, Round 4

> Per `docs/quality-assurance.md § Plan-review protocol`.
> Cadence A → Tier 5 (plan stage), targeted 2-agent verification round against
> `docs/plans/session-03.md` v4 (committed at `a9f7152` + fixup).
> Round 4 triggered by 3 CRITICALs in Round 3. Focus: verify R3 remediations
> held cleanly and no new regressions were introduced.

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
    - feature-dev:code-reviewer
    - pr-review-toolkit:type-design-analyzer
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## R4 watch-list from Round 3 — all 7 items VERIFIED PASS

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 | T-α: `NimaScore` derives `Eq` + `Ord`; "NOT Eq" removed | PASS | lines 241-242: `PartialEq + Eq + PartialOrd + Ord` |
| 2 | T-β: D5e heartbeat-death in-process only; no subprocess; no `PHOTOHELPER_HEARTBEAT_POISON_TICKS` | PASS | lines 674-682, 713-716 |
| 3 | T-γ: D0 §Threading semantics binding; `thread_local!` path + `Arc<Nima>` path both specified | PASS | lines 115-126 (D0), 541-575 (D4) |
| 4 | T-δ: `NimaScore::cmp` uses `f32::total_cmp` (no `expect()`) | PASS | lines 246-247 |
| 5 | T-ζ: `CullStats` enumerates all 8 fields including `catalog_inconsistency` + `derive_failed` | PASS | lines 497-508 |
| 6 | T-η: `PhotoId::derive` uses `match` (not `?`); `derive_failed` dispatch row present | PASS | lines 514-529, ~601 |
| 7 | T-ι: TD-012 cross-reference clarified (`DN-022` only; `DN-023` disclaimed) | PASS | TECH-DEBT.md line 225 |

---

## Triage summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 0 | R3 remediations all held cleanly |
| **HIGH**      | 0 | |
| **MEDIUM**    | 2 | Both resolved in v4 fixup commit before this artifact |
| **LOW**       | 0 | |

---

## MEDIUM (both resolved inline before this artifact)

### R4-M1 — Stale pre-T-η pseudocode block left at lines 577-586 (rev, type)

**Status: RESOLVED** in v4 fixup commit.

The v4 edits left a stale duplicate of the per-photo pipeline pseudocode (using
`?` operator, which T-η fixed). The T-η remediated block at lines 512-529
(using `match`) was correct. The stale `?` block at lines 577-586 was deleted
in the v4 fixup.

---

### R4-M2 — `Nima::new` constructor undefined; `Result` from `LoadedModel::from_verified` unhandled in `thread_local!` pseudocode (type)

**Status: RESOLVED** in v4 fixup commit.

The `thread_local!` pseudocode called `Nima::new(LoadedModel::from_verified(&bytes))`
but: (a) `Nima::new` was never defined in D1c, and (b) `from_verified` returns
`Result<LoadedModel>`, not `LoadedModel` — cannot be passed directly.
`get_or_insert_with` takes an infallible `FnOnce() -> T`.

Resolved by:
1. Adding `Nima::new(model: LoadedModel) -> Self` as an infallible constructor
   in D1c's `nima.rs` specification.
2. Rewriting the `thread_local!` pseudocode with an explicit `if borrow.is_none()`
   block that handles the `Result` from `from_verified`, propagating errors to
   the `inference_failed` dispatch path.

---

## Plan-review conclusion

**Plan review is COMPLETE.** Four rounds of review (R1 → R2 → R3 → R4) converged to zero CRITICAL + zero HIGH findings. The plan is implementation-ready.

**Review statistics**:
- R1: 10 CRITICAL + 18 HIGH + 10 MEDIUM + 5 LOW → plan v2
- R2: 3 CRITICAL + 10 HIGH + 9 MEDIUM + 4 LOW → plan v3
- R3: 3 CRITICAL + 4 HIGH + 2 MEDIUM + 1 LOW → plan v4
- R4: 0 CRITICAL + 0 HIGH + 2 MEDIUM → resolved inline, plan-review CLEAN

**Implementation may begin.** Next action: implementation per plan v4 in the
plan's stated Deliverable order (D6 first-chore commit → D0 pre-flight →
D1a → D1b/c → D1d → D2a/b/c → D3 → D4 → D5 → D7).

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 2
  verified: 2
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  notes: >
    Both MEDIUM findings verified by two agents (code-reviewer + type-design)
    against file content. Both resolved in v4 fixup commit before this artifact
    was written. R4 watch-list verification: 7/7 PASS with verbatim evidence
    snippets from the plan file. No new CRITICALs or HIGHs surfaced. Plan
    review declared complete.
  details:
    - {finding_id: R4-M1, file: docs/plans/session-03.md, line: 577, present: yes-resolved, retain: no, reason: "stale ? pseudocode deleted in v4 fixup"}
    - {finding_id: R4-M2, file: docs/plans/session-03.md, line: 563, present: yes-resolved, retain: no, reason: "Nima::new added; thread_local! Result handling fixed in v4 fixup"}
```
