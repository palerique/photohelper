---
name: eight-agent-review
description: The eng-protocol review engine for Antigravity. Launches the specialized review-agent suite IN PARALLEL using `invoke_subagent` with the `self` subagent, consolidates findings by THEME, triages by severity, verifies findings via a 9th Agent with verbatim Read-window quotation, and writes a docs/code-reviews artifact with machine-verifiable YAML markers. Honors the repo's recorded cadence (A tier-graduated). Use whenever a checkpoint requires a review; plan-review and session-end delegate to this one.
argument-hint: '[plan|code|session] [scope or paths]'
allowed-tools: run_command view_file grep_search list_dir write_to_file replace_file_content invoke_subagent send_message
---

You are running the eng-protocol review engine under the Antigravity CLI. The authoritative spec is this repo's `docs/quality-assurance.md` (§ The agent suite, § Double-review protocol, § Findings triage, § Consolidation discipline). Follow it.

## 0. Precondition gate (session-config check with memoization)

Before launching any sub-agent work, verify the parent session is configured to deliver review-grade quality. Pattern adapted for our Cadence A in Antigravity.

### 0.a Check the session-config cache

The cache lives at `.eng-protocol/session-config-cache.json` (created/cleared by session-start, when wired). Read it with the `view_file` tool (or `run_command` with `cat`). If present, SKIP the prompt and reuse the cached `gate_state` + `session_config` block. Subsequent invocations within the same session (Round 1 + Round 2 of plan-review; Round 1 + Round 2 of session-end) fire the prompt EXACTLY ONCE.

If no cache: proceed to §0.b. After the gate completes, WRITE the resulting `session_config` block to `.eng-protocol/session-config-cache.json` so subsequent invocations reuse it. Create the directory if missing.

### 0.b First-invocation gate (AskUserQuestion or direct text inquiry)

Use the `ask_question` tool if available, or ask the user directly in the chat with these THREE PARALLEL options (use this phrasing verbatim — UX consistency makes the gate auditable across sessions):

- **Option 1**: "I'm on Gemini 1.5 Pro / 2.0 / Flash [MAX effort] → proceed at full quality (gate_state: pass)"
- **Option 2**: "Acknowledge sub-spec configuration; proceed at reduced quality (gate_state: downgraded-acknowledged + visual callout in artifact)"
- **Option 3**: "Halt — let me reconfigure my session (gate_state: aborted-by-user)"

Map responses to `gate_state`:

| user_response | gate_state |
|---|---|
| option-1 | pass |
| option-2 | downgraded-acknowledged |
| option-3 | aborted-by-user — HALT, do not write artifact |
| Other / null | aborted-by-user — HALT and re-prompt; NEVER default to option-2 |

### 0.c Non-interactive mode

If user interaction is unavailable (headless invocation / CI / automated scripts), emit a stderr warning ("eight-agent-review running in non-interactive mode; cannot verify session config — proceeding with gate_state: downgraded-no-prompt") and proceed with `gate_state: downgraded-no-prompt`.

### 0.d User dismisses prompt

If the user closes the prompt without answering: treat as `gate_state: aborted-no-response` (distinct from explicit Option 3 `aborted-by-user`). HALT either way.

### 0.e Emit machine-verifiable YAML marker

The artifact's FIRST CONTENT SECTION (immediately after the `# Title` line) MUST be a fenced `yaml` block with the `session_config` schema.

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

## 1. Subagent Invocation Strategy in Antigravity

In Antigravity, we utilize the `invoke_subagent` tool with `TypeName: "self"` (which inherits full tools, configuration, and parent model). We specify a distinct `Role` and a tailored `Prompt` containing the specific lens for each of the 8 agents. This guarantees that all required specialized lenses are executed without needing any external plugins!

Since we use `self` subagents, set the `plugin_availability` block in the artifact as follows:

```yaml
plugin_availability:
  schema_version: 1
  agents_requested: ["general-purpose", "code-architect", "code-reviewer", "type-design-analyzer", "silent-failure-hunter", "comment-analyzer", "pr-test-analyzer", "code-simplifier"]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## 2. Determine cadence and agent count

Read `CLAUDE.md § Quality gates` for the recorded cadence (currently **A** for photohelper — solo project; tier-graduated):

- **Cadence A** (tier-graduated): scale to blast radius — 1 agent for a trivial doc tweak; 3–5 for a multi-file change; **all 8** for plan review, a cross-cutting feature, or session end.
- **Cadence B** (full suite): launch all 8 agents.

## 3. Launch the suite IN PARALLEL

Issue the agent calls in a SINGLE call to `invoke_subagent` by passing all 8 subagents in the `Subagents` array so they run concurrently.

Roster and roles:

| Subagent Role | Lens / Purpose |
|---------------|----------------|
| `General Consistency Analyst` | cross-cutting consistency (docs vs code, plan vs impl) |
| `Code Architect` | architecture, boundaries, algorithmic correctness |
| `Code Reviewer` | logic errors, security, CLAUDE.md conventions |
| `Type Design Analyzer` | encapsulation, invariant expression + enforcement |
| `Silent Failure Hunter` | swallowed errors, fail-open holes, missing alerts |
| `Comment Analyzer` | prose accuracy, cross-refs, count drift |
| `PR Test Analyzer` | coverage adequacy, edge-case + error-path gaps |
| `Code Simplifier` | complexity reduction, dead-abstraction removal |

### 3.a Rigorized sub-agent prompt template (sentinel-marker-enforced)

Each prompt MUST contain these 5 sentinel sections (orchestrator self-check via anchored-line regex `^# Scope\b`, `^# Lens\b`, `^# Required citations\b`, `^# Adversarial framing\b`, `^# Anti-confirmation deliverable\b` before invoking):

```
# Scope
<paths + diff range or plan file location>

# Lens
<specific lens for this agent's specialty>

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

AFTER consolidation by theme, BEFORE writing the artifact: spawn a 9th subagent (`TypeName: "self"` with `Role: "Verification Agent"`). This catches hallucinated findings before they enter the audit trail.

```
You are the verification agent for an eng-protocol code review. For each consolidated finding in the input list, you MUST:
1. Read the file ±5 line window around the cited location.
2. Inspect: does the window contain the pattern the original finding describes?
3. Emit ONE YAML row per finding with these exact keys:
   - finding_id: <sha1(theme_letter + "::" + file + ":" + line + ":" + first-32-chars-of-message)>
   - file: <string>
   - line: <int>
   - present: yes | drifted | no | file-unreadable
   - evidence_snippet: <verbatim quote of the read window — MUST be substring-greppable against the file>
   - retain: yes | yes-with-corrected-line | no | yes-flag-for-human-triage
   - reason: <one-line>

Output as a single YAML document inside a fenced yaml block. No prose, no preamble. If you cannot read a file, emit the row with present=file-unreadable rather than omitting.

Findings: <consolidated findings list>
```

### 6.a Orchestrator post-hoc substring-grep (turtle-layer-2 halt)

For each row in the 9th agent's output, the orchestrator (this main agent) verifies the `evidence_snippet` by re-reading the cited file and substring-matching. If the snippet does NOT appear in the file: mark the finding `compromised`, RETAIN with flag (never silently discard).

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

If the 9th agent emits malformed YAML: write the raw output to `verification.raw_output_unparseable: true` AND halt before publishing the artifact; request human triage.

## 7. Write the artifact

Write to the correct file per `docs/quality-assurance.md § Checkpoints`:

- Plan reviews → `docs/code-reviews/session-NN-plan-round{R}.md`
- Code/session reviews → `docs/code-reviews/session-NN-round{R}.md`

**File template**:

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
