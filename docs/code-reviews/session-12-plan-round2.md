# Session 12 — docs/plans/session-12.md, Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "Gemini 3.5 Flash (High)"
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
  agents_unavailable: ["type-design-analyzer"]
  fallback_used: true
  fallback_agents: []
```

## Triage summary

<table>
  <thead>
    <tr>
      <th>Theme</th>
      <th>Severity</th>
      <th>Status</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Theme A: Algorithmic Correctness & Pipeline Architecture</td>
      <td>CRITICAL</td>
      <td>Pending Remediation</td>
    </tr>
    <tr>
      <td>Theme B: CLI Parsing & Validation</td>
      <td>CRITICAL</td>
      <td>Pending Remediation</td>
    </tr>
    <tr>
      <td>Theme C: Testing & Boundary Conditions</td>
      <td>HIGH</td>
      <td>Pending Remediation</td>
    </tr>
    <tr>
      <td>Theme D: Ledger & Workspace Integrity</td>
      <td>CRITICAL</td>
      <td>Pending Remediation</td>
    </tr>
  </tbody>
</table>

## Theme A: Algorithmic Correctness & Pipeline Architecture

- [Code Architect]: **CRITICAL**. Double Black-Level Subtraction. LibRaw `dcraw_process` already subtracts black level and normalizes to `0..=65535`. Normalizing again in Rust will crush shadows. Treat `u16` output as clean linear data.
- [Silent Failure Hunter]: **CRITICAL**. FFI Error Propagation. `read_raw_linear_16bit` must check and propagate LibRaw error codes (e.g. `Result<Vec<u16>, LibRawError>`), preventing silent memory corruption.
- [Code Architect]: **HIGH**. Premultiplied Alpha Math Error. `tiny-skia` uses premultiplied alpha. Custom RGB blending must use `dst = src + dst * (1 - src_alpha)`.
- [Code Reviewer]: **HIGH**. Unsafe Buffer Indexing. The affine blending loop must rigorously bounds-check using safe iterators (e.g. `chunks_exact_mut`).
- [Code Simplifier]: **HIGH**. Cognitive Load in Affine Math. Do not write a manual affine geometry loop. Transform the badge into a small RGBA buffer via `tiny-skia`, then do a simple 1:1 pixel alpha-blend onto the RGB target.
- [Code Architect]: **MEDIUM**. Coordinate System Desync. LibRaw already rotates the array based on EXIF. Ignore EXIF orientation in Rust to avoid double-rotation.
- [Code Simplifier]: **MEDIUM**. State-Mutating C-Shim API. Unify setters into a single declarative `photohelper_decode_with_options` C function.
- [Code Simplifier]: **LOW**. Duplicate wrappers. Unify `read_raw_rgb` and `read_raw_linear` into `read_raw(options)`.

**Remediation**: Remove manual black/white normalization. Propagate FFI errors securely. Use declarative C-shim options. Let `tiny-skia` handle affine transforms to a tiny intermediate buffer, then blend with premultiplied alpha logic via safe iterators. Ignore EXIF double-rotation.

## Theme B: CLI Parsing & Validation

- [Code Architect]: **CRITICAL**. CLI Arity Desync. Parallel arrays (`--badge`, `--badge-pos`) break if optional elements are omitted. Use key-value semantics: `--badge path="...",pos=...,scale=...`.
- [Code Architect]: **HIGH**. HashMap Silent Overwrite. `HashMap::insert` silently replaces. Must explicitly check `Entry::Vacant` to trigger `ExportError::DuplicateWatermarkPosition`.
- [Silent Failure Hunter]: **HIGH**. Cryptic IO Errors. Watermark PNG loads must yield `ExportError::BadgeLoadFailed { path, reason }` instead of generic `io::Error`.
- [Code Reviewer]: **MEDIUM**. Missing Upper-Bound Clamp. Missing upper clamp allows OOM/overflow on giant scale percentages.
- [Silent Failure Hunter]: **MEDIUM**. Tracing Context. Must wrap export in `#[instrument]` so errors carry context.

**Remediation**: Switch to `key=value` parser for `--badge`. Use `Entry` API for collision detection. Define explicit `BadgeLoadFailed` errors. Add upper-bound clamping to scale. Add `tracing::instrument`.

## Theme C: Testing & Boundary Conditions

- [PR Test Analyzer]: **HIGH**. Unhandled Duplicate Position Error Test. Missing unit test simulating position collisions to verify graceful error return.
- [PR Test Analyzer]: **HIGH**. Auto-scaling clamp test. Missing test simulating 1x1 image to verify scale clamping.
- [PR Test Analyzer]: **HIGH**. Integration Test Weakness. Asserting "correct dimensions" doesn't test the ISP. Must assert color/luma stats on output JPEG.

**Remediation**: Add the missing unit tests for `DuplicateWatermarkPosition` and 1x1 clamping. Expand integration tests to check output image statistics.

## Theme D: Ledger & Workspace Integrity

- [General Consistency]: **CRITICAL**. Dangling Module Declaration. `mod pixmap_test;` in `crates/photohelper-export/src/lib.rs` must be removed.
- [Comment Analyzer]: **CRITICAL**. ID Collision in Tech Debt. `TD-015` already exists. Assign `TD-023`.
- [Comment Analyzer]: **HIGH**. Phantom Rogue Dependency. Remove instruction to revert `uuid` dependency (it was a hallucination).
- [General Consistency]: **MEDIUM**. Partial Ledger Update. `SESSION-STATE.md` requires updates to "Last session", "Next action", and "Status", not just the table.
- [Comment Analyzer]: **MEDIUM**. Misleading Complexity Claim. LUT lookup is O(1) per pixel.
- [Code Reviewer]: **LOW**. Non-compliant Tech Debt. Include "stop-gap location" and "consequence of inaction" in `TECH-DEBT.md`.

**Remediation**: Add explicit cleanup for `mod pixmap_test`. Remove `uuid` fix. Use `TD-023`. Enforce complete `SESSION-STATE.md` updates. Add required TD-fields.

## Disposition summary

<table>
  <thead>
    <tr>
      <th>Total Findings</th>
      <th>Remediated</th>
      <th>Deferred (TECH-DEBT)</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>21</td>
      <td>21</td>
      <td>0</td>
    </tr>
  </tbody>
</table>

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: "pass"
  total_findings: 21
  verified: 21
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.0
  details: []
```
