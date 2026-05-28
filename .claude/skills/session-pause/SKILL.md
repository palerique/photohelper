---
name: session-pause
description: Safely pause the current session when the context window is nearing its limit. Runs local CI gating, updates state ledgers, commits intermediate work, and provides a restart prompt for a fresh context window. Do NOT open a PR or push to main.
allowed-tools: Read Grep Glob Write Edit Bash(just *) Bash(git *)
color: blue
---

Run the **context-refresh** protocol to save intermediate state before the user clears the LLM context window. Do these steps in order; do not skip the state updates.

## Steps

1. **Gate:** Run `just session-end` (which runs `just ci`). This must be fully green locally.

   - _Note:_ If tests, linting, or builds fail, you must remediate the issues before proceeding with the context refresh.

2. **Update the ledgers:**

   - `SESSION-STATE.md`: Update Last session, Next action, Status, component-progress table, and explicitly note that the session was paused for a context refresh.
   - `HANDOFF_REPORT.md`: Append a checkpoint block summarizing exactly what was achieved in this window and the precise next steps required when context is restored.
   - `docs/discovery-notes.md`: Append any new architectural or technical findings surfaced during this window.
   - `TECH-DEBT.md`: Close/file every TD touched during this specific window.

3. **Commit:** Commit the current progress per `conventional-commits` (one logical change per commit).

   - _Example:_ `chore(session-NN): save intermediate state for context refresh`.
   - Review artifacts and state updates should be in their own commits. Do NOT push or open a PR.

4. **Handoff & Restart Prompt:** End your final response with EXACTLY the following block to instruct the user on how to proceed.

   > ### 🛑 Context Saved Successfully
   >
   > The local CI is green, ledgers are updated, and intermediate work is committed.
   >
   > **Next Steps for the User:**
   >
   > 1. Clear your current context window (e.g., using `/clear` or `/compact`).
   > 2. Paste the prompt below to seamlessly resume our work:
   >
   > ```text
   > I have cleared the context window. Please read `SESSION-STATE.md` and the latest checkpoint in `HANDOFF_REPORT.md` to re-orient yourself, then resume the implementation for our next action.
   > ```
