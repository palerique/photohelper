# Session 10 — Pipeline Orchestration via the `run` Subcommand, Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "Gemini 1.5 Pro"
  model_observed: unverifiable
  effort_claimed: "MAX"
  effort_observed: unverifiable
  ask_user_question_id: null
  user_response: not-asked
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
  <thead>
    <tr>
      <th>Theme</th>
      <th>Severity</th>
      <th>Description</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Theme A</td>
      <td>CRITICAL</td>
      <td>Clap `#[command(flatten)]` Duplication Panic on `--strict`/`--force`</td>
    </tr>
    <tr>
      <td>Theme B</td>
      <td>CRITICAL</td>
      <td>Performance Collapse via Unbatched ML Inference and SQLite Thrashing</td>
    </tr>
    <tr>
      <td>Theme C</td>
      <td>HIGH</td>
      <td>Path Nesting Bypass / Incomplete Output Validation Loophole</td>
    </tr>
    <tr>
      <td>Theme D</td>
      <td>HIGH</td>
      <td>Test Coverage Gaps (Non-deterministic, Missing Negatives, Missing Output Setup, No Bounds Checks)</td>
    </tr>
    <tr>
      <td>Theme E</td>
      <td>HIGH</td>
      <td>Error Swallowing in Non-Strict Mode and Fail-Open `INSERT OR IGNORE`</td>
    </tr>
    <tr>
      <td>Theme F</td>
      <td>MEDIUM</td>
      <td>Omission of `dedup` degrades `--lr-keywords`</td>
    </tr>
    <tr>
      <td>Theme G</td>
      <td>LOW</td>
      <td>Restating existing `value_parser` bounds</td>
    </tr>
    <tr>
      <td>Theme H</td>
      <td>MEDIUM</td>
      <td>`PathBuf` as an Unsafe Context Primitive / Loss of IO Disjoint Invariant</td>
    </tr>
    <tr>
      <td>Theme I</td>
      <td>MEDIUM</td>
      <td>I/O Redundancy in Pipeline Data Flow</td>
    </tr>
    <tr>
      <td>Theme J</td>
      <td>MEDIUM</td>
      <td>Database Locking / Stop-Gap Policy Violation</td>
    </tr>
  </tbody>
</table>

## Theme A — Clap `#[command(flatten)]` Duplication Panic

- [Code Architect, Consistency Analyst, Comment Analyzer]: finding 'CRITICAL'

**Description**:
The plan mandates using `#[command(flatten)]` for `IngestArgs` etc. but also hoisting `--strict` and `--force` out, which causes a clap duplicate argument runtime panic.

**Remediation**:
Drop the `#[command(flatten)]` approach. Instead, create a clean `RunArgs` struct that explicitly defines `<path>`, `--output`, and the shared flags (`--strict`, `--force`). These flags will then be manually mapped into the underlying structs (e.g. `IngestArgs { strict: run_args.strict, ... }`) before being passed to the horizontal stage functions.

## Theme B — Performance Collapse via Unbatched ML Inference and SQLite Thrashing

- [Code Architect, Code Simplifier, Verification Agent]: finding 'CRITICAL'

**Description**:
The plan specifies transitioning from horizontal bulk execution to a vertical per-photo stream. This destroys machine learning batching for NIMA and SQLite batch performance, while adding excessive cognitive overhead.

**Remediation**:
Revert the architecture back to the horizontal bulk processing model (`ingest` all -> `cull` all -> `develop` all -> `export` all). Initialize `PipelineContext` (with SQLite and NIMA) once in the `run` command, and pass references to it across the horizontal stages.

## Theme C — Path Nesting Bypass / Incomplete Output Validation Loophole

- [Code Reviewer, Consistency Analyst, Verification Agent]: finding 'HIGH'

**Description**:
The plan checks if the output directory is nested inside the input directory, but does not canonicalize the output or check if output is exactly equal to the input, allowing path collision bypasses.

**Remediation**:
Require canonicalization of both `<path>` and `--output`. Assert that `canonicalize(output) != canonicalize(input)` and `canonicalize(output)` does not start with `canonicalize(input)`. Introduce an explicit invariant struct `ValidatedIO { input: PathBuf, output: PathBuf }` to enforce this disjoint property throughout the pipeline.

## Theme D — Test Coverage Gaps

- [PR Test Analyzer, Comment Analyzer, Verification Agent]: finding 'HIGH'

**Description**:
The integration testing relies on non-deterministic file system iteration order. It also lacks negative behavioral tests for `--min-rating`, missing output directory setup tests, and validation boundary tests for `--quality`.

