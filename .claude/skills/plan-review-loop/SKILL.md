---
name: plan-review-loop
description: Run the plan review autonomously in a continuous loop until no findings remain. Uses the eight-agent-review skill for each round, addresses all findings automatically, and repeats until the review is clean.
argument-hint: "[session-NN]"
allowed-tools: Read Grep Glob Write Edit Agent Bash
---

Run the **plan-review-loop** to autonomously review and fix the session plan (`docs/plans/session-NN.md`) until it is fully approved.

## Steps

1. **Initialize**: Set the current round to `R = 1`.
2. **Review Round R**: Follow the `eight-agent-review` skill against the plan. Consolidate the findings and write them to `docs/code-reviews/session-NN-plan-round{R}.md`.
3. **Evaluate Findings**:
   - If there are **NO** findings (or only acknowledged false positives), the plan is clean. Exit the loop and proceed to the next phase.
   - If there **ARE** findings, proceed to step 4.
4. **Autonomous Remediation**:
   - Analyze the root cause of the findings.
   - Apply the fixes directly to `docs/plans/session-NN.md` and any necessary ledgers.
   - **Important**: In this loop mode, you do **NOT** pause for user approval before fixing. You autonomously implement the fixes.
5. **Next Round**: Increment `R = R + 1` and loop back to Step 2. Do this until the review is clean.
