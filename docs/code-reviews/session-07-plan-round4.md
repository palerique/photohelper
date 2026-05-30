# Session 07 — Plan Review, Round 4

```yaml
session_config:
  schema_version: 1
  model_claimed: "Gemini 1.5 Pro [2m]"
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

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 13
  verified: 13
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: "S07-R4-F01"
      file: "crates/photohelper-sidecar/src/writer.rs"
      line: 29
      present: yes
      retain: yes
      reason: "Thread-unique temp suffix is required to prevent Rayon file write collisions"
      evidence_snippet: "let tmp_path = path.with_extension(\"phdev.tmp\");"
    - finding_id: "S07-R4-F02"
      file: "crates/photohelper-sidecar/src/settings.rs"
      line: 282
      present: yes
      retain: yes
      reason: "Case-insensitive suffix decay of nima: and cluster: can destroy user keywords like cluster:favorites"
      evidence_snippet: "kw_lower.starts_with(\"nima:\")"
    - finding_id: "S07-R4-F03"
      file: "crates/photohelper-sidecar/src/reader.rs"
      line: 71
      present: yes
      retain: yes
      reason: "Unescaping failures in attributes must not map to empty string which clears the label"
      evidence_snippet: "unwrap_or_default()"
    - finding_id: "S07-R4-F04"
      file: "crates/photohelper-sidecar/src/writer.rs"
      line: 129
      present: yes
      retain: yes
      reason: "Rating::Unrated must be completely omitted from XML output to align with Lightroom Classic"
      evidence_snippet: "settings.rating()"
    - finding_id: "S07-R4-F05"
      file: "crates/photohelper-sidecar/src/settings.rs"
      line: 272
      present: yes
      retain: yes
      reason: "Empty label string must be preserved as Some(\"\") in merge to output xmp:Label=\"\""
      evidence_snippet: "Some(l) if l.is_empty() => None"
    - finding_id: "S07-R4-F06"
      file: "crates/photohelper-sidecar/src/conflict.rs"
      line: 100
      present: yes
      retain: yes
      reason: "First-run merging with pre-existing XMP is bypassed and returns ConflictPreserved"
      evidence_snippet: "(Some(_), None) => WriteOutcome::ConflictPreserved"
    - finding_id: "S07-R4-F07"
      file: "crates/photohelper-sidecar/src/reader.rs"
      line: 41
      present: yes
      retain: yes
      reason: "Prefix-tag stack matching can suffer from namespace collision or read misses"
      evidence_snippet: "dc:subject"
    - finding_id: "S07-R4-F08"
      file: "crates/photohelper-catalog/src/row.rs"
      line: 197
      present: yes
      retain: yes
      reason: "Negative dedup_cluster_id database sentinels like -1 trigger hard builder validation failures"
      evidence_snippet: "self.dedup_cluster_id"
    - finding_id: "S07-R4-F09"
      file: "crates/photohelper-sidecar/src/reader.rs"
      line: 157
      present: yes
      retain: yes
      reason: "Standard parse::<i32> fails on decimal integers (e.g., 3.0) written by other tools"
      evidence_snippet: "val.trim().parse::<i32>()"
    - finding_id: "S07-R4-F10"
      file: "crates/photohelper-cli/src/commands/develop.rs"
      line: 202
      present: yes
      retain: yes
      reason: "High-frequency unix_now_as_datetime system calls and lack of path deduplication under Rayon loop"
      evidence_snippet: "unix_now_as_datetime()"
    - finding_id: "S07-R4-F11"
      file: "crates/photohelper-cli/src/commands/develop.rs"
      line: 139
      present: yes
      retain: yes
      reason: "CLI warnings for active Lightroom flags are missing for --lr-label and --lr-keywords"
      evidence_snippet: "args.lr_rating && !has_any_cull_score"
    - finding_id: "S07-R4-F12"
      file: "crates/photohelper-sidecar/src/reader.rs"
      line: 45
      present: yes
      retain: yes
      reason: "Malformed XML with unbounded nesting depth can cause OOM on tag stack"
      evidence_snippet: "tag_stack.push"
    - finding_id: "S07-R4-F13"
      file: "crates/photohelper-sidecar/src/settings.rs"
      line: 343
      present: yes
      retain: yes
      reason: "Extreme out-of-range Temperature and Tint values are discarded rather than clamped"
      evidence_snippet: "if v >= 2000 && v <= 50000"
```

