# Session 11 — Develop strict mode & resilience, Review Round 3

```yaml
session_config:
  schema_version: 1
  model_claimed: gemini-2.5-pro
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

<table>
  <tr>
    <th>Theme</th>
    <th>Description</th>
    <th>Severity</th>
    <th>Status</th>
  </tr>
  <tr>
    <td>A</td>
    <td>Concurrent write race via Unicode deduplication bypass</td>
    <td>HIGH</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>B</td>
    <td>Heap allocation bottleneck in XML sanitizer</td>
    <td>MEDIUM</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>C</td>
    <td>Partial cluster ID filter (Leakage)</td>
    <td>HIGH</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>D</td>
    <td>Missing XML domain validation at builder boundary</td>
    <td>HIGH</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>E</td>
    <td>Silent coercion via `clamp()` on invariants</td>
    <td>MEDIUM</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>F</td>
    <td>I/O and execution fall-open vulnerabilities</td>
    <td>HIGH</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>G</td>
    <td>Code duplication and clarity</td>
    <td>LOW</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>H</td>
    <td>Untested Force Error Recovery</td>
    <td>HIGH</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>I</td>
    <td>Policy Violations (CLAUDE.md)</td>
    <td>CRITICAL</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>J</td>
    <td>Comment rot on timestamps</td>
    <td>LOW</td>
    <td>Remediate</td>
  </tr>
  <tr>
    <td>K</td>
    <td>Forward compatibility hazard in outcome matcher</td>
    <td>LOW</td>
    <td>Remediate</td>
  </tr>
</table>

## Theme A — Concurrent write race via Unicode deduplication bypass

- [Code Architect]: Concurrent Write Race via Unicode Deduplication Bypass 'HIGH'
- [General Consistency Analyst]: Partial Implementation of Theme E (Windows `to_string_lossy` retention) 'LOW'

**Remediation**: Remove the `#[cfg(unix)]` split in `crates/photohelper-cli/src/commands/develop.rs:229`. Use `sidecar_path.to_string_lossy().to_lowercase().into_bytes()` unconditionally to ensure correct Unicode case-folding across all operating systems.

## Theme B — Heap allocation bottleneck in XML sanitizer

- [Code Architect]: Extreme Heap Allocation Bottleneck in XML Sanitization 'MEDIUM'
- [General Consistency Analyst]: Severe O(N) Allocation Regression in Theme F Remediation 'MEDIUM'

**Remediation**: Refactor `crates/photohelper-sidecar/src/writer.rs:228`. Extract the core XML validation logic into `is_valid_xml_char(c: char) -> bool` to avoid iterating over `&c.to_string()` and invoking allocations.

## Theme C — Partial cluster ID filter (Leakage)

- [Type Design Analyzer]: Domain-Filter Asymmetry & Permanent Catalog Pollution 'HIGH'
- [General Consistency Analyst]: Partial Implementation of Theme B (`dedup_cluster_id` keyword leakage) 'HIGH'
- [Code Architect]: Inconsistent Syncing of Suppressed Cluster IDs 'LOW'

**Remediation**: Mirror the bound filter in `crates/photohelper-cli/src/commands/develop.rs:424` by applying `.filter(|&id| id >= 0)` to `row.dedup_cluster_id()`.

## Theme D — Missing XML domain validation at builder boundary

- [Type Design Analyzer]: Encapsulation Leak & Deferred XML Sanitization 'HIGH'

**Remediation**: In `crates/photohelper-sidecar/src/settings.rs:658`, invoke `crate::writer::is_valid_xml_string()` for `label`, `keywords`, `hierarchical_keywords`, and `photohelper_id`. Return `Err(Error::Validation)` if illegal characters are found, rather than silently mutating it later.

## Theme E — Silent coercion via `clamp()` on invariants

- [Type Design Analyzer]: Inconsistent Invariant Enforcement (Silent Clamping) 'MEDIUM'

**Remediation**: In `crates/photohelper-sidecar/src/settings.rs:649`, remove `nima_score` clamping coercion and explicitly reject out-of-bounds scores.

## Theme F — I/O and execution fall-open vulnerabilities

- [Silent Failure Hunter]: `Path::exists()` Fail-Open Swallows Permission Errors 'HIGH'
- [Silent Failure Hunter]: Fail-Open on `mtime` Metadata Retrieval Defeats Conflict Protection 'HIGH'
- [Silent Failure Hunter]: `std::fs::metadata` Error Swallowed During Atomic Write Permission Sync 'MEDIUM'

**Remediation**:
1. `crates/photohelper-cli/src/commands/develop.rs:336`: Replace `!source_path.exists()` with `std::fs::try_exists()`.
2. `crates/photohelper-sidecar/src/conflict.rs:122`: Return `Err(Error::Io { ... })` instead of `false` on `mtime` read failure.
3. `crates/photohelper-sidecar/src/writer.rs:68`: Explicitly ignore `ErrorKind::NotFound`, but warn/escalate on other IO errors for `std::fs::metadata`.

