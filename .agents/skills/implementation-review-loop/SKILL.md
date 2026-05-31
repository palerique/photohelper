---
name: implementation-review-loop
description: Run the implementation review autonomously in a continuous loop until no findings remain. Uses the eight-agent-review skill for each round, addresses all findings by modifying the code, and repeats until the review is clean.
argument-hint: "[session-NN]"
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content multi_replace_file_content invoke_subagent send_message
---

Run the **implementation-review-loop** to autonomously review and fix the session code until it is perfect.

## Steps

1. **Initialize**: Set the current round to `R = 1`.
2. **Review Round R**: Run the `eight-agent-review` skill (scope = `session`, targeting all code written this session). Consolidate the findings and write them to `docs/code-reviews/session-NN-round{R}.md`.
3. **Evaluate Findings**:
   - If there are **NO** findings (or only acknowledged false positives), the implementation is clean. Exit the loop and proceed to the next phase (e.g., session-end ledgers/commits).
   - If there **ARE** findings, proceed to step 4.
4. **Autonomous Remediation**:
   - Analyze the root cause of the findings (Failure Mode, regression risk).
   - Choose the best architectural or code-level fixes.
   - Apply the fixes directly to the source code and tests.
   - **Important**: In this loop mode, you do **NOT** pause for user approval before fixing. Autonomously execute the fixes.
5. **Next Round**: Increment `R = R + 1` and loop back to Step 2. Do this until the review is clean.