## Triage summary

| Theme | Description | Severity | Target File / Area |
| :--- | :--- | :--- | :--- |
| **Theme A** | Parallel Write Hazards and Concurrency | **CRITICAL** | `crates/photohelper-sidecar/src/writer.rs`, `crates/photohelper-cli/src/commands/develop.rs` |
| **Theme B** | Keyword Strip Preciseness & User Data Integrity | **CRITICAL** | `crates/photohelper-sidecar/src/settings.rs` |
| **Theme C** | Lightroom Clearing Compatibility & Attributes Omission | **HIGH** | `crates/photohelper-sidecar/src/{settings,writer}.rs` |
| **Theme D** | XML Parser Robustness, Decimal Parsing, & Memory Boundaries | **HIGH** | `crates/photohelper-sidecar/src/{reader,settings}.rs` |
| **Theme E** | Database Sentinels, First-Run Merging, & CLI Refinements | **HIGH** | `crates/photohelper-sidecar/src/conflict.rs`, `crates/photohelper-catalog/src/row.rs`, `crates/photohelper-cli/src/commands/develop.rs` |

---

## Theme A — Parallel Write Hazards and Concurrency

### [CRITICAL] Finding A.1 — Suffix-less Temporary Sidecar Writing Race Hazard (S07-R4-F01)
* **Location**: `crates/photohelper-sidecar/src/writer.rs:29` and `crates/photohelper-cli/src/commands/develop.rs:199`
* **Problem**: The developer command uses Rayon (`rows.par_iter().for_each(...)`) to process multiple sidecar writes in parallel. Currently, `writer.rs` writes temporary XML content to `<path>.phdev.tmp` before performing an atomic rename. Because this temporary file path is static, if any photos share the same directory/path (e.g. duplicate files, virtual copies, or overlapping targets), separate threads will write to the exact same temporary file concurrently, resulting in write collisions, truncated contents, permission errors, or file corruption.
* **Remediation**: Append a thread-unique identifier to the temporary file extension:
  ```rust
  let thread_id = format!("{:?}", std::thread::current().id())
      .replace("ThreadId(", "")
      .replace(")", "");
  let tmp_ext = format!("phdev.{thread_id}.tmp");
  let tmp_path = path.with_extension(tmp_ext);
  ```

### [MEDIUM] Finding A.2 — High-Frequency SystemTime Syscalls and Lack of Path Deduplication (S07-R4-F10)
* **Location**: `crates/photohelper-cli/src/commands/develop.rs:202`
* **Problem**: `unix_now_as_datetime()` is called on every iteration inside the parallel loop. For large catalogs of 10,000+ photos, this issues thousands of concurrent `SystemTime::now()` system calls, introducing substantial CPU context-switching overhead and cache invalidation. Additionally, the CLI loops directly over `rows.par_iter()` without pre-iteration path deduplication, which means virtual copies or duplicate paths are processed concurrently instead of being grouped or processed deterministically.
* **Remediation**:
  1. Call `unix_now_as_datetime()` exactly once before the `par_iter()` loop starts, and pass the resolved timestamp to the parallel iterations.
  2. Group or deduplicate targeted sidecar paths prior to running the parallel iteration, or document a deterministic resolution strategy.

---

## Theme B — Keyword Strip Preciseness & User Data Integrity

### [CRITICAL] Finding B.1 — Stale Keyword Decay Over-Stripping (S07-R4-F02)
* **Location**: `crates/photohelper-sidecar/src/settings.rs:282` and `docs/plans/session-07.md:146`
* **Problem**: To prevent keyword accumulation across subsequent runs, the plan specifies case-insensitively stripping previous flat and hierarchical keywords starting with `"cluster:"` or `"nima:"`. However, doing so will silently strip a user's own custom keywords like `"cluster:favorites"`, `"cluster:london"`, or `"nima:amazing"`, leading to silent data loss.
* **Remediation**: Be highly precise when stripping flat keywords. Strip:
  - The exact keyword `"photohelper"`.
  - Any keyword starting with `"photohelper:"` or `"photohelper|"`.
  - Any keyword matching `"cluster:<id>"` where `<id>` parses as a valid integer.
  - Any keyword matching `"nima:<tier>"` where `<tier>` is one of our valid aesthetic adjectives (`discard`, `poor`, `fair`, `good`, `excellent`).
  This ensures user-defined keywords starting with `"cluster:"` or `"nima:"` remain untouched.