## Theme G — Code duplication and clarity

- [Code Simplifier]: Redundant / Duplicated Code Block 'HIGH'

**Remediation**: In `crates/photohelper-cli/src/commands/develop.rs:287`, remove the redundant `SidecarSettings::builder()` logic and reuse `base_builder`.

## Theme H — Untested Force Error Recovery

- [PR Test Analyzer]: Untested Error-Recovery Hatch for `--force` 'HIGH'

**Remediation**: Add a test in `crates/photohelper-sidecar/src/lib.rs` for `Error::XmlParse` branch in `conflict.rs:76` covering `ConflictStrategy::ForceOverwrite`.

## Theme I — Policy Violations (CLAUDE.md)

- [Code Reviewer]: Unjustified Linter Exception (Policy Violation) 'CRITICAL'
- [Code Reviewer]: Unchecked Failure in Production Path (Policy Violation) 'HIGH'

**Remediation**:
1. `crates/photohelper-cli/src/commands/develop.rs:27`: Add `TD` trigger to `#[allow(clippy::struct_excessive_bools)]`.
2. `crates/photohelper-sidecar/src/writer.rs:111`: Remove `.expect()` statements on string formatting.

## Theme J — Comment rot on timestamps

- [Comment Analyzer]: Factual Accuracy: I/O delay invalidating timestamp claims 'HIGH'

**Remediation**: In `crates/photohelper-cli/src/commands/develop.rs:383`, correct or relocate the `now_utc` inline comment.

## Theme K — Forward compatibility hazard in outcome matcher

- [Code Architect]: Forward-Compatibility Hazard in Outcome Matcher 'LOW'

**Remediation**: In `crates/photohelper-cli/src/commands/develop.rs:124`, avoid the `_ => self.errored...` catch-all for unknown `WriteOutcome` variants.

## Disposition summary

<table>
  <tr>
    <th>Theme</th>
    <th>Disposition</th>
    <th>Notes</th>
  </tr>
  <tr>
    <td>A-K</td>
    <td>Accept</td>
    <td>Require fixes to meet full zero-tolerance standard before session handoff.</td>
  </tr>
