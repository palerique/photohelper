---
name: session-start
description: Begin an eng-protocol work session in this repo. Creates the session-NN/<slug> branch off updated main, runs `just session-start` (verify-state), reads SESSION-STATE.md + the latest docs/code-reviews Round-2 file + HANDOFF_REPORT.md + docs/discovery-notes.md, resolves any blocking Round-2 item, then declares the session goal in docs/plans/session-NN.md. Use at the start of any work session in a repo that carries CLAUDE.md + docs/quality-assurance.md.
argument-hint: "[short-kebab-slug for the session focus]"
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content
---

You are starting a work session under the eng-protocol using the Antigravity CLI. The authoritative protocol is this repo's `docs/quality-assurance.md § Session-start protocol` and `CLAUDE.md § Mandatory session protocol` — follow them; this skill is the operational checklist.

## Steps

1. Confirm you are in an eng-protocol repo (a `CLAUDE.md` and `docs/quality-assurance.md` exist at the root). If not, stop and say so.
2. Determine the next session number `NN` from `SESSION-STATE.md` ("Last session") and `docs/plans/`. Zero-pad 1–9.
3. Create the session branch (never work on `main`):
   ```bash
   git switch main && git pull --ff-only origin main
   git switch -c session-NN/<slug>   # <slug> from $ARGUMENTS or the next component
   ```
4. Run `just session-start`; confirm `STATUS: ready`. If `STATUS: blocked`, resolve the missing scaffolding before continuing.
5. Read, in order: `SESSION-STATE.md`; the latest `docs/code-reviews/session-*-round2.md`; `HANDOFF_REPORT.md`; `docs/discovery-notes.md`. Surface any unresolved Round-2 item — do NOT plan on top of it.
6. Declare the session goal by writing the top block of `docs/plans/session-NN.md` (the session contract): goal; what will exist by end-of-session; what's out of scope (deferrals → `TECH-DEBT.md`); how each deliverable is tested; which checkpoints fire.
7. Hand off to plan review: tell the user the plan is ready for the `plan-review` skill.

Keep the branch name and the plan's stated scope consistent with `SESSION-STATE.md`. Commit the plan's top block before plan-review.
