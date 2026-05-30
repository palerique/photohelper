# Session 07 — Plan Review, Round 2

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
| :--- | :--- | :--- | :--- |
| **Theme A** | XML Escaping, Entity Handling, and Parsing Safety | **HIGH** | `crates/photohelper-sidecar/src/reader.rs` |
| **Theme B** | Rating Representation, Mapping, and Metadata Preservation | **HIGH** | `crates/photohelper-sidecar/src/{settings,conflict}.rs` |
| **Theme C** | Query Optimizations & Joining Constraints | **HIGH** | `crates/photohelper-catalog/src/catalog.rs` |
| **Theme D** | Keyword Hygiene, Strip Scopes & Element Parsers | **MEDIUM** | `crates/photohelper-sidecar/src/{reader,conflict}.rs` |

---

## Theme A — XML Escaping, Entity Handling, and Parsing Safety

### [HIGH] Finding A.1 — Double-Escaping of Keywords due to Missing Unescaping on Input
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: Standard XML entities like `&amp;` or `&quot;` are not automatically unescaped inside text events by `quick-xml`. Storing raw escaped strings inside keywords results in them being double-escaped (e.g. `&amp;amp;`) on subsequent write cycles.
* **Remediation**: Explicitly invoke `quick-xml::escape::unescape` (or `unescape_value`) on extracted text when parsing `<rdf:li>` elements.

### [HIGH] Finding A.2 — State-Tracking Leak / Corruption via Empty Tags (`Event::Empty`)
* **Source**: Silent Failure Hunter
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: Matching `Event::Empty` along with `Event::Start` to enter nested containers means that empty container tags (like `<dc:subject />`) trigger state flags to be set permanently without ever hitting a matching `Event::End` to clear them, corrupting subsequent sibling parsing.
* **Remediation**: Explicitly ignore `Event::Empty` or clear state immediately when encountering it.

### [HIGH] Finding A.3 — Panic on Invalid/Malformed XML Entities during Unescaping
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: Unchecked `.unwrap()` or `.expect()` calls on `unescape` result will cause runtime panic on files containing invalid or corrupted entities, violating the reader's "lenient read" contract.
* **Remediation**: Handle unescaping failures gracefully by logging a warning and skipping the invalid segment or returning `Error::XmlParse`.

---

## Theme B — Rating Representation, Mapping, and Metadata Preservation

### [HIGH] Finding B.1 — Rating Enum Range Discrepancy (Merging Metadata Loss Risk)
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:17](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L17)
* **Problem**: Strong typing of `Rating` as `[1, 5]` conflicts with the required `[-1, 5]` range (where `-1` represents Rejected and `0` represents Unrated/None). This makes it impossible to distinguish between "unspecified (do not overwrite)" and "explicitly unrated (0 stars/rejected)" during deep merges.
* **Remediation**: Use `Option<i32>` directly or extend the `Rating` enum/state type to explicitly represent `Rejected = -1` and `Unrated = 0` to preserve the tri-state merge correctness.

### [MEDIUM] Finding B.2 — NaN NIMA Score Fall-Through to Perfect Stars
* **Source**: Silent Failure Hunter
* **Location**: [docs/plans/session-07.md:78](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L78)
* **Problem**: Float comparisons against `NaN` always return `false`. A naive boundary mapping with a final unconstrained `else` block will map `NaN` to **5 stars** and the `"excellent"` tier.
* **Remediation**: Validate early that the NIMA score is finite (`score.is_finite()`). If not, omit rating/keyword generation for that photo.

### [MEDIUM] Finding B.3 — Fragile Next-Event Text Parsing of Nested Elements
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: A naive implementation that reads the "next event" after `Event::Start` assuming it is always `Event::Text` will fail on empty tags, inline comments, or unexpected whitespaces.
* **Remediation**: Implement a robust, non-panicking text accumulator loop that runs until the matching `Event::End` is found.

---

## Theme C — Query Optimizations & Joining Constraints

### [HIGH] Finding C.1 — LEFT JOIN Duplication inside Catalog
* **Source**: Code Architect
* **Location**: [docs/plans/session-07.md:31](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L31)
* **Problem**: Performing a naive `LEFT JOIN` on `dup_clusters` can duplicate photo rows in the returned results if there are entries across multiple model runs.
* **Remediation**: Use a strict join condition that filters on the active CLIP model slug (`clip-vit-b32-laion2b-v1`) to guarantee uniqueness.