</table>

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 15
  verified: 10
  drifted: 4
  hallucinated: 1
  unreadable: 0
  compromised: 0
  discard_rate: 0.06
  details:
    - finding_id: b8b29d33174ea66394a7828561ff1e9d9bbb1ee5
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 424
      present: 'yes'
      retain: 'yes'
      reason: Missing filter is exactly at the specified line
      evidence_snippet: |
        if let Some(cluster_id) = row.dedup_cluster_id() {
            flat.insert(format!("cluster:{cluster_id}"));
            hierarchical.insert(format!("photohelper|cluster:{cluster_id}"));
        }
    - finding_id: 9142b4047c0fe33a32d9c229609cdffead288ca6
      file: crates/photohelper-sidecar/src/writer.rs
      line: 228
      present: 'yes'
      retain: 'yes'
      reason: Pattern is present exactly at line 228
      evidence_snippet: |
        fn sanitize_xml_string(s: &str) -> String {
            s.chars().filter(|c| is_valid_xml_string(&c.to_string())).collect()
        }
    - finding_id: eb7d517e4298a0a8dbe52c1e94eeead9d9b55c42
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 230
      present: drifted
      retain: yes-with-corrected-line
      reason: Pattern is present but drifted to line 229
      evidence_snippet: |
        let dedup_key: Vec<u8> = {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                sidecar_path.as_os_str().as_bytes().to_ascii_lowercase()
            }
    - finding_id: 01f6290cbe628554871ab258cd9d441d99c2ab5d
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 336
      present: 'yes'
      retain: 'yes'
      reason: Pattern is present exactly at line 336
      evidence_snippet: |
        // Step a: existence pre-check.
        if !source_path.exists() {
            tracing::warn!(path = %source_path.display(), "file missing since ingest; skipping");
    - finding_id: bbdab448fe3727c31b34ddf1bae24fe7621e900c
      file: crates/photohelper-sidecar/src/writer.rs
      line: 111
      present: 'yes'
      retain: 'yes'
      reason: Pattern is present exactly at line 111
      evidence_snippet: |
        write!(attrs, "\n      xmp:MetadataDate=\"{iso}\"").expect("fmt::Write on String cannot fail");

        if settings.last_processed_at().is_some() {
            write!(attrs, "\n      ph:LastProcessedAt=\"{iso}\"").expect("fmt::Write on String cannot fail");
        }
    - finding_id: 837856f1793125d5a241790031f6929050fd4918
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 240
      present: 'no'
      retain: 'no'
      reason: The duplicate drop is not silent; it logs a warning via tracing::warn!
      evidence_snippet: |
        if seen_paths.insert(dedup_key) {
            unique_rows.push(row);
        } else {
            tracing::warn!(
                path = %sidecar_path.display(),
                photo = %row.source_path().display(),
                "skipping duplicate photo row targeting the same sidecar path to prevent concurrent write race hazards"
            );
        }
    - finding_id: 9eb6a9672610c04732660e5d699568f55fae40fb
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 27
      present: 'yes'
      retain: 'yes'
      reason: Pattern is present exactly at line 27
      evidence_snippet: |
        /// Clap args for `photohelper develop`.
        #[derive(clap::Args, Debug, Clone)]
        #[allow(clippy::struct_excessive_bools)]
        pub(crate) struct DevelopArgs {
    - finding_id: 58b151b6d5e32be6b8c7dbcd138f1cfb9ee00130
      file: crates/photohelper-sidecar/src/settings.rs
      line: 658
      present: 'yes'
      retain: 'yes'
      reason: The method is present and lacks XML validation for the label
      evidence_snippet: |
        // Normalize color label: trim and convert to String::new() if whitespace/empty
        let label = self.label.map(|v| {
            let trimmed = v.trim().to_string();
            if trimmed.is_empty() {
                String::new()
            } else {
                trimmed
            }
        });
    - finding_id: 0f1e36eb1ba1e6962891eac7566ff825f43d5d3d
      file: crates/photohelper-sidecar/src/conflict.rs
      line: 76
      present: 'yes'
      retain: 'yes'
      reason: The branch for Error::XmlParse exists exactly at line 76
      evidence_snippet: |
        if strategy == ConflictStrategy::ForceOverwrite {
            if matches!(e, Error::XmlParse { .. }) {
                tracing::warn!(path = %path.display(), error = %e, "force: failed to parse existing XMP; falling back to direct write");
                write_xmp(path, incoming)?;
                tracing::info!(path = %path.display(), "develop: XMP sidecar force-overwritten");
                return Ok(WriteOutcome::ForcedOverwrite);
            }
        }
    - finding_id: e47c08fc7c2e2172d88e1c3c4c111e50b11c2881
      file: crates/photohelper-sidecar/src/conflict.rs
      line: 122
      present: 'yes'
      retain: 'yes'
      reason: Pattern is present exactly at line 122
      evidence_snippet: |
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to retrieve sidecar file mtime; falling back to metadata timestamp comparison"
            );
            false
        }
    - finding_id: dc7401cdcee972ba22cd6e3e47f5dfac1e073d57
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 381
      present: drifted
      retain: yes-with-corrected-line
      reason: Pattern is present but drifted to line 383
      evidence_snippet: |
        // Retrieve the UTC timestamp per-photo immediately before writing
        // to completely eliminate write-buffer delay and scheduling drift.
        let now_utc = time::OffsetDateTime::now_utc();
        builder = builder.last_processed_at(now_utc);
    - finding_id: 40a860af11b3a8f9ff26cb4309c66abb6f953bf9
      file: crates/photohelper-sidecar/src/settings.rs
      line: 648
      present: drifted
      retain: yes-with-corrected-line
      reason: Pattern is present but drifted to line 649
      evidence_snippet: |
        if let Some(s) = self.nima_score {
            if !s.is_finite() {
                return Err(Error::Validation {
                    message: format!("nima_score {s} is not finite"),
                });
            }
            self.nima_score = Some(s.clamp(1.0, 10.0));
        }
    - finding_id: 89ab710a83847063bc7cc7b96239e7ab665a4386
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 287
      present: 'yes'
      retain: 'yes'
      reason: Pattern is present exactly at line 287
      evidence_snippet: |
        let mut base_builder = SidecarSettings::builder();
        if let Some(v) = args.exposure {
            base_builder = base_builder.exposure(v);
        }
        if let Some(v) = args.temp {
            base_builder = base_builder.temperature(v);
        }
    - finding_id: 2725c63fe0060da9a90ce2209829bde9a2ea58f5
      file: crates/photohelper-sidecar/src/writer.rs
      line: 67
      present: drifted
      retain: yes-with-corrected-line
      reason: Pattern is present but drifted to line 68
      evidence_snippet: |
        // Copy permissions from original file if it exists, to avoid inheriting umask defaults
        if let Ok(metadata) = std::fs::metadata(path.as_path()) {
            if let Err(e) = std::fs::set_permissions(&tmp_path, metadata.permissions()) {
                tracing::warn!(path = %tmp_path.display(), error = %e, "failed to copy permissions to temp file");
            }
        }
    - finding_id: 9c5abc013979d19c8b0ceef0a71477feb4a26b27
      file: crates/photohelper-cli/src/commands/develop.rs
      line: 124
      present: 'yes'
      retain: 'yes'
      reason: Pattern is present exactly at line 124
      evidence_snippet: |
        Ok(WriteOutcome::ForcedOverwrite) => { self.force_overwritten.fetch_add(1, Ordering::Relaxed); }
        Err(()) => { self.errored.fetch_add(1, Ordering::Relaxed); }
        _ => { self.errored.fetch_add(1, Ordering::Relaxed); }
```
