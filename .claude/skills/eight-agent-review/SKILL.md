---
name: eight-agent-review
description: The eng-protocol review engine. Launches the specialized review-agent suite IN PARALLEL against a target (a plan, a code change, or the whole session), consolidates findings by THEME (not by agent), triages by severity, verifies findings via a 9th Agent with verbatim Read-window quotation, and writes a docs/code-reviews artifact with machine-verifiable YAML markers. Honors the repo's recorded cadence (A tier-graduated). Use whenever a checkpoint requires a review; plan-review and session-end delegate to this one.
argument-hint: '[plan|code|session] [scope or paths]'
allowed-tools: Read Grep Glob Agent Write Edit Bash(test *) Bash(cat *) Bash(mkdir *)
color: cyan
---

You are running the eng-protocol review engine. The authoritative spec is this repo's `docs/quality-assurance.md` (§ The agent suite, § Double-review protocol, § Findings triage, § Consolidation discipline). Follow it.

## 0. Precondition gate (session-config check with memoization)

Before launching any sub-agent work, verify the parent session is configured to deliver review-grade quality. Pattern borrowed from fox's eng-protocol upgrade (2026-05-28) and adapted for our Cadence A.

### 0.a Check the session-config cache

The cache lives at `.eng-protocol/session-config-cache.json` (created/cleared by session-start, when wired). Read it with `Bash(cat .eng-protocol/session-config-cache.json 2>/dev/null)`. If present, SKIP the prompt and reuse the cached `gate_state` + `session_config` block. Subsequent invocations within the same session (Round 1 + Round 2 of plan-review; Round 1 + Round 2 of session-end) fire the prompt EXACTLY ONCE.

If no cache: proceed to §0.b. After the gate completes, WRITE the resulting `session_config` block to `.eng-protocol/session-config-cache.json` so subsequent invocations reuse it. Create the directory if missing.

### 0.b First-invocation gate (3-option AskUserQuestion)

Fire `AskUserQuestion` with these THREE PARALLEL options (use this phrasing verbatim — UX consistency makes the gate auditable across sessions):

- **Option 1**: "I'm on Opus 4.7 [1m] + MAX effort + all review plugins installed (`feature-dev`, `pr-review-toolkit`) → proceed at full quality (gate_state: pass)"
- **Option 2**: "Acknowledge sub-spec configuration; proceed at reduced quality (gate_state: downgraded-acknowledged + visual callout in artifact)"
- **Option 3**: "Halt — let me reconfigure my session (gate_state: aborted-by-user)"

Map responses to `gate_state`:

| user_response | gate_state |
|---|---|
| option-1 | pass |
| option-2 | downgraded-acknowledged |
| option-3 | aborted-by-user — HALT, do not write artifact |
| Other / multi-select / null | aborted-by-user — HALT and re-prompt; NEVER default to option-2 |

### 0.c Non-interactive mode

If `AskUserQuestion` is unavailable (e.g., `claude --print` / SDK / CI-headless invocation), emit a stderr warning ("eight-agent-review running in non-interactive mode; cannot verify session config — proceeding with gate_state: downgraded-no-prompt") and proceed with `gate_state: downgraded-no-prompt`.

### 0.d User dismisses prompt

If the user closes the AskUserQuestion modal without answering (timeout, ESC, terminal closed): treat as `gate_state: aborted-no-response` (distinct from explicit Option 3 `aborted-by-user`). HALT either way.

### 0.e Emit machine-verifiable YAML marker

The artifact's FIRST CONTENT SECTION (immediately after the `# Title` line) MUST be a fenced `yaml` block with the `session_config` schema. Photohelper does not yet have a `scripts/verify-review-artifact.sh` enforcer (TD candidate: port fox's mjs version); for now the YAML block is a discipline marker that the simplifier/comment-analyzer agents can read in follow-up rounds.

```yaml
session_config:
  schema_version: 1
  model_claimed: <string>          # what orchestrator believes parent is on (best-effort)
  model_observed: unverifiable     # until a runtime-API gap closes
  effort_claimed: <string>         # MAX / high / medium / low / unknown
  effort_observed: unverifiable
  ask_user_question_id: <string|null>
  user_response: option-1 | option-2 | option-3 | other-or-null | not-asked
  gate_state: pass | downgraded-acknowledged | downgraded-no-prompt | aborted-by-user | aborted-no-response | hard-failed-fallback
  cache_used: true | false         # true if reused from .eng-protocol/session-config-cache.json
```

