# Session 02 — libraw-cr3-decode, Review Round 1 (Post-Hoc, session 06 D1)

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent); agents pinned to opus"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
  gate_state: pass
  cache_used: true
  note: "Post-hoc review — session-02 shipped with TD-011 deferral (within-3-sessions binding trigger, now overdue)"
```

```yaml
plugin_availability:
  schema_version: 1
  agents_requested: [general-purpose, feature-dev:code-architect, pr-review-toolkit:pr-test-analyzer]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 2 |
| MEDIUM | 5 |
| LOW | 5 |
| **Total** | **12** |

---

## Theme A — No error-path integration tests for `read_cr3` / `read_raw` / `read_raw_rgb` [HIGH]

**Agent**: pr-test-analyzer

All 4 integration tests in `tests/integration_cr3.rs` exercise happy paths with valid Canon R8 CR3
fixtures. There are zero tests that pass a nonexistent path, a non-RAW file, or a corrupt file to the
public entry points. A LibRaw version bump could silently change which error variant is returned for
file-not-found; no test would catch the regression.

**Remediation**: Add 3 tests that do NOT require LFS fixtures:
- `read_cr3_returns_error_for_nonexistent_file` — `read_cr3(Path::new("/nonexistent/file.cr3"))` → `Err(...)`
- `read_cr3_returns_error_for_non_raw_file` — create a temp file with `b"not a cr3 file"`, pass it → `Err(...)`
- `read_raw_returns_error_for_non_raw_file` — same pattern for `read_raw`

---

## Theme B — `RawExifCause::UnsupportedFormat` variant is dead code [HIGH]

**Agent**: pr-test-analyzer

`UnsupportedFormat { libraw_make: String, libraw_model: String }` at `lib.rs:166` is defined but
never constructed anywhere in `ffi.rs`, `exif.rs`, or `decode.rs`. It is never tested.
The empty-make case (`make.is_empty()` at `ffi.rs:257`) returns `ExifFieldsMissing`, not
`UnsupportedFormat`. If intended for future camera filtering (e.g., reject non-Canon bodies),
it needs a producer and a test; if not, it should be removed.

**Remediation**: Option A — wire a producer: add a Canon-make check in `parse_libraw_fields` and
return `UnsupportedFormat { libraw_make, libraw_model }` when make != "Canon". Option B — file a TD
noting it is reserved for DN-014 (non-Canon body support). Option C — remove the variant.

---

## Theme C — `WhiteBalance` accepts partial-zero `cam_mul` (R=0 or B=0) [MEDIUM]

**Agents**: code-architect, general-purpose

`from_libraw_cam_mul` (decode.rs:409) only rejects ALL four channels being zero. A `cam_mul` of
`[0.0, 1.0, 1.4, 1.0]` (R=0) passes both the all-zero check and the `!x.is_finite() || *x < 0.0`
check (0.0 is finite and not negative). A zero R or B multiplier is physically impossible and would
produce a completely black channel downstream. G2 (index 3) = 0 is legitimate for 3-channel Canon
sensors.

**Remediation**: Change the per-channel check to `*x < 0.0` → `*x <= 0.0` for channels 0, 1, 2 only.
Or: add `if cam_mul[0] == 0.0 || cam_mul[1] == 0.0 || cam_mul[2] == 0.0 { return Err(...) }` after the
all-zero check, before the NaN check.

---

## Theme D — `CamRgbToXyzD65Matrix` accepts all-zero matrix [MEDIUM]

**Agent**: code-architect

`from_libraw_rgb_cam` (decode.rs:471) rejects the identity matrix and NaN/Inf entries, but an
all-zero matrix passes both checks (not identity, all finite). An all-zero color matrix produces
a completely black image after color management. LibRaw can return an all-zero matrix when the
color matrix is unloaded for an unsupported camera model.

**Remediation**: Add a zero-row check:
```rust
if rgb_cam.iter().any(|row| row.iter().all(|&v| v.abs() < 1e-6)) {
    return Err(Error::RawDecodeFailed { path: path.to_path_buf(),
        cause: RawDecodeCause::ColorMatrixInvalid });
}
```

---

## Theme E — C shim comment documents wrong flip-to-EXIF mapping [MEDIUM]

**Agents**: general-purpose, code-architect

`photohelper_libraw_shim.c:32` says:
```
flip 5 -> EXIF Rotate90Cw (6)
flip 6 -> EXIF Rotate270Cw (8)
```

Per LibRaw's `dcraw_common.cpp` mapping table and the verified Rust implementation at `ffi.rs:358-365`:
- flip 5 → `Rotate90Ccw` (EXIF 8) ← C comment is wrong
- flip 6 → `Rotate90Cw` (EXIF 6) ← C comment is wrong

The Rust code is correct; only the C shim comment is misleading.

**Remediation**: Fix `photohelper_libraw_shim.c:32` comment:
```c
 *   flip 5 -> EXIF Rotate90Ccw (8) -- 270°CW
 *   flip 6 -> EXIF Rotate90Cw  (6) -- 90°CW
```

---

## Theme F — `RawInvalidBitDepth` variant missing `path: PathBuf` field [MEDIUM]

**Agent**: general-purpose

Every other `Error` / `RawDecodeCause` variant carries `path: PathBuf` for operator triage. `RawInvalidBitDepth { value: u8 }` (lib.rs:129) does not. In a batch of 370 photos, an error log reading `"RAW invalid bit depth: 7"` with no file path is unactionable.

**Remediation**: Add `path: PathBuf` to `RawInvalidBitDepth`. Update `SensorBitDepth::new` to accept
`path: &Path`. Update the call site in `ffi.rs`. Update error Display string.

---

## Theme G — `SensorLevels::new` rejection branches indistinguishable [MEDIUM]

**Agent**: general-purpose

Three rejection branches (inverted levels, too-narrow range, exceeds bit depth) all return
`Error::RawInvalidLevels { path, black, white }` with no discriminator field. An operator log
reading `"RAW invalid sensor levels at /path: black=1000, white=1100"` cannot distinguish
"inverted" from "too narrow range" without re-deriving the logic.

**Remediation**: Add `reason: &'static str` field to `RawInvalidLevels` (e.g., `"inverted"`,
`"range-too-narrow"`, `"exceeds-bit-depth"`). Low priority — can be filed as a TD if deferred.

---

## Disposition summary

| Theme | Severity | Disposition |
|---|---|---|
| A — No error-path tests | HIGH | Remediate: add 3 tests |
| B — UnsupportedFormat dead code | HIGH | Remediate: file TD-021 with binding trigger (DN-014) |
| C — Partial-zero WB accepted | MEDIUM | Remediate inline |
| D — All-zero color matrix accepted | MEDIUM | Remediate inline |
| E — C shim comment wrong | MEDIUM | Remediate inline (1 line) |
| F — RawInvalidBitDepth missing path | MEDIUM | Remediate inline |
| G — SensorLevels undiscriminated | MEDIUM | Defer → TD-022 (operator-triage quality; not correctness) |
| LOW themes (L1-L5) | LOW | Defer → TD-022 or inline trivial fixes |

## R2 watch-list
- [ ] R2-A: `read_cr3_returns_error_for_nonexistent_file` test added
- [ ] R2-B: TD-021 filed for `UnsupportedFormat` dead code
- [ ] R2-C: `from_libraw_cam_mul` rejects partial-zero (R=0, B=0)
- [ ] R2-D: `from_libraw_rgb_cam` rejects all-zero matrix
- [ ] R2-E: C shim comment corrected
- [ ] R2-F: `RawInvalidBitDepth` has `path: PathBuf` field

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 7
  verified: 7
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
```
