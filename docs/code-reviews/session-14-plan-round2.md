# Session 14 — TD-022: XMP Sidecar I/O Proper Library, Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "Gemini 3.5 Flash (High)"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
  ask_user_question_id: null
  user_response: option-1
  gate_state: pass
  cache_used: true
```

```yaml
plugin_availability:
  schema_version: 1
  agents_requested: ["general-purpose", "code-architect", "code-reviewer", "type-design-analyzer", "silent-failure-hunter", "comment-analyzer", "pr-test-analyzer", "code-simplifier"]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## Triage summary

| Theme | Severity |
|---|---|
| Theme A — Document Hierarchy and State Machine Formalization | CRITICAL |
| Theme B — Event Memory and Borrow Checker Semantics | HIGH |
| Theme C — I/O, Resource Leaks, and Error Handling | HIGH |
| Theme D — Explicit Creation vs. Stream Merging | CRITICAL |
| Theme E — Documentation, Scripts, and Test Plan | CRITICAL |

## Theme A — Document Hierarchy and State Machine Formalization

- [Type Design Analyzer]: Fragmented state machine and underflow vulnerability ('CRITICAL')
- [Code Architect]: Structural corruption via nested RDF descriptions ('CRITICAL')
- [Code Architect, Code Reviewer]: State machine desynchronization on Event::Empty tags ('HIGH')
- [Type Design Analyzer, PR Test Analyzer]: Synthesizing rdf:Description at EOF produces malformed XML ('HIGH')

**Remediation**: Use a unified `WriterState` with `NonZeroUsize` for depth to prevent underflow. Handle nested `rdf:Description` safely by tracking depth. Handle `Event::Empty` tags correctly without entering dropping state. Re-injected `rdf:Description` must be inside `x:xmpmeta/rdf:RDF`, not at EOF.

## Theme B — Event Memory and Borrow Checker Semantics

- [Code Architect]: Borrow-checker deadlock on Event reconstruction ('HIGH')
- [Type Design Analyzer, Comment Analyzer]: Weak structural guarantee for attributes and missing attribute clearing ('MEDIUM')

**Remediation**: Allocate an owned tag (`BytesStart::owned_name()`) and push attributes from the old tag + new managed tags to satisfy the borrow checker. Use a `BTreeMap` or `IndexMap` for attributes to structurally guarantee no duplicate attributes during injection, and handle attribute clearing (`Update::Clear`).

## Theme C — I/O, Resource Leaks, and Error Handling

- [Code Reviewer, PR Test Analyzer]: Resource leak on atomic write abort ('HIGH')
- [Silent Failure Hunter]: Ambiguous NotFound catch (Directory vs File) and missing Error Context ('HIGH')

**Remediation**: Use `tempfile::NamedTempFile` or explicit `fs::remove_file` to ensure temporary file cleanup on parse/write errors. Differentiate between file `NotFound` and directory `NotFound` in the creation fallback. Include context (path and `WriterState`) in error reporting.

## Theme D — Explicit Creation vs. Stream Merging

- [Code Simplifier]: Overly complex indirect abstraction for file creation ('HIGH')
- [Code Reviewer]: Missing required XMP processing instructions (xpacket) in creation ('HIGH')
- [Comment Analyzer]: Broken force-overwrite recovery path in conflict.rs ('CRITICAL')

**Remediation**: Separate "Create New File" from "Update Existing File". Bypass stream parsing entirely for creation and write the XML skeleton directly with standard XMP Processing Instructions (`xpacket`). Add a `fallback_on_parse_error` flag or explicitly delete the corrupted sidecar in `conflict.rs` to allow force-overwrite recovery.

## Theme E — Documentation, Scripts, and Test Plan

- [General Consistency]: Missing Ledger and User Doc Synchronization ('CRITICAL')
- [Comment Analyzer]: Dead Code Retention (render_xmp) and inaccurate "AST-level" terminology ('MEDIUM')
- [PR Test Analyzer, General Consistency, Comment Analyzer]: Test suite gaps (naming, impossible assertions, missing coverage) ('MEDIUM')