If `gate_state` ∈ {`downgraded-acknowledged`, `downgraded-no-prompt`}, ALSO emit a visual callout block immediately after the YAML marker:

```
> ⚠️ **Session config below spec**: this review was produced under [downgraded conditions / non-interactive mode]. The quality ceiling is bounded by the parent session's actual model + effort.
```

## 1. Plugin-availability detection (post-hoc error-catching)

The Agent tool **errors** on unregistered `subagent_type` with the message format `Agent type 'X' not found. Available agents: [list]`. The tool does NOT silently substitute.

Pre-launch: emit advisory once per session: "This skill assumes feature-dev + pr-review-toolkit plugins are installed. If any specialized agent is unavailable, the Agent tool will error; this skill catches that and lets you opt into general-purpose fallback per-agent."

Per Agent invocation:

- Wrap the `Agent` call in error-handling.
- On error matching `Agent type '<X>' not found`: parse the "Available agents: [...]" tail to discover registered roster; log missing agent in `plugin_availability` block; ask `AskUserQuestion` whether to fall back to `general-purpose` with inlined lens prompt OR halt. **Default: HALT with `/plugin install feature-dev` / `/plugin install pr-review-toolkit` instructions**; user opt-in for fallback.

Emit `plugin_availability` as a typed YAML block in the artifact (after `session_config`):

```yaml
plugin_availability:
  schema_version: 1
  agents_requested: [string]       # the 7 specialized agents the suite needs
  agents_unavailable: [string]     # those the Agent tool refused
  fallback_used: true | false
  fallback_agents: [string]        # which agents fell back to general-purpose
```

## 2. Determine cadence and agent count

Read `CLAUDE.md § Quality gates` for the recorded cadence (currently **A** for photohelper — solo project; tier-graduated):

- **Cadence A** (tier-graduated): scale to blast radius — 1 agent for a trivial doc tweak; 3–5 for a multi-file change; **all 8** for plan review, a cross-cutting feature, or session end.
- **Cadence B** (full suite): launch all 8 agents.

## 3. Launch the suite IN PARALLEL

Issue the agent calls in a SINGLE message (multiple `Agent` tool calls) so they run concurrently. **Pin `model: "opus"` on every invocation** — this is the single most direct lever to standardize sub-agent quality regardless of parent-session model selection.

Roster and lens:

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

### 3.a Rigorized sub-agent prompt template (sentinel-marker-enforced)

Each prompt MUST contain these 5 sentinel sections (orchestrator self-check via anchored-line regex `^# Scope\b`, `^# Lens\b`, `^# Required citations\b`, `^# Adversarial framing\b`, `^# Anti-confirmation deliverable\b` BEFORE issuing the 8 Agent calls; abort if any missing).

```
# Scope
<paths + diff range or plan file location>

# Lens
<verbatim from docs/quality-assurance.md § The agent suite for this agent>

# Required citations
For every finding: severity (CRITICAL / HIGH / MEDIUM / LOW) + file:line + concrete remediation suggestion.

# Adversarial framing
Find the bug. Do not confirm correctness. Treat the artifact as guilty until proven innocent.

# Anti-confirmation deliverable
If your lens surfaces nothing, list 2-4 things you considered and ruled out, with one-line reasoning each. A clean review must be JUSTIFIED, not asserted. Look for what is NOT yet on the list (bug classes peculiar to this diff). Surfacing a new class is high-signal.
```

After each sub-agent returns, perform the self-check before consolidation.

## 4. Consolidate by THEME, not by agent

Group findings so that when three agents flag the same issue it appears once, as one theme with the three agents cited — that overlap is the priority signal. Never organize the artifact under per-agent headings.

## 5. Triage

Assign CRITICAL / HIGH / MEDIUM / LOW per `docs/quality-assurance.md § Findings triage`. Route deferrals to `TECH-DEBT.md` with a binding trigger; a deferral without a plan is a CRITICAL finding on its own.

