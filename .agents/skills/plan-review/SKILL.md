---
name: plan-review
description: Run the mandatory plan review (before any code) on docs/plans/session-NN.md using the eng-protocol multi-agent double-review. Fires the agent suite for Round 1, consolidates findings by theme, remediates, then re-runs Round 2; writes docs/code-reviews/session-NN-plan-round{1,2}.md. Use after authoring a session plan and before writing any implementation code.
argument-hint: "[session-NN]"
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content invoke_subagent
---

Run the **plan-review** checkpoint per `docs/quality-assurance.md § Plan-review protocol`. The plan must answer: what will exist by end-of-session; what is out of scope (deferrals → `TECH-DEBT.md`); how each deliverable is tested; which checkpoints fire; what discovery items are expected.

## Steps

1. **Plan File Identification**: Locate the plan file `docs/plans/session-NN.md` (NN from `$ARGUMENTS` or the current branch name).
2. **Skeleton Setup**: Optionally scaffold the artifacts: `just plan-review-skeleton NN 1` and `just plan-review-skeleton NN 2`.

---

## 3. Review Round 1 & Consolidation
- Run the `eight-agent-review` skill against the plan (scope = `plan`).
- Consolidate all sub-agent findings by theme, perform the 9th Agent verification, and write `docs/code-reviews/session-NN-plan-round1.md`.

---

## 4. Findings Remediation Blueprint (Between Review & Fixes)

Between receiving the Round 1 review findings and executing any fixes, the agent MUST draft a highly disciplined and structured **Deep Remediation Blueprint** and present it to the user. This blueprint acts as a deliberate design and alignment phase to prevent shallow or superficial patching. Follow this protocol:

### Phase A: Root-Cause Analysis (RCA) & Classification
Categorize and analyze each consolidated finding (by theme and the 9th Agent's verified ID) across the following dimensions:
1. **Failure Mode**: Classify the issue (e.g., *Type Invariant Bypass*, *Silent Failure Path*, *Design Model Mismatch*, *Cognitive Overload/Bloat*, *QA/Testing Gap*).
2. **Underlying Cause**: Explain *why* the initial plan or design allowed this gap to exist. What assumption was proved wrong by the review lens?

### Phase B: Multi-Option Evaluation & Architectural Adjustment
For every **CRITICAL** or **HIGH** severity finding, the agent must not default to the easiest superficial fix. Instead:
- **Evaluate Alternatives**: Contrast at least *two* distinct remediation approaches (e.g., compile-time enforcement vs. runtime verification, structural refactoring vs. early returns).
- **Justify the Choice**: Explain why the chosen architectural adjustment represents the cleanest, most maintainable, and most robust solution.

### Phase C: File & Scope Traceability Matrix
Construct a clear mapping of the planned modifications:
- **Traceability Link**: Pair each Finding ID explicitly to the files and line numbers targeted for edit.
- **Detailed Scope**: Specify the exact data structures, functions, or signatures to be altered.

### Phase D: Regression Risk & Verification Protocol
- **Analyze Side-Effects**: Identify downstream modules, API callers, or FFI bindings that might be impacted by the remediation.
- **Custom Verification Checklist**: Define exactly what tests (unit, integration, or custom dry-runs) will be executed to prove that the findings are successfully resolved and that no regressions have been introduced.

### Phase E: Interactive Alignment Milestone (Pause for Approval)
- Present the Deep Remediation Blueprint to the user in a highly readable, structured format.
- **Explicit Halt**: Pause and request the user's explicit review and green-light. Do not touch any plan or source files until the user gives consent.

---

## 5. Round 2 Review & Regression Check
- Once the Round 1 Remediation Blueprint is approved and implemented, re-run `eight-agent-review` against the remediated plan.
- **Audit for Regressions**: Pay explicit attention to regressions or side-effects introduced by the Round 1 fixes—this is the critical function of Round 2.
- Write the findings to `docs/code-reviews/session-NN-plan-round2.md`.
- **Round 2 Remediation**: If Round 2 surfaces findings, repeat the **Remediation Blueprint** process (§ 4) to plan and get approval for the Round 2 fixes. If critical regressions are found, proceed to a Round 3 cycle.

---

## 6. Implementation Readiness
- Only after Round 2 (or higher) is completely clean and all remediation steps are verified, tell the user the plan is ready.
- Request explicit, final authorization from the user to start the implementation.
- **CRITICAL SAFEGUARD**: Never write or modify any production, library, application, or test code until the user has explicitly given the green light to start implementing.
