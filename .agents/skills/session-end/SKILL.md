---
name: session-end
description: Run the eng-protocol session-end checkpoint and ship the PR. Performs the final multi-agent double-review on the session's code, updates SESSION-STATE.md, checkpoints HANDOFF_REPORT.md + docs/discovery-notes.md, closes/files touched TDs, runs `just session-end` (just ci), opens the PR to main, waits for green CI, merges with a merge commit, and renders the two-block session handoff. Use when a session's implementation is complete and ready to ship.
argument-hint: "[session-NN]"
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content invoke_subagent send_message
---

Run the **session-end** checkpoint per `docs/quality-assurance.md § Session-end protocol`. Do these in order; do not skip the review or the handoff.

## Steps

1. **Final review (double):** run the `eight-agent-review` skill (scope = `session`, target = all code written this session) for Round 1 → remediate → Round 2 → remediate. Write `docs/code-reviews/session-NN-round{1,2}.md`.
2. **Update the ledgers:**
   - `SESSION-STATE.md`: Last session, Next action, Status, component-progress table, any open Round-2 items.
   - `HANDOFF_REPORT.md`: append a checkpoint block.
   - `docs/discovery-notes.md`: append if new findings surfaced.
   - `TECH-DEBT.md`: close/file every TD touched this session (each with a binding trigger).
3. **Commit** per conventional-commits (one logical change per commit; review artifacts + state updates as their own commits).
4. **Gate:** `just session-end` (runs `just ci`) must be fully green.
5. **Ship:**
   ```bash
   git push -u origin session-NN/<slug>
   gh pr create --base main --head session-NN/<slug> \
     --title "session NN: <summary>" \
     --body "<points at docs/plans/session-NN.md + the Round-2 review>"
   gh pr checks --watch        # never merge yellow/red — investigate first
   gh pr merge --merge --delete-branch
   ```
6. **Render the handoff:** end your final response with EXACTLY the two blocks from `docs/session-handoff-format.md`, in order and last: the `## Session NN final summary` table, then the next-session bash bootstrap in a `bash` fence. Use the row labels verbatim.