**Remediation**: Document unstaged script modifications in the plan or revert them. Update `SESSION-STATE.md`, `TECH-DEBT.md`, and user-facing docs. Delete the `render_xmp` function completely instead of just updating its docstring. Rewrite `write_xmp` docstring using "stream-based". Update testing section naming and use string `contains` for unknown fields test. Add specific tests for sibling preservation, namespace injection, and multiple `rdf:Description` tags.

## Disposition summary

| Theme | Disposition |
|---|---|
| Theme A | Remediate in plan |
| Theme B | Remediate in plan |
| Theme C | Remediate in plan |
| Theme D | Remediate in plan |
| Theme E | Remediate in plan |

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 14
  verified: 12
  drifted: 1
  hallucinated: 0
  unreadable: 0
  compromised: 1
  discard_rate: 0.00
  details:
    - finding_id: f1496679fed429b16ab79e0fb1f7589216aad925
      file: docs/plans/session-14.md
      line: 18
      present: 'yes'
      evidence_snippet: '- Replace monolithic loops with a strictly-typed `WriterState`
        (e.g., `PassingThrough`, `Dropping { depth: usize }`) to guarantee structural
        invariants. When dropping elements like `dc:subject`, explicitly track depth increment/decrement
        on start/end events to avoid leaking nested elements.'
      retain: 'yes'
      reason: Depth tracking logic is explicitly present and could underflow; boolean
        state is fragmented.
    - finding_id: 9e8908edfda0a34f8579b254a59d4b031fd2f9a2
      file: docs/plans/session-14.md
      line: 21
      present: 'yes'
      evidence_snippet: '- The `rdf:Description` element will be intercepted (only the
        *first* one encountered, tracked via `has_injected_keywords = false`).'
      retain: 'yes'
      reason: Tracking interception via a boolean ignores nested rdf:Description elements.
    - finding_id: 346ccc45594e32ce3c9de5f282a0694fdd1d0df3
      file: docs/plans/session-14.md
      line: 18
      present: 'yes'
      evidence_snippet: '- Replace monolithic loops with a strictly-typed `WriterState`
        (e.g., `PassingThrough`, `Dropping { depth: usize }`) to guarantee structural
        invariants. When dropping elements like `dc:subject`, explicitly track depth increment/decrement
        on start/end events to avoid leaking nested elements.'
      retain: 'yes'
      reason: Explicitly mentions start/end events but misses Event::Empty which also
        affects structure depth.
    - finding_id: 472d2e10fb17428ee2b2f7e57e2ce436b75f5274
      file: docs/plans/session-14.md
      line: 29
      present: 'yes'
      evidence_snippet: '- Verify `rdf:Description` is encountered; synthesize if missing
        to avoid silently dropping writes. Ensure atomic rename guarantees (sync/flush
        then rename) are preserved.'
      retain: 'yes'
      reason: Synthesizing an element at EOF would violate the x:xmpmeta wrapper.
    - finding_id: afb17112fccb0cdaa0ed843946d94d84c61c4c25
      file: docs/plans/session-14.md
      line: 22
      present: 'yes'
      evidence_snippet: '  - **Attributes**: Parse existing attributes into a buffer.
        Overwrite the keys for managed settings, insert namespaces only if the keys don''t
        exist, and then reconstruct and emit the modified `Event::Start`. Ensure duplicate
        attributes are structurally impossible.'
      retain: 'yes'
      reason: Modifying Event::Start from a buffer borrows it, which fails in rust quick-xml.
    - finding_id: 8fbb4394537d7f26ab95bc8554717513619ffd21
      file: docs/plans/session-14.md
      line: 22
      present: 'yes'
      evidence_snippet: '  - **Attributes**: Parse existing attributes into a buffer.
        Overwrite the keys for managed settings, insert namespaces only if the keys don''t
        exist, and then reconstruct and emit the modified `Event::Start`. Ensure duplicate
        attributes are structurally impossible.'
      retain: 'yes'
      reason: Mentions overwriting keys but not removing managed fields that should be
        cleared.
    - finding_id: 18f742c80cc300254af93fae87509856151b2fd1
      file: docs/plans/session-14.md
      line: 27
      present: 'yes'
      evidence_snippet: '- Immediately halt and abort atomic rename on any read or write
        errors (`write_event`).'
      retain: 'yes'
      reason: Halting the atomic rename aborts but fails to mention cleaning up the created
        temp file.
    - finding_id: 8b273f31e9eda90d3e737df3787e2b6520795507
      file: docs/plans/session-14.md
      line: 28
      present: 'yes'
      evidence_snippet: '- Fallback to "Create new from skeleton" ONLY on `std::io::ErrorKind::NotFound`
        during read, propagating fatal errors like `PermissionDenied`.'
      retain: 'yes'
      reason: A NotFound could refer to the directory itself, not just the file missing.
    - finding_id: 00c5e2c23d4fa50c99ef455e7ef99b314322bf2a
      file: docs/plans/session-14.md
      line: 17
      present: 'yes'
      evidence_snippet: '- Define distinct structural paths for sidecar creation vs update.
        If the file does not exist, `write_xmp` will feed a hardcoded minimal XMP skeleton
        (with `x:xmpmeta` and `rdf:Description` shells) as the source `quick_xml::Reader`
        stream to reuse the update logic.'
      retain: 'yes'
      reason: Feeding a skeleton to the reader to reuse the update path is overly indirect.
    - finding_id: 47a320d0b2179f1f0ba6dea9452bf69cd8440168
      file: docs/plans/session-14.md
      line: 17
      present: 'yes'
      evidence_snippet: '- Define distinct structural paths for sidecar creation vs update.
        If the file does not exist, `write_xmp` will feed a hardcoded minimal XMP skeleton
        (with `x:xmpmeta` and `rdf:Description` shells) as the source `quick_xml::Reader`
        stream to reuse the update logic.'
      retain: 'yes'
      reason: The skeleton mentions xmpmeta and rdf:Description but completely misses
        the vital xpacket processing instructions.
    - finding_id: bb2a221aa6b033f4e2b8b383bb769e61ec36e686
      file: docs/plans/session-14.md
      line: 27
      present: 'no'
      evidence_snippet: |-
        - Immediately halt and abort atomic rename on any read or write errors (`write_event`).
        - Fallback to "Create new from skeleton" ONLY on `std::io::ErrorKind::NotFound` during read, propagating fatal errors like `PermissionDenied`.
      retain: yes-flag-for-human-triage
      reason: There is no mention of conflict.rs or force-overwrite recovery in the relevant
        window.
    - finding_id: d115a8b2ac6964c4875d5c003427eb80f95db1eb
      file: docs/plans/session-14.md
      line: 31
      present: drifted
      evidence_snippet: '### 4. Ledger & Doc Sync (TD-022 Closure)'
      retain: yes-with-corrected-line
      reason: The heading explicitly promises Ledger & Doc Sync, but it may be incomplete
        regarding User Docs.
    - finding_id: a9c50f180daa06505adf003a7fde4b453b88d4de
      file: docs/plans/session-14.md
      line: 34
      present: 'yes'
      evidence_snippet: '- **writer.rs**: Delete the stale `# Stop-gap` docstring on `render_xmp`
        referencing TD-022. Rewrite `write_xmp`''s docstring to describe its AST-level
        non-destructive merge behavior and its dual-mode (create vs merge).'
      retain: 'yes'
      reason: It explicitly mentions modifying a docstring on render_xmp instead of removing
        it, and uses 'AST-level'.
    - finding_id: 8d3ba1f38efab58d4134847ca22faad0cc617d4a
      file: docs/plans/session-14.md
      line: 48
      present: 'yes'
      evidence_snippet: |-
        - No new stop-gaps. This session *closes* TD-022.

        ---

        ## Verification & Testing Strategy

        ### 1. Unit Tests in `crates/photohelper-sidecar/src/lib.rs`
      retain: yes-with-corrected-line
      reason: Line 48 is slightly before the Testing strategy starts, which indeed contains
        gaps in assertions.
```
