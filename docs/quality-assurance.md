# Quality Assurance — photohelper

> The review engine. This document is **stack-agnostic** — it describes the
> review *discipline*, not any language's tooling. The concrete quality gates
> (formatter, linter, test runner, dependency audit) live in
> `stacks/rust.md` (`stacks/rust.md`) and are wired into
> `justfile` + `.github/workflows/ci.yml`.
>
> This protocol catches defect classes a single PR review never would. The
> agents are language-aware; the discipline is language-independent.

---

## Philosophy

Quality is continuous, not a phase. This protocol makes quality checks
*unavoidable* at the points that matter — session start, plan authoring,
implementation, session end — while keeping friction proportional to scope.

The discipline is the product. A change that "works" but skipped a checkpoint
is not done.

---

## The agent suite

A review is performed by launching specialized Claude Code subagents **in
parallel** (a single message with multiple `Agent` tool calls). Every agent
runs on every artifact at a full checkpoint; the uniformity is the discipline.

| # | Agent                                     | Primary lens                                                                 |
|---|-------------------------------------------|------------------------------------------------------------------------------|
| 1 | `general-purpose`                         | Cross-cutting consistency: docs vs. code, plan vs. implementation, session-plan vs. session-state claims |
| 2 | `feature-dev:code-architect`              | Architecture, module/package boundaries, algorithmic correctness, SLO credibility |
| 3 | `feature-dev:code-reviewer`               | Logic errors, security, project-convention adherence (CLAUDE.md compliance)  |
| 4 | `pr-review-toolkit:type-design-analyzer`  | Encapsulation, invariant expression, invariant enforcement, usefulness       |
| 5 | `pr-review-toolkit:silent-failure-hunter` | Error swallowing, missing alerts, fail-open holes, retry-without-DLQ          |
| 6 | `pr-review-toolkit:comment-analyzer`      | Prose accuracy, cross-reference correctness, count drift, heading structure  |
| 7 | `pr-review-toolkit:pr-test-analyzer`      | Test-coverage adequacy, edge-case gaps, unit-vs-integration balance          |
| 8 | `pr-review-toolkit:code-simplifier`       | Complexity reduction, pattern consolidation, unused-abstraction removal      |

**Why a suite, not one reviewer:** different agents catch different classes of
issues. `type-design-analyzer` finds bypass holes `comment-analyzer` cannot;
`silent-failure-hunter` finds suppressed errors `code-reviewer` dismisses;
`code-simplifier` prevents accumulated complexity `code-reviewer` accepts as
"already written." The multi-lens parallel review is the quality ceiling —
anything less leaves surface uncovered.

### Agent-availability fallback (read this once per machine)

