# Session 07 — Plan Review, Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "Opus 4.7 [1m]"
  model_observed: "unverifiable"
  effort_claimed: "MAX"
  effort_observed: "unverifiable"
  ask_user_question_id: null
  user_response: "option-1"
  gate_state: "pass"
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

| Theme | Description | Severity | Target File / Area |
|---|---|---|---|
| **Theme A** | XML Escaping, Safe Parsing & Nesting Invariants | **HIGH** | `crates/photohelper-sidecar/src/{reader,writer}.rs` |
| **Theme B** | Sidecar Conflict & Metadata Loss Prevention | **CRITICAL** | `crates/photohelper-sidecar/src/conflict.rs` |
| **Theme C** | Keyword Set Uniqueness, VCS Churn & Stale Tag Decay | **HIGH** | `crates/photohelper-sidecar/src/{settings,conflict}.rs` |
| **Theme D** | Type Safety & Data Normalization | **MEDIUM** | `crates/photohelper-sidecar/src/settings.rs` |
| **Theme E** | Namespace Declaration & Label Invariants | **LOW** | `crates/photohelper-sidecar/src/writer.rs` |

---

## Theme A — XML Escaping, Safe Parsing & Nesting Invariants

### [HIGH] Finding A.1 — Unescaped XML writing of user-provided strings
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: Storing flat and hierarchical keywords or color labels that contain XML special characters (like `"Black & White"`, `<cull>`, or nested quotes) raw into sidecar templates creates invalid XML, crashing both `photohelper`'s parsing and Adobe Lightroom Classic.
* **Remediation**: Use `quick-xml::escape::escape` (or equivalent) to escape all user-provided labels and keywords during formatting.

### [HIGH] Finding A.2 — Fragile nested parse sub-loops in streaming reader
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:24](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L24)
* **Problem**: Parsing nested structures (`<dc:subject>` -> `<rdf:Bag>` -> `<rdf:li>`) in `quick-xml` using naive loops can cause infinite-loops on truncated XML or panics on unexpected comments/whitespace/empty tags.
* **Remediation**: Design an explicit, EOF-safe, non-panicking sub-loop helper to extract `<rdf:li>` text safely.

### [MEDIUM] Finding A.3 — Naive parser matches unrelated nested `<rdf:li>` tags
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:24](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L24)
* **Problem**: Unrelated elements like `crs:GradientBasedCorrections` also contain `<rdf:li>` items. A naive element parser collecting all `<rdf:li>` elements will pollute keywords with unrelated correction data.
* **Remediation**: Maintain explicit parser state flags (`in_dc_subject` and `in_lr_hierarchical`) to parse `<rdf:li>` elements only when inside standard keyword blocks.

---

## Theme B — Sidecar Conflict & Metadata Loss Prevention

### [CRITICAL] Finding B.1 — Partial overwrites silently wipe out user ratings & color labels
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:26](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L26)
* **Problem**: If `photohelper develop` is run without `--lr-rating` or `--lr-label` flags, the `incoming` settings built will have these fields set to `None`. During an overwrite write, these fields will be omitted, deleting any existing user-defined ratings or labels from the sidecar file.
* **Remediation**: In `conflict.rs`, perform a merge step of ratings and labels from `existing` to `incoming` when those fields are `None` in `incoming`.

### [HIGH] Finding B.2 — Force overwrite bypassing keyword and metadata merging
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:100](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L100)
* **Problem**: If `--force` is active, `merge_and_write` currently completely bypasses reading the existing sidecar. Doing so wipes out all existing user keywords and other metadata. `--force` should override the timestamp lock gate, not destroy user-defined metadata.
* **Remediation**: Load the existing sidecar and merge non-conflict metadata (like user keywords, ratings, labels) even when force-overwriting.

---

## Theme C — Keyword Set Uniqueness, VCS Churn & Stale Tag Decay

### [HIGH] Finding C.1 — Use of `Vec<String>` violates uniqueness and determinism
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:19](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L19)
* **Problem**: Storing flat and hierarchical keywords in `Vec<String>` allows duplicates, comparison inequality under different orderings (causing unnecessary file writes), and VCS churn.
* **Remediation**: Store keywords in `BTreeSet<String>` to natively enforce uniqueness, alphabetical sorting, and deterministic equality.

