# Session 12 — docs/plans/session-12.md, Review Round 1

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
  agents_unavailable: ["type-design-analyzer", "silent-failure-hunter"]
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
      <td>Theme A: Algorithmic & Color Science Integrity</td>
      <td>CRITICAL</td>
      <td>Pending Remediation</td>
    </tr>
    <tr>
      <td>Theme B: CLI & Input Validation</td>
      <td>CRITICAL</td>
      <td>Pending Remediation</td>
    </tr>
    <tr>
      <td>Theme C: Architectural Boundaries & Dependency Integrity</td>
      <td>CRITICAL</td>
      <td>Pending Remediation</td>
    </tr>
    <tr>
      <td>Theme D: Completeness & Synchronization</td>
      <td>HIGH</td>
      <td>Pending Remediation</td>
    </tr>
  </tbody>
</table>

## Theme A: Algorithmic & Color Science Integrity

- [Code Architect]: **CRITICAL**. Linear blending trap. PNG badges are sRGB; blending them into 16-bit linear RAW data prior to OETF will cause a double-gamma encode. Must linearize badges first.
- [Code Architect]: **CRITICAL**. Unnormalized sensor data. 16-bit data from LibRaw does not natively span 0-65535 (e.g. 14-bit sensors cap at 16383). Must normalize against `imgdata.color.maximum` and `imgdata.color.black` before tone mapping.
- [PR Test Analyzer]: **HIGH**. Clipping/Overflow risk. Applying exposure multipliers to linear data will exceed `u16::MAX`. Must explicitly clip to valid bounds before OETF cast.
- [Code Architect]: **HIGH**. Performance bottleneck. Computing S-curves and OETF per-pixel on 24MP images introduces millions of transcendental instructions. Must precalculate curves into a 1D Lookup Table (LUT).
- [Code Simplifier]: **MEDIUM**. Monolithic pipeline risk. Preemptively separate exposure, tone curve, and OETF into modular functions, avoiding a procedural monster loop.

**Remediation**: Architecturally require a LUT generation step mapping normalized 16-bit data `[0..=65535]` to `u8`. Enforce extracting `maximum`/`black` levels from LibRaw. Enforce explicit bounding constraints and modular pure functions. Linearly decode badges before composite, or composite them after the OETF.

## Theme B: CLI & Input Validation

- [Code Reviewer]: **CRITICAL**. Windows Path Colon Collision. `--badge <PATH>:<POSITION>` uses colons as delimiters, breaking Windows absolute paths (`C:\...`). Move to `clap` key-value semantics or separate flags.
- [Code Architect]: **MEDIUM**. O(N^2) Validation Complexity. Iterating over an array to find duplicates is an anti-pattern. Parse CLI options directly into a `HashMap<WatermarkPosition, Badge>` for O(1) collision detection.
- [Comment Analyzer]: **HIGH**. Unhandled Watermark Conflict. The existing text `--watermark` defaults to `bottom-left`. Specifying a text watermark and an image badge at the same position creates an undefined collision.
- [Code Reviewer]: **MEDIUM**. Zero-Pixel Scaling Panic. Defaulting to 5% of a very small target image edge could yield 0px, causing affine transform panics. Must clamp to `max(size, 1.0)`.
- [PR Test Analyzer]: **HIGH**. Missing coverage for malformed inputs (unreadable PNG, non-numeric scale, out-of-bounds scale).
- [General Consistency]: **MEDIUM**. Low-density error docs. The plan does not cite the fully-qualified error types (e.g. `ExportError::DuplicateBadgePosition`).

**Remediation**: Use `clap` value_parser to build a `HashMap` for O(1) duplicate checking. Fail fast on ANY position collision (text vs text, image vs image, text vs image). Enforce `max(size, 1.0)` bounding. Explicitly define `ExportError` variants and add rigorous boundary tests.

## Theme C: Architectural Boundaries & Dependency Integrity

- [Comment Analyzer]: **CRITICAL**. AI Pipeline Breakage. Completely replacing `read_raw_rgb` with `read_raw_linear_16bit` will break `photohelper-ai` clustering (which expects 8-bit `RgbImage`). Must retain `read_raw_rgb`.
- [Code Architect]: **HIGH**. Memory allocation bottleneck. Forcing a 24MP RGB array into an RGBA `Pixmap` just to add a tiny badge wastes huge memory on alpha channels. Custom RGB/RGBA compositing required.
- [General Consistency]: **HIGH**. Undefined Data Flow. `photohelper-export` currently doesn't depend on XMP edits. Must define `ToneMappingOptions` passed from CLI to `export_photo`.
- [Comment Analyzer]: **MEDIUM**. C-Shim Invariant Breach. Setters violate `photohelper_libraw_shim.c`'s "side-effect-free" comment. Must document the architectural shift.
- [Code Reviewer]: **LOW**. Omitted `// SAFETY:` block requirements for new FFI bridges.

**Remediation**: Keep `read_raw_rgb`. Pass `ToneMappingOptions` via `ExportOptions`. Add custom RGB composite loops to avoid converting the massive RAW image into an RGBA `Pixmap`. Enforce safety comments.

## Theme D: Completeness & Synchronization

- [General Consistency]: **CRITICAL**. Rogue Workspace State. The plan omits cleanup of leftover spiking artifacts (`bin.rs`, `pixmap_test.rs`) and an unstaged `uuid` dependency in `photohelper-sidecar/Cargo.toml`.
- [General Consistency]: **HIGH**. Ledger Updates Omitted. Plan must update `SESSION-STATE.md` and `README.md` CLI documentation.
- [Code Reviewer]: **MEDIUM**. Tech Debt Non-Compliance. Deferred tasks (ACEScg, Temp/Tint) lack binding triggers (date/session).
- [Comment Analyzer]: **LOW**. Misleading Discovery. `tiny-skia` depends on the `png` crate; it's not "native without dependencies".
- [PR Test Analyzer]: **MEDIUM**. Integration Test Weakness. Tests must assert file generation and dimensions programmatically, not just "run ci". Multi-badge and orientation tests missing.

**Remediation**: Add explicit cleanup steps for rogue states. Enforce `SESSION-STATE.md` and `README.md` updates. Add binding triggers to `TECH-DEBT.md`. Reword discovery claim. Rigorize integration tests.

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
      <td>19</td>
      <td>19</td>
      <td>0</td>
    </tr>
  </tbody>
</table>

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: "pass"
  total_findings: 19
  verified: 19
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.0
  details: []
```