Agents 2–8 are provided by the `feature-dev` and `pr-review-toolkit` plugins. If
a teammate's Claude Code does not have them installed, the review still runs —
substitute `general-purpose` agents with the role prompt inlined (e.g.
"Act as a type-design analyzer: assess encapsulation, invariant expression,
and enforcement…"). The lens matters, not the plugin. Install the real agents
when you can:

```
/plugin marketplace add anthropics/claude-code      # or your internal mirror
/plugin install feature-dev
/plugin install pr-review-toolkit
```

The `eng-protocol` plugin's `/eight-agent-review` skill performs this
substitution automatically when an agent type is unavailable.

---

## Double-review protocol

Every review is a **double review**: Round 1 → remediate → Round 2 →
remediate. Round 2 regressions introduced by Round 1 remediation are expected;
catching them is the point.

```
┌────────────────────┐  ┌────────────┐  ┌────────────────────┐  ┌────────────┐
│ Write / edit the   │→ │ Round 1:   │→ │ Consolidate by     │→ │ Batch      │
│ artifact           │  │ agents in  │  │ theme, not by      │  │ edits to   │
│                    │  │ parallel   │  │ agent              │  │ address    │
└────────────────────┘  └────────────┘  └────────────────────┘  └────────────┘
                                                                       │
                                                                       ▼
┌────────────────────┐  ┌────────────┐  ┌────────────────────┐  ┌────────────┐
│ Done. Commit +     │← │ Batch      │← │ Consolidate +      │← │ Round 2:   │
│ update             │  │ edits to   │  │ triage regressions │  │ agents in  │
│ SESSION-STATE      │  │ address    │  │                    │  │ parallel   │
└────────────────────┘  └────────────┘  └────────────────────┘  └────────────┘
```

**Never** stop after Round 1 remediation. **Never** ask "should we run
Round 2?" — it is mandatory. If Round 2 surfaces regressions large enough to
need another cycle, add Round 3. Extra rounds beat shipping known regressions.

---

## Review cadence — choose one per repo

Two cadences are supported. Pick one at adoption time and record the choice in
`CLAUDE.md`. They differ only in *how many* agents fire at the smaller
checkpoints; the double-review structure is identical.

### Cadence A — tier-graduated (DEFAULT for team repos)

Scale the agent count to the blast radius of the change. A cheap session-start
alignment pass; the full suite only where it pays off.

| Tier | Trigger                                          | Agents                                  | Double-review? |
|------|--------------------------------------------------|-----------------------------------------|----------------|
| 1    | Session start                                    | 1 (`general-purpose`, `haiku`, ~5 min)  | No (alignment) |
| 2    | Trivial single-file doc/config tweak             | 1 (`code-reviewer`)                      | No             |
| 3    | Bug fix bounded to one module/package            | 1–2                                      | Yes            |
| 4    | Multi-file refactor / new public API / new module| 3–5                                      | Yes            |
| 5    | Plan review · cross-cutting feature · session end | **Full 8**                              | Yes            |

This is the right default when a team ships frequently and the cost of 32
agent-fires on every small PR would outweigh the benefit.

### Cadence B — full suite at every checkpoint

Every checkpoint — session start, plan review, sub-component review, session
end — runs the full 8-agent × double-review cadence.

Choose this when the artifact's **credibility is the product** (a verification
harness, a security control, a compliance surface, a shared SDK) and
under-reviewing even a small change carries outsized risk. It is more
expensive and slower; that is the trade.

> **Recorded choice:** `A (tier-graduated — solo project)` (set in `CLAUDE.md § Quality gates`).

---

## Checkpoints

| Checkpoint               | When                                                        | Blocking? | Artifact produced                                          |
|--------------------------|------------------------------------------------------------|-----------|------------------------------------------------------------|
| **Session start**        | Beginning of a session                                     | Yes       | Short alignment note atop the new `docs/plans/session-NN.md` |
| **Plan review**          | After `docs/plans/session-NN.md` is authored, BEFORE code  | Yes       | `docs/code-reviews/session-NN-plan-round{1,2}.md`          |
| **Sub-component review** | At every module/package boundary within a session          | Yes       | `docs/code-reviews/session-NN-<component>-round{1,2}.md`   |
| **Session-end review**   | After the session's code is complete, before commit + push | Yes       | `docs/code-reviews/session-NN-round{1,2}.md`               |

A session that skips a checkpoint has not completed, regardless of whether the
code "works."

---

## Session-start protocol

1. Ensure local `main` is current, then branch:
   ```bash
   git switch main && git pull --ff-only origin main
   git switch -c session-NN/<kebab-slug>
   ```
   Working directly on `main` is never allowed.
2. `just session-start` — runs `scripts/verify-state.sh`; prints the required-reading list.
3. Read, in order: `SESSION-STATE.md`, the latest `docs/code-reviews/session-*-round2.md` (unresolved Round-2 items), `HANDOFF_REPORT.md`, and `docs/discovery-notes.md`. Resolve any blocking item. Do **not** plan on top of unresolved Round-2 regressions.
4. **Codebase Exploration & Discovery (Code-Explorer Emulation)**:
   - Methodically audit directories, public APIs, type invariants, Rust safe/unsafe boundaries, control/data flow, error-handling/silent-failure baselines, and existing test coverage before authoring the plan.
5. **Architectural Blueprinting (Code-Architect Emulation)**:
   - Formulate a clean architectural blueprint covering new types, compile-time safety checks, error-handling propagation, and cognitive simplification targets before drafting the plan.
6. **Clarifying Questions Milestone**:
   - Formulate and present deep, structured questions to the user across three key categories (Architectural Trade-offs, Requirements & Edge Cases, Integration & Regression Risks). Pause for user alignment.