### [HIGH] Finding C.2 — Stale keyword accumulation over multiple develop passes
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:101](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L101)
* **Problem**: Doing a blind union: `existing_keywords ∪ generated_keywords` causes outdated `photohelper` keywords (like older duplicate cluster IDs or NIMA tiers) to persist and accumulate forever if a photo is re-processed.
* **Remediation**: Before taking the union, strip out any pre-existing keywords starting with `photohelper` (both flat and hierarchical).

### [MEDIUM] Finding C.3 — Malformed hierarchical keywords validation
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:20](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L20)
* **Problem**: Hierarchical keywords with trailing/leading pipes, empty segments, or surrounding spaces break indexing inside Lightroom Classic.
* **Remediation**: Trim and normalize hierarchical keyword segments when building or merging settings.

---

## Theme D — Type Safety & Data Normalization

### [MEDIUM] Finding D.1 — Star ratings as `Option<i32>` allows invalid ranges
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:17](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L17)
* **Problem**: Representing rating as `Option<i32>` allows invalid ratings (like `Some(6)` or `Some(-1)`) to exist in-memory, violating type-safety rules.
* **Remediation**: Introduce a strongly-typed `Rating` enum inside `settings.rs` with variants `One = 1` through `Five = 5` and a `TryFrom<i32>` implementation.

### [LOW] Finding D.2 — Empty/whitespace color labels represent inconsistent states
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:18](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L18)
* **Problem**: Allowing `Some("")` or `Some("   ")` alongside `None` creates inconsistent representation of unlabeled states.
* **Remediation**: Trim and convert empty/whitespace labels directly to `None` in the builder and parser.

---

## Theme E — Namespace Declaration & Label Invariants

### [LOW] Finding E.1 — Declaring namespaces unconditionally
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: If nested structures are written, `xmlns:dc` and `xmlns:lr` must be defined on the main `<rdf:Description>` element to prevent XML engines from rejecting the sidecar.
* **Remediation**: Unconditionally write `xmlns:dc="http://purl.org/dc/elements/1.1/"` and `xmlns:lr="http://ns.adobe.com/lightroom/1.0/"` declarations on `<rdf:Description>`.

---

## Disposition summary

| Finding | Severity | Triage Disposition / Action | Remediated in Plan? |
|---|---|---|---|
| **A.1 (XML Escaping)** | **HIGH** | Implement `quick-xml::escape::escape` on writing | Yes (planned for v2) |
| **A.2 (Safe Parsing)** | **HIGH** | Implement EOF-safe non-panicking reader loop | Yes (planned for v2) |
| **A.3 (Nesting Filter)**| **MEDIUM**| Track `in_dc_subject`/`in_lr_hierarchical` state | Yes (planned for v2) |
| **B.1 (Rating Merging)**| **CRITICAL**| Merge ratings/labels from existing if None in incoming | Yes (planned for v2) |
| **B.2 (Force Merging)** | **HIGH** | Load and merge existing metadata even under `--force` | Yes (planned for v2) |
| **C.1 (BTreeSet)** | **HIGH** | Use `BTreeSet<String>` for keywords in type definitions | Yes (planned for v2) |
| **C.2 (Stale Decay)** | **HIGH** | Retain only non-photohelper keywords during merging | Yes (planned for v2) |
| **C.3 (Hierarchy Trim)**| **MEDIUM**| Normalize hierarchical keywords on ingestion | Yes (planned for v2) |
| **D.1 (Rating Enum)** | **MEDIUM**| Define typed `Rating` enum with range validation | Yes (planned for v2) |
| **D.2 (Label Norm)** | **LOW** | Trim whitespace and convert empty color labels to `None` | Yes (planned for v2) |
| **E.1 (Namespace Dec)**| **LOW** | Unconditionally declare `xmlns:dc` and `xmlns:lr` | Yes (planned for v2) |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 11
  verified: 11
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: 1a8c3e80e90c885fa229e62fe83181829e061801
      file: docs/plans/session-07.md
      line: 25
      present: yes
      retain: yes
      reason: "XMP writer templating requires XML escaping to prevent structural breakage"
      evidence_snippet: "XMP Writer: Native rendering of nested `dc:subject`"
    - finding_id: 2b8c3e80e90c885fa229e62fe83181829e061802
      file: docs/plans/session-07.md
      line: 24
      present: yes
      retain: yes
      reason: "Robust parsing is needed for hierarchical and flat keyword elements"
      evidence_snippet: "XMP Reader: Parsing support for nested `dc:subject`"
```