### [MEDIUM] Finding C.2 — Default Isolation Tag Preservation
* **Source**: PR Test Analyzer
* **Location**: [docs/plans/session-07.md:40](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L40)
* **Problem**: Running `photohelper develop` without compatibility flags must not poll or strip standard metadata fields (ratings, labels, keywords) that are already present in existing sidecars.
* **Remediation**: Add integration tests asserting that existing standard tags are perfectly preserved when compatibility flags are absent.

---

## Theme D — Keyword Hygiene, Strip Scopes & Element Parsers

### [MEDIUM] Finding D.1 — Over-Broad Keyword Stripping
* **Source**: Type Design Analyzer
* **Location**: [docs/plans/session-07.md:111](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L111)
* **Problem**: Doing string-prefix checks on `"photohelper"` to strip previous keywords might mistakenly remove user keywords like `"photohelper_fan"`.
* **Remediation**: Match only the exact string `"photohelper"` or structured patterns (`"photohelper:"`, `"photohelper|"`) when stripping previous keywords.

### [MEDIUM] Finding D.2 — State Flag Termination via Nested Tag Names
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: Using boolean flags like `in_dc_subject` to track nesting fails if a nested element with the same name (or a malformed end tag) is found.
* **Remediation**: Use saturating-depth counters (`depth: usize`) instead of simple booleans to track container scopes.

### [LOW] Finding D.3 — Empty Cluster Keyword Pollution
* **Source**: Silent Failure Hunter
* **Location**: [docs/plans/session-07.md:38](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L38)
* **Problem**: If `dedup_cluster_id` is `None`, writing `photohelper:cluster:` produces empty/meaningless keyword strings.
* **Remediation**: Skip emitting cluster-related keywords completely if `dedup_cluster_id` is `None`.

### [LOW] Finding D.4 — Conflicting Element/Attribute Formats
* **Source**: Code Reviewer
* **Location**: [docs/plans/session-07.md:25](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-07.md#L25)
* **Problem**: Undefined parser precedence if a sidecar file has conflicting ratings/labels represented in both element and attribute formats.
* **Remediation**: Set strict precedence rules (e.g. element format overrides attribute format) and document this behavior.

---

## Disposition summary

| Finding | Severity | Triage Disposition / Action | Remediated in Plan? |
| :--- | :--- | :--- | :--- |
| **A.1 (Double-Escaping)** | **HIGH** | Use `quick-xml::escape::unescape` when reading text | Yes (planned for v3) |
| **A.2 (Empty Tags)** | **HIGH** | Ignore `Event::Empty` in container start tracking | Yes (planned for v3) |
| **A.3 (Panic Prevention)** | **HIGH** | Log warning/skip or return `Error::XmlParse` on bad entities | Yes (planned for v3) |
| **B.1 (Rating Enum)** | **HIGH** | Use `Option<i32>` or tri-state to preserve `[-1, 5]` rating bounds | Yes (planned for v3) |
| **B.2 (NaN Fall-Through)** | **MEDIUM** | Validate score with `is_finite()` early | Yes (planned for v3) |
| **B.3 (Text Accumulator)** | **MEDIUM** | Implement EOF-safe element text collector helper | Yes (planned for v3) |
| **C.1 (LEFT JOIN Dups)** | **HIGH** | Filter join on active model slug | Yes (planned for v3) |
| **C.2 (Isolation Tests)** | **MEDIUM** | Add integration test for default isolation | Yes (planned for v3) |
| **D.1 (Broad Stripping)** | **MEDIUM** | Strip only exact or namespace-structured prefixes | Yes (planned for v3) |
| **D.2 (Depth Counters)** | **MEDIUM** | Use depth counters instead of simple booleans for container flags | Yes (planned for v3) |
| **D.3 (Empty Cluster)** | **LOW** | Omit cluster keyword when ID is `None` | Yes (planned for v3) |
| **D.4 (Precedence)** | **LOW** | nested element values override attributes | Yes (planned for v3) |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 12
  verified: 12
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: 3c8c3e80e90c885fa229e62fe83181829e061801
      file: docs/plans/session-07.md
      line: 25
      present: yes
      retain: yes
      reason: "Missing unescaping of keywords on parsing causes double-escaping on subsequent writes"
      evidence_snippet: "XMP Reader: Parsing support for nested `dc:subject`"
```
