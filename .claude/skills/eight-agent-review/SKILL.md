---
name: eight-agent-review
description: The eng-protocol review engine. Launches the specialized review-agent suite IN PARALLEL against a target (a plan, a code change, or the whole session), consolidates findings by THEME (not by agent), triages by severity, and writes a docs/code-reviews artifact. Honors the repo's recorded review cadence (A tier-graduated vs B full-suite). Use whenever a checkpoint requires a review; the plan-review and session-end skills delegate to this one.
argument-hint: "[plan|code|session] [scope or paths]"
allowed-tools: Read Grep Glob Agent Write Edit
---

You are running the eng-protocol review engine. The authoritative spec is this
repo's `docs/quality-assurance.md` (§ The agent suite, § Double-review protocol,
§ Findings triage, § Consolidation discipline). Follow it.

## 1. Determine cadence and agent count

Read `CLAUDE.md § Quality gates` for the recorded cadence:
- **Cadence B** (full suite): launch all 8 agents.
- **Cadence A** (tier-graduated): scale to blast radius — 1 agent for a trivial
  doc tweak; 3–5 for a multi-file change; **all 8** for plan review, a
  cross-cutting feature, or session end.

## 2. Launch the suite IN PARALLEL

Issue the agent calls in a SINGLE message (multiple `Agent` tool calls) so they
run concurrently. Roster and lens:

| Agent | Lens |
|-------|------|
| `general-purpose` | cross-cutting consistency (docs vs code, plan vs impl) |
| `feature-dev:code-architect` | architecture, boundaries, algorithmic correctness |
| `feature-dev:code-reviewer` | logic errors, security, CLAUDE.md conventions |
| `pr-review-toolkit:type-design-analyzer` | encapsulation, invariant expression + enforcement |
| `pr-review-toolkit:silent-failure-hunter` | swallowed errors, fail-open holes, missing alerts |
| `pr-review-toolkit:comment-analyzer` | prose accuracy, cross-refs, count drift |
| `pr-review-toolkit:pr-test-analyzer` | coverage adequacy, edge-case + error-path gaps |
| `pr-review-toolkit:code-simplifier` | complexity reduction, dead-abstraction removal |

**Fallback:** if an agent type is unavailable here, substitute a
`general-purpose` agent and inline its lens in the prompt ("Act as a type-design
analyzer: assess encapsulation, invariant expression, and enforcement…"). The
lens matters, not the plugin.

Give every agent the same scope (the `$ARGUMENTS` target) and ask each for a
findings list with severities and concrete file:line references.

## 3. Consolidate by THEME, not by agent

Group findings so that when three agents flag the same issue it appears once, as
one theme with the three agents cited — that overlap is the priority signal.
Never organize the artifact under per-agent headings.

## 4. Triage

Assign CRITICAL / HIGH / MEDIUM / LOW per `docs/quality-assurance.md § Findings
triage`. Route deferrals to `TECH-DEBT.md` with a binding trigger; a deferral
without a plan is a CRITICAL finding on its own.

## 5. Write the artifact

Write to the correct file: plan reviews →
`docs/code-reviews/session-NN-plan-round{R}.md`; code/session reviews →
`docs/code-reviews/session-NN-round{R}.md`. One theme per section, agents cited
in brackets, remediation noted.

This skill performs ONE round. The caller (plan-review / session-end) runs it
twice: Round 1 → remediate → Round 2 → remediate. Never stop after Round 1.