**Remediation**:
- Rewrite idempotency test to use deterministic file sorting or multi-pass execution.
- Add negative tests to prove `min-rating` correctly skips exports.
- Add boundary tests explicitly verifying `clap` rejects `--quality 101` or `--min-rating 6`.
- Add test verifying behavior when `--output` path doesn't exist (it should auto-create or fail cleanly).

## Theme E — Error Swallowing and Fail-Open `INSERT OR IGNORE`

- [Silent Failure Hunter, Verification Agent]: finding 'HIGH'

**Description**:
The plan specifies strict execution halt on `Err`, but integration tests tie this to `--strict`, leaving the behavior of non-strict error swallowing implicitly defined as silent failure. Furthermore, using `INSERT OR IGNORE` swallows unrecoverable DB constraints.

**Remediation**:
- Clarify non-strict mode: errors must be caught, logged, and accumulated, continuing to the next photo, then failing the process at the end.
- `--strict` mode should leverage Rayon `try_for_each` or collect errors.
- Ban `INSERT OR IGNORE`. Use `INSERT ... ON CONFLICT (id) DO NOTHING` to ensure unrelated DB faults propagate properly.

## Theme F — Omission of `dedup` degrades `--lr-keywords`

- [Comment Analyzer, Consistency Analyst, Verification Agent]: finding 'MEDIUM'

**Description**:
The plan orchestrates the pipeline without the `dedup` stage, which causes `DevelopArgs` and `--lr-keywords` to fail since tier/cluster keywords rely on duplicate cluster IDs generated by `dedup`.

**Remediation**:
Insert the `dedup` stage into the horizontal pipeline orchestration (between `cull` and `develop`) so that `--lr-keywords` functions properly.

## Theme G — Restating existing bounds

- [Comment Analyzer, Verification Agent]: finding 'LOW'

**Description**:
The plan claims to add boundary validation for `--quality` and `--min-rating`, but `export.rs` already defines these exact limits.

**Remediation**:
Remove the claim of adding these bounds from the plan.

## Theme H — Unsafe Context Primitive

- [Type Design Analyzer, Verification Agent]: finding 'MEDIUM'

**Description**:
`PipelineContext` uses a raw `PathBuf` for the catalog, breaking the invariant that the path represents an existing validated database.

**Remediation**:
Use a strongly-typed, instantiated database connection or a `VerifiedCatalogPath` in `PipelineContext`. Use the `ValidatedIO` struct to enforce disjoint input/output paths.

## Theme I — I/O Redundancy

- [Code Architect, Verification Agent]: finding 'MEDIUM'

**Description**:
The pipeline writes an XMP sidecar in `develop` and reads it back from disk in `export`, introducing unnecessary redundant I/O in a unified memory pipeline.

**Remediation**:
Pass the parsed metadata state in memory between `develop` and `export` to avoid redundant file reads.

## Theme J — Database Locking / Stop-Gap Policy

- [Code Reviewer, Consistency Analyst, Verification Agent]: finding 'MEDIUM'

**Description**:
The plan verifies "graceful handling (timeout)" for DB locks, acting as a stop-gap for concurrency issues without documenting it as a tech debt.

**Remediation**:
Since we are reverting to horizontal bulk processing with large transactions, locking contention drops significantly. Configure SQLite with `PRAGMA journal_mode=WAL` in the `PipelineContext` initialization.

## Disposition summary

<table>
  <thead>
    <tr>
      <th>Theme</th>
      <th>Disposition</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Theme A</td>
      <td>Remediate (Explicit `RunArgs` mapping)</td>
    </tr>
    <tr>
      <td>Theme B</td>
      <td>Remediate (Revert to horizontal bulk execution)</td>
    </tr>
    <tr>
      <td>Theme C</td>
      <td>Remediate (Strict IO disjoint validation and canonicalization)</td>
    </tr>
    <tr>
      <td>Theme D</td>
      <td>Remediate (Enhance test determinism and coverage)</td>
    </tr>
    <tr>
      <td>Theme E</td>
      <td>Remediate (Strict non-strict logging and ON CONFLICT DO NOTHING)</td>
    </tr>
    <tr>
      <td>Theme F</td>
      <td>Remediate (Include `dedup` in pipeline)</td>
    </tr>
    <tr>
      <td>Theme G</td>
      <td>Remediate (Remove redundant task)</td>
    </tr>
    <tr>
      <td>Theme H</td>
      <td>Remediate (Type-safe DB and IO contexts)</td>
    </tr>
    <tr>
      <td>Theme I</td>
      <td>Remediate (In-memory metadata propagation)</td>
    </tr>
    <tr>
      <td>Theme J</td>
      <td>Remediate (Enable WAL mode instead of timeouts)</td>
    </tr>
  </tbody>
</table>

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 15
  verified: 15
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.0
```