---

## Theme C — Lightroom Clearing Compatibility & Attributes Omission

### [HIGH] Finding C.1 — Rating::Unrated Serialized Verbatim as `"0"`, Violating Omission Contract (S07-R4-F04)
* **Location**: `crates/photohelper-sidecar/src/writer.rs:129-131`
* **Problem**: The plan mandates that `Rating::Unrated` (`0`) must be written by omitting the `xmp:Rating` attribute entirely on write, to prevent non-standard clutter in Lightroom Classic. However, the writer writes `xmp:Rating="0"` if the rating is `Some(Rating::Unrated)`.
* **Remediation**: Update `writer.rs` to guard against writing `Rating::Unrated`:
  ```rust
  if let Some(r) = settings.rating() {
      if r != Rating::Unrated {
          let _ = write!(attrs, "\n      xmp:Rating=\"{}\"", r.as_i32());
      }
  }
  ```

### [HIGH] Finding C.2 — Color Label Clearance Broken due to Merge Flattening (S07-R4-F05)
* **Location**: `crates/photohelper-sidecar/src/settings.rs:272-276` and `crates/photohelper-sidecar/src/settings.rs:398-411`
* **Problem**: The plan specifies that `Some("")` represents "explicitly clear label", which the XML writer must translate to `xmp:Label=""` to clear catalog color labels in Lightroom Classic. However, inside `SidecarSettings::merge` and `from_parsed`, empty strings are flattened to `None`. As a result, the writer sees `None` and completely omits the attribute rather than writing `xmp:Label=""`. Lightroom Classic will not clear its catalog value on attribute omission.
* **Remediation**: Retain `Some(String::new())` inside `SidecarSettings::merge` and `from_parsed` when the label is empty, and update `writer.rs` to output `xmp:Label=""` for empty string labels rather than omitting the attribute. Omitting should only occur when the field is `None`.

---

## Theme D — XML Parser Robustness, Decimal Parsing, & Memory Boundaries

### [HIGH] Finding D.1 — Unescaping Failure Silently Clearing Color Labels (S07-R4-F03)
* **Location**: `crates/photohelper-sidecar/src/reader.rs:71`
* **Problem**: Any failure in unescaping attribute values inside `reader.rs` is caught by `.unwrap_or_default()`, yielding `""` (empty string). However, because `""` is interpreted as "explicitly clear label during deep merge", any unescaping error on standard attributes like `xmp:Label` will map to an empty string and cause `photohelper` to silently **clear** the label on merge, rather than keeping it, mapping to `None`, or raising an error.
* **Remediation**: Check the unescaping result. If it returns an `Err`, log a warning or raise a parser error, and treat the field value as `None` (unspecified/inherit) rather than mapping it to an empty string.

### [HIGH] Finding D.2 — Prefix-tag Stack Prefix Sensitivity / Namespace Collision (S07-R4-F07)
* **Location**: `crates/photohelper-sidecar/src/reader.rs:41` and `docs/plans/session-07.md:41-45`
* **Problem**: Tracking paths literally (e.g. `["dc:subject", "rdf:Bag", "rdf:li"]`) will fail if another editor writes the same metadata using a different prefix (e.g. `<dublin:subject>` or `<dcore:subject>`). Conversely, tracking only local names (e.g. `["subject", "Bag", "li"]`) risks matching elements with the same name inside another custom namespace.
* **Remediation**: Standardize the path stack to resolve element names to qualified names using proper namespace URIs (e.g., using `reader.resolve_element_name(name)`), or match elements by both their namespace URI and local name. Alternatively, if a simple parser is kept, match the stack against resolved local names only and verify the enclosing namespace blocks.

### [MEDIUM] Finding D.3 — Decimal Integer Parsing Compatibility Failure (S07-R4-F09)
* **Location**: `crates/photohelper-sidecar/src/reader.rs:157-173`
* **Problem**: Industry tools like Aftershoot and Lightroom often write ratings or other integer fields with decimals (e.g. `xmp:Rating="3.0"` or `crs:Temperature="5500.0"`). Currently, `parse_i32` and `parse_i64` use direct string integer parsing (`val.trim().parse::<i32>()`) which fails on any float string, dropping the values entirely.
* **Remediation**: Update integer parsing helpers to parse strings as `f64` first, check if finite and within bounds, round them, and then cast to integer.