7. **Declare the Session Goal**: Write the top block of `docs/plans/session-NN.md` (the session contract) incorporating discoveries and assumptions.
8. Submit the plan to the **plan-review** checkpoint.

---

## Plan-review protocol (mandatory before any code)

Run the parallel 8-agent suite against the plan document. The plan must answer:
- **What will exist by end-of-session?** (concrete files, types, behaviors)
- **What is explicitly out of scope?** (deferrals go to `TECH-DEBT.md`)
- **How is each deliverable tested?** (unit + integration boundaries named)
- **Which checkpoints fire this session, and when?**
- **What discovery items are expected?** (unknowns flagged up-front)

### Deep Remediation Blueprint (Between Review & Fixes)
Before making any batched edits to address findings (both for plan reviews and session-end/code reviews), the agent MUST draft a structured **Deep Remediation Blueprint** and present it to the user:
- **Root-Cause Analysis (RCA)**: Categorize and analyze findings by Failure Mode (e.g., *Type Invariant Bypass*, *Silent Failure Path*) and trace why the initial design or code allowed this gap.
- **Multi-Option Evaluation**: For CRITICAL/HIGH findings, contrast at least two alternative architectural approaches and justify the choice.
- **Traceability Matrix**: Explicitly link Finding IDs (from the 9th Agent's verification) to the target files, line numbers, and scopes.
- **Regression Assessment**: Identify side-effects on downstream callers/bindings and define verification checklists.
- **Interactive Approval Milestone**: Explicitly pause and await user authorization before editing any plan or source files.

Re-run Round 2 on the remediated plan. Only after Round 2 is complete, clean, and remediated, begin writing code.

**Why so strict at the plan stage?** Code written from a flawed plan needs both the plan *and* the code re-reviewed. Plan review is the cheapest defect-removal point in the entire pipeline.

---

## Sub-component review protocol

Fires at every package/module boundary within a session, not just at session
end. Triggers:

- A package first exposes a non-scaffold public API.
- A file/module grows past ~300 lines of non-test code.
- A decision lands that materially affects downstream packages (record a
  `docs/decisions/` note first, then review).

Same agents, same double-review cadence (count per the chosen cadence).

---

## Session-end protocol

1. Final double-review on all code written in the session.
2. Artifacts committed to `docs/code-reviews/session-NN-round{1,2}.md`.
3. `SESSION-STATE.md` updated: "Last session", "Next action", "Status",
   component-progress table.
4. `HANDOFF_REPORT.md` checkpoint appended.
5. `docs/discovery-notes.md` checkpoint appended if new findings surfaced.
6. Conventional-commit commits on the branch (one logical change per commit;
   many commits per session is welcome — they preserve the PR narrative).
7. `just session-end` (runs `just ci`) — must be green before push.
8. Push the branch and open a PR targeting `main`:
   ```bash
   git push -u origin session-NN/<kebab-slug>
   gh pr create --base main --head session-NN/<kebab-slug> \
       --title "session NN: <one-line summary>" \
       --body "<PR body pointing at docs/plans/session-NN.md + the Round-2 review>"
   ```
9. Wait for every CI job to go green (`gh pr checks --watch`). Never merge with
   yellow/red checks; investigate failures.
10. Merge with a **merge commit** (not squash) to preserve the per-session
    commit chain:
    ```bash
    gh pr merge --merge --delete-branch
    ```
11. Render the **session-handoff response** per `docs/session-handoff-format.md`
    — the summary table + next-session bash bootstrap, as the final two blocks.

---

## Findings triage

| Severity | Definition                                                            | Response                            |
|----------|----------------------------------------------------------------------|-------------------------------------|
| CRITICAL | Blocks implementation, architectural flaw, data-loss/correctness risk | Immediate (same round)              |
| HIGH     | Significant bug, security issue, major deviation from design          | Before the next checkpoint          |
| MEDIUM   | Quality issue, minor deviation, missing test                         | Before session end                  |
| LOW      | Style, doc polish, minor optimization                                | When convenient; OK to defer to `TECH-DEBT.md` with a plan |

Deferrals go to `TECH-DEBT.md` with a concrete remediation plan and a binding
trigger (see `CLAUDE.md § No Acceptable Trade-offs Policy`). **A deferral
without a plan is a CRITICAL finding on its own.**

---

## Consolidation discipline

**Group findings by THEME, not by agent.** Example:

```markdown
### Theme: error type leaks across the public API

- [general-purpose]: scaffold error appears in README examples (HIGH)
- [code-reviewer]: variant name leaks into generated docs (MEDIUM)
- [comment-analyzer]: docstrings cite session numbers that will shift (LOW)

Remediation: hide the variant; add TECH-DEBT.md entry TD-001; stop citing
specific session numbers and link to SESSION-STATE.md instead.
```

Per-agent organization hides that the same issue surfaced three times — which
is the signal that it's worth prioritizing.

---

## State & Context Synchronization Discipline

To support seamless parallel development, multi-agent execution, and flawless handover between different LLM brains or automated harnesses, all agents MUST maintain perfect synchronization across our shared context files and workspace ledgers.

### 1. Unified Reference Identity (URI)
Any technical debt, decision, bug report, or architectural change must carry a unique and globally consistent reference identifier across all files:
- **Technical Debt**: Must use the identifier format `TD-NNN` (where `NNN` is a sequential index from `TECH-DEBT.md`).
- **Architectural Decisions**: Must use the identifier format `ADR-NNNN` (where `NNNN` is a sequential index from `docs/adr/`).
- **Bug Investigations**: Must use the identifier format `BUG-NNN` (where `NNN` is a sequential index from `docs/bugs/`).
Any update to one ledger referencing these IDs must immediately update all corresponding referencing ledgers (`SESSION-STATE.md`, `HANDOFF_REPORT.md`, `TECH-DEBT.md`) in the *same* session commit, eliminating stale pointers.

### 2. High-Density, Non-Summarized Documentation
When documenting session plans, goal accomplishments, design gaps, or remediation blueprints, agents MUST avoid vague, high-level summaries (e.g., "Updated decoders to fix errors").
Instead, write **high-density, physically precise context**:
- Cite the **exact** file names, line ranges, and fully-qualified types or functions changed (e.g., `src/raw.rs:L145-180`, `struct RawDecoder`, `fn decode_cr3`).
- Detail the **exact** failure modes, returned error types, and concrete safety invariants enforced.
- Document the *rationale* and technical trade-offs behind each decision so that downstream agents can ingest the state with zero ambiguity.

### 3. Machine-Parsable Schema Formats
Ledgers and reports are parsed both by humans and automated script chains. Agents must strictly adhere to the defined metadata formats:
- Keep the `yaml` frontmatter and structured tables in all session plans, reviews, and handoff reports syntactically valid and physically precise.
- Maintain the column headers, labels, and status flags of the progress tables inside `SESSION-STATE.md` and `HANDOFF_REPORT.md` verbatim.

### 4. Zero-Friction Handover State
At the end of every session, the final handoff documentation must represent a complete, runnable state transition. It must explicitly specify:
- The exact `bash` bootstrap command to start the next session.
- Any open Round-2 items, pending linting issues, or known blocked states.
- The precise list of context files and directories the next agent or LLM must read first to immediately gain 100% domain context.

---

## Enforcement

This protocol is enforced, not merely documented:

- `SESSION-STATE.md` — read on session start (`just session-start`).
- `CLAUDE.md` — declares the session protocol mandatory.
- `scripts/verify-state.sh` — flags missing `docs/quality-assurance.md` or
  `SESSION-STATE.md` as `blocked`; runs in CI and as a pre-push hook.
- `docs/code-reviews/` — git history of review artifacts is the audit trail.

---

## Metrics worth tracking across sessions

| Metric                              | Target | Where tracked                          |
|-------------------------------------|--------|----------------------------------------|
| CRITICAL findings open at session end | 0    | `SESSION-STATE.md` status line         |
| HIGH findings carried to next session | ≤ 2  | `SESSION-STATE.md` "Open Round-2 items"|
| Test pass rate                      | 100%   | `just ci` at session end               |
| Lint warnings                       | 0      | CI                                     |
