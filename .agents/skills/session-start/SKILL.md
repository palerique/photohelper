---
name: session-start
description: Begin an eng-protocol work session in this repo. Creates the session-NN/<slug> branch off updated main, runs `just session-start` (verify-state), reads SESSION-STATE.md + the latest docs/code-reviews Round-2 file + HANDOFF_REPORT.md + docs/discovery-notes.md, resolves any blocking Round-2 item, then declares the session goal in docs/plans/session-NN.md. Use at the start of any work session in a repo that carries CLAUDE.md + docs/quality-assurance.md.
argument-hint: "[short-kebab-slug for the session focus]"
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content
---

You are starting a work session under the eng-protocol using the Antigravity CLI. The authoritative protocol is this repo's `docs/quality-assurance.md § Session-start protocol` and `CLAUDE.md § Mandatory session protocol` — follow them; this skill is the operational checklist.

## Steps

1. **Repo Pre-Check**: Confirm you are in an eng-protocol repo (a `CLAUDE.md` and `docs/quality-assurance.md` exist at the root). If not, stop and say so.
2. **Session Numbering**: Determine the next session number `NN` from `SESSION-STATE.md` ("Last session") and `docs/plans/`. Zero-pad 1–9.
3. **Branch Creation**: Create the session branch (never work on `main`):
   ```bash
   git switch main && git pull --ff-only origin main
   git switch -c session-NN/<slug>   # <slug> from $ARGUMENTS or the next component
   ```
4. **State Verification**: Run `just session-start`; confirm `STATUS: ready`. If `STATUS: blocked`, resolve the missing scaffolding before continuing.
5. **Ledger Auditing**: Read, in order: `SESSION-STATE.md`; the latest `docs/code-reviews/session-*-round2.md`; `HANDOFF_REPORT.md`; `docs/discovery-notes.md`. Surface any unresolved Round-2 item — do NOT plan on top of it.

---

## 6. Exploration & Discovery Phase (Emulating Code-Explorer)

Before drafting the session plan, the agent MUST perform a deep, methodical exploration of the target codebase area to eliminate all assumptions. Follow this structured protocol:

### Phase A: Structural & Architectural Mapping
1. **Directory and Module Mapping**: Use `list_dir` on directories relevant to the session's slug to map files and modules.
2. **Public API and Invariant Audit**: Use `grep_search` and `view_file` to locate and read relevant type definitions, public traits/interfaces, and safe/unsafe boundaries. Identify existing invariants, state machines, and constructors.
3. **Safety & FFI Boundaries**: In Rust files, audit safe/unsafe boundaries. Note any `unsafe` block or LibRaw interaction, and identify the required safety annotations.

### Phase B: Flow & Behavior Analysis
1. **Trace Control & Data Flow**: Trace the logical execution path from the CLI/application entry point down to the specific functions or structures targeted for modification. Ensure you trace how data flows into, through, and out of the component.
2. **Silent Failure & Error Handling Baseline**: Audit how errors are currently handled in this area. Look for potential places where errors are swallowed or ignored (e.g., `let _ = ...`, broad catches) to establish a baseline.
3. **Existing Test Coverage Audit**: Check what tests are already running for this module, where they are located, and what scenarios (especially edge cases and error paths) they cover.

### Phase C: Technical Spiking & Code Verification
- Run fast compiling checks or search queries to ensure your understanding of types is 100% physically accurate.
- **Capture Discoveries**: Document integration points, observed design patterns, existing constraints, and technical debts. **Do not begin writing the plan from memory or assumptions; prove the code's shape via live file views.**

---

## 7. Architectural Blueprinting Phase (Emulating Code-Architect)

Before engaging the user with clarifying questions or drafting the session plan, construct a detailed **Architectural Blueprint** for the target feature:
- **Data Model & Type Invariants**: Design any new structs, enums, traits, or type-level invariants. Decide how compile-time safety can be leveraged (e.g., using type-safe builders or specialized wrapper types instead of raw primitives).
- **Error Boundaries & Propagation Strategy**: Define the error enum changes, how errors will propagate, and how silent failures will be prevented at the boundary of your new implementation.
- **Cognitive Simplification Targets**: Identify any overly complex existing patterns (such as nested loops, deep branching, or bloated functions) that you can simplify or refactor as part of your implementation, reducing overall cognitive load.

---

## 8. Clarifying Questions Milestone

Once the exploration and architectural blueprinting are complete, and before drafting the final session plan, the agent MUST engage the user to resolve ambiguities:
- **Formulate Deep, Non-Obvious Questions**: Group your questions into the following categories:
  1. **Architectural & Design Trade-offs**: (e.g., performance vs. complexity, compile-time vs. runtime safety, choice of patterns, FFI boundary isolation).
  2. **Requirements & Edge Cases**: (e.g., how to handle corrupted input files, missing fields, timeout behavior, out-of-bounds inputs).
  3. **Integration & Regression Risks**: (e.g., downstream side-effects, how the changes affect existing callers or FFI bindings, impact on testing suite).
- **Ask the User**: Present these questions in a clear, highly structured list to the user and request feedback.
- *Non-interactive exception*: If running in headless/CI mode, the agent must document these questions, state its chosen assumptions, and outline the corresponding risks inside the top block of the plan file.

---

## 9. Draft the Session Contract

Only after resolving clarifying questions (or documenting assumptions) can you declare the session goal by writing the top block of `docs/plans/session-NN.md`. The plan must contain:
- The overall session goal.
- What will exist by the end of the session.
- What is explicitly out of scope (deferrals routed to `TECH-DEBT.md`).
- How each deliverable is tested (unit vs. integration, test vectors).
- Which quality checkpoints and review gates will fire.
- A summary of discoveries and assumptions resolved during the discovery phase.

## 10. Handoff to Plan Review
Commit the plan's top block and tell the user the plan is ready for the `plan-review` skill.

**CRITICAL SAFEGUARD**: Never write, edit, or modify any production, library, application, or test code during or after session-start. The agent must strictly wait until `plan-review` is completely finished, remediated, approved, and the user explicitly grants permission to start the implementation.