## 6. Post-review verification step (9th Agent with verbatim Read-window quotation)

AFTER consolidation by theme, BEFORE writing the artifact: spawn a 9th `Agent` invocation as the verification agent. This catches hallucinated findings before they enter the audit trail.

```
subagent_type: "general-purpose"
model: "opus"
description: "Verify Round-N findings against source"
prompt: |
  You are the verification agent for an eng-protocol code review. For each consolidated finding in the input list, you MUST:
  1. Read(file, max(1, line-5) to line+5)   # ±5 line window around the cited location
  2. Inspect: does the window contain the pattern the original finding describes?
  3. Emit ONE YAML row per finding with these exact keys:
     - finding_id: <sha1(theme_letter + "::" + file + ":" + line + ":" + first-32-chars-of-message)>
     - file: <string>
     - line: <int>
     - present: yes | drifted | no | file-unreadable
     - evidence_snippet: <verbatim quote of the read window — MUST be substring-greppable against the file>
     - retain: yes | yes-with-corrected-line | no | yes-flag-for-human-triage
     - reason: <one-line>
  Mapping:
     - present=yes → retain=yes
     - present=drifted → retain=yes-with-corrected-line (update the line number in the finding)
     - present=no → retain=no (discard as hallucination)
     - present=file-unreadable → retain=yes-flag-for-human-triage (never silently discard)
  Output as a single YAML document. No prose, no preamble. If you cannot read a file, emit the row with present=file-unreadable rather than omitting.
findings: <consolidated findings list from § 4>
```

### 6.a Orchestrator post-hoc substring-grep (turtle-layer-2 halt)

For each row in the 9th agent's output, the orchestrator (NOT a 10th agent — explicit halt at turtle layer 2) verifies the `evidence_snippet` by re-reading the cited file and substring-matching. If the snippet does NOT appear in the file: mark the finding `compromised`, RETAIN with flag (never silently discard).

### 6.b Emit verification YAML block

```yaml
verification:
  schema_version: 1
  parent_gate_state: <string>          # propagates session_config.gate_state — verification quality is bounded by parent
  total_findings: N                    # invariant: == verified + drifted + hallucinated + unreadable + compromised
  verified: N                          # present=yes, evidence_snippet checks out via post-hoc grep
  drifted: N                           # present=drifted, retained with corrected line
  hallucinated: N                      # present=no, discarded
  unreadable: N                        # present=file-unreadable, retained with human-triage flag
  compromised: N                       # 9th agent's evidence_snippet did NOT substring-match → retained with flag
  discard_rate: 0.NN                   # = hallucinated / total_findings
  details:
    [{finding_id, file, line, present, retain, reason, evidence_snippet}, ...]
```

If the 9th agent emits malformed YAML (parse failure or required keys missing): write the raw output to `verification.raw_output_unparseable: true` AND halt before publishing the artifact; request human triage.

## 7. Write the artifact

Write to the correct file per `docs/quality-assurance.md § Checkpoints`:

- Plan reviews → `docs/code-reviews/session-NN-plan-round{R}.md`
- Code/session reviews → `docs/code-reviews/session-NN-round{R}.md`

**File template** (a future `scripts/verify-review-artifact.sh` enforcer will parse the first 3 YAML blocks and validate schema invariants — TD candidate):

````markdown
# Session NN — <target>, Review Round R

```yaml
session_config: ...
```

```yaml
plugin_availability: ...
```

> ⚠️ <visual callout if gate_state is downgraded> (omit if pass)

## Triage summary

<table>

## Theme A — <description>

- [agent]: finding 'SEVERITY'
- [agent]: finding 'SEVERITY'

**Remediation**: ...

## Theme B — ...

...

## Disposition summary

<table>

## Verification

```yaml
verification: ...
```
````

## 8. Round semantics

This skill performs ONE round. The caller (plan-review / session-end) runs it twice: Round 1 → remediate → Round 2 → remediate. **Never** stop after Round 1. If Round 2 surfaces CRITICAL-class regressions, add Round 3.

The precondition gate (§ 0) fires ONCE per session (memoized via `.eng-protocol/session-config-cache.json`); subsequent invocations within the same session consume the cached `session_config` block transparently.
