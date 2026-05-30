---
name: plan-review
description: Run the mandatory plan review (before any code) on docs/plans/session-NN.md using the eng-protocol multi-agent double-review. Fires the agent suite for Round 1, consolidates findings by theme, remediates, then re-runs Round 2; writes docs/code-reviews/session-NN-plan-round{1,2}.md. Use after authoring a session plan and before writing any implementation code.
argument-hint: "[session-NN]"
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content invoke_subagent
---

Run the **plan-review** checkpoint per `docs/quality-assurance.md § Plan-review protocol`. The plan must answer: what will exist by end-of-session; what is out of scope (deferrals → `TECH-DEBT.md`); how each deliverable is tested; which checkpoints fire; what discovery items are expected.

## Steps

1. Identify the plan file `docs/plans/session-NN.md` (NN from `$ARGUMENTS` or the current branch name).
2. Optionally scaffold the artifacts: `just plan-review-skeleton NN 1` and `just plan-review-skeleton NN 2`.
3. **Round 1:** follow the `eight-agent-review` skill against the plan (scope = `plan`). Write `docs/code-reviews/session-NN-plan-round1.md`, consolidated by theme.
4. Remediate Round 1 findings in batched edits to the plan.
5. **Round 2:** re-run `eight-agent-review` against the remediated plan. Expect regressions introduced by Round 1 edits — that is the point. Write `docs/code-reviews/session-NN-plan-round2.md`.
6. Remediate Round 2 findings. If Round 2 surfaced CRITICAL-class regressions needing another cycle, add Round 3.
7. Only after Round 2 + remediation are clean, tell the user the plan is fully ready and ask for explicit authorization to start the implementation. **Never skip Round 2, and NEVER write or modify any application/library/test code until the user has explicitly given the green light to start implementing.**

Triage every finding (CRITICAL/HIGH/MEDIUM/LOW) and route deferrals to `TECH-DEBT.md` with a binding trigger.
