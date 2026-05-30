---
name: session-end
description: Run the eng-protocol session-end checkpoint and ship the PR. Performs the final multi-agent double-review on the session's code, updates SESSION-STATE.md, checkpoints HANDOFF_REPORT.md + docs/discovery-notes.md, closes/files touched TDs, runs `just session-end` (just ci), opens the PR to main, waits for green CI, merges with a merge commit, and renders the two-block session handoff. Use when a session's implementation is complete and ready to ship.
argument-hint: "[session-NN]"
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content invoke_subagent send_message
---

Run the **session-end** checkpoint per `docs/quality-assurance.md § Session-end protocol`. Do these in order; do not skip the review or the handoff.

## Steps

1. **Final Review - Round 1**: Run the `eight-agent-review` skill (scope = `session`, target = all code written this session) and write `docs/code-reviews/session-NN-round1.md`.

---

## 2. Findings Remediation Blueprint (Between Review & Fixes)

Between receiving the Round 1 review findings and executing any fixes on the production/test code, the agent MUST draft a highly disciplined and structured **Deep Remediation Blueprint** and present it to the user. This blueprint acts as a deliberate design and alignment phase to prevent shallow or superficial patching. Follow this protocol:

### Phase A: Root-Cause Analysis (RCA) & Classification
Categorize and analyze each consolidated finding (by theme and the 9th Agent's verified ID) across the following dimensions:
1. **Failure Mode**: Classify the issue (e.g., *Type Invariant Bypass*, *Silent Failure Path*, *Design Model Mismatch*, *Cognitive Overload/Bloat*, *QA/Testing Gap*).
2. **Underlying Cause**: Explain *why* the initial implementation allowed this gap to exist. What assumption or logic was proved wrong by the review lens?

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
- **Explicit Halt**: Pause and request the user's explicit review and green-light. Do not touch any source or test files until the user gives consent.

### Phase F: State & Ledger Synchronization
- Ensure all proposed changes, deferred technical debts, and architectural decisions resolved during this remediation phase adhere strictly to `docs/quality-assurance.md § State & Context Synchronization Discipline`.
- Record and sync these changes across all referencing ledgers (`TECH-DEBT.md`, `SESSION-STATE.md`, `HANDOFF_REPORT.md`) in a single commit, using precise identifiers and high-density, non-summarized context.

---

## 3. Review Round 2 & Regression Check
- Once the Round 1 Remediation Blueprint is approved and implemented, re-run `eight-agent-review` against the remediated session code.
- Write the findings to `docs/code-reviews/session-NN-round2.md`.
- **Round 2 Remediation**: If Round 2 surfaces findings or regressions introduced by Round 1 fixes, repeat the **Remediation Blueprint** process (§ 2) to get approval for Round 2 fixes.

---

## 4. Update the Ledgers
- `SESSION-STATE.md`: Last session, Next action, Status, component-progress table, any open Round-2 items.
- `HANDOFF_REPORT.md`: append a checkpoint block.
- `docs/discovery-notes.md`: append if new findings surfaced.
- `TECH-DEBT.md`: close/file every TD touched this session (each with a binding trigger).

5. **Commit**: Commit per conventional-commits (one logical change per commit; review artifacts + state updates as their own commits).
6. **Gate**: `just session-end` (runs `just ci`) must be fully green.
7. **Ship**:
   ```bash
   git push -u origin session-NN/<slug>
   gh pr create --base main --head session-NN/<slug> \
     --title "session NN: <summary>" \
     --body "<points at docs/plans/session-NN.md + the Round-2 review>"
   gh pr checks --watch        # never merge yellow/red — investigate first
   git switch main && git pull --ff-only origin main
   gh pr merge --merge --delete-branch
   ```
8. **Render the Handoff**: End your final response with EXACTLY the two blocks from `docs/session-handoff-format.md`, in order and last: the `## Session NN final summary` table, then the next-session bash bootstrap in a `bash` fence. Use the row labels verbatim.