### [LOW] Finding D.4 — Unbounded Parser Stack Depth Memory Vulnerability (S07-R4-F12)
* **Location**: `crates/photohelper-sidecar/src/reader.rs:45` and `docs/plans/session-07.md:45`
* **Problem**: If the flat parser processes a malformed or malicious XML document with millions of nested opening tags without closing them, the `Vec<String>` path stack will grow unboundedly, leading to excessive memory usage and an OOM panic.
* **Remediation**: Enforce a hard maximum nesting depth on the path stack (e.g., 64) and return a parsing error if exceeded.

### [LOW] Finding D.5 — Discarding Out-of-Range Camera Raw Settings instead of Clamping (S07-R4-F13)
* **Location**: `crates/photohelper-sidecar/src/settings.rs:343-358`
* **Problem**: In `from_parsed`, if `crs:Temperature` is outside `[2000, 50000]` or `crs:Tint` is outside `[-150, 150]`, the parser discards the values entirely and returns `None`. During append-only merge, discarding extreme settings written by Lightroom Classic causes us to lose metadata or incorrectly fall back to other parameters.
* **Remediation**: Clamp extreme but valid parsed values to our valid ranges rather than discarding them.

---

## Theme E — Database Sentinels, First-Run Merging, & CLI Refinements

### [CRITICAL] Finding E.1 — First-Run Merging Bypassed in `conflict.rs` (S07-R4-F06)
* **Location**: `crates/photohelper-sidecar/src/conflict.rs:100` and `docs/plans/session-07.md:55`
* **Problem**: The plan specifies that on the first run against files with pre-existing XMP sidecars (represented by `(Some(_), None)`), photohelper must perform a non-destructive merge and write back, returning `WriteOutcome::Overwritten` / `WriteOutcome::Merged`. However, the implementation simply returns `WriteOutcome::ConflictPreserved` directly and skips writing entirely, rendering develop metadata application inert on first runs.
* **Remediation**: Update the `(Some(_), None)` match arm in `conflict.rs`:
  ```rust
  (Some(_), None) => {
      tracing::info!(path = %path.display(), "develop: existing XMP has metadata date (first photohelper run); merging and writing");
      let merged = existing.merge(incoming);
      write_xmp(path, &merged)?;
      WriteOutcome::Overwritten
  }
  ```

### [HIGH] Finding E.2 — Negative `dedup_cluster_id` Database Sentinels Triggering Builder Validation Failures (S07-R4-F08)
* **Location**: `crates/photohelper-catalog/src/row.rs:197` and `crates/photohelper-cli/src/commands/develop.rs:241`
* **Problem**: Database sentinels like `-1` are commonly used to represent unclustered photos. Because `SidecarSettingsBuilder::build` enforces `c >= 0`, any negative database values result in a validation error, skipping sidecar generation entirely.
* **Remediation**: Map negative `dedup_cluster_id` values gracefully to `None` inside `DevelopRow`'s getter or during row mapping:
  ```rust
  pub fn dedup_cluster_id(&self) -> Option<i64> {
      self.dedup_cluster_id.and_then(|id| if id >= 0 { Some(id) } else { None })
  }
  ```

### [MEDIUM] Finding E.3 — Incomplete Warnings for Active Lightroom Compatibility Flags (S07-R4-F11)
* **Location**: `crates/photohelper-cli/src/commands/develop.rs:139`
* **Problem**: The CLI only checks and warns if `--lr-rating` is active and no scores exist, failing to warn the user if `--lr-label` or `--lr-keywords` are requested but no catalog scores or clusters are available.
* **Remediation**: Generalize the warning check to cover all requested Lightroom flags against active data:
  ```rust
  let has_any_cull_score = rows.iter().any(|r| r.nima_score().is_some());
  let has_any_cluster = rows.iter().any(|r| r.dedup_cluster_id().is_some());
  if args.lr_rating && !has_any_cull_score { ... }
  if args.lr_label && !has_any_cull_score { ... }
  if args.lr_keywords && !has_any_cull_score && !has_any_cluster { ... }
  ```
