# Session 02 — libraw-cr3-decode, Review Round 2 (Post-Hoc, session 06 D1)

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent); verification inline"
  gate_state: pass
  cache_used: true
```

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 0 |
| LOW | 0 |
| **Total** | **0** |

**Round 2 is CLEAN.**

---

## R1 Watch-list Verification

**R2-A — Error-path integration tests** — CLOSED.
`tests/integration_cr3.rs` now has 3 error-path tests:
`read_cr3_returns_error_for_nonexistent_file`, `read_cr3_returns_error_for_non_raw_file`,
`read_raw_returns_error_for_non_raw_file`. No LFS fixtures needed.

**R2-B — UnsupportedFormat dead code** — CLOSED (TD-021 filed).
TD-021 filed in `TECH-DEBT.md` with binding trigger: DN-014 (non-Canon body support) or
pre-release. Variant remains but is now tracked.

**R2-C — WhiteBalance partial-zero** — CLOSED.
`from_libraw_cam_mul` now checks `r == 0.0 || g1 == 0.0 || b == 0.0` in addition to
the all-zero and NaN/Inf checks. G2=0.0 still accepted for 3-channel sensors.
Tests: `white_balance_rejects_zero_red_channel`, `white_balance_accepts_zero_g2_for_3channel_sensor`.

**R2-D — CamRgbToXyzD65Matrix all-zero row** — CLOSED.
`from_libraw_rgb_cam` now rejects rows where all entries are ≈0.0.
Tests: `color_matrix_rejects_all_zero_row`, `color_matrix_rejects_infinite_entry`.

**R2-E — C shim comment corrected** — CLOSED.
`photohelper_libraw_shim.c`: flip 5 → EXIF Rotate90Ccw (8); flip 6 → EXIF Rotate90Cw (6).

**R2-F — RawInvalidBitDepth missing path** — CLOSED.
`RawInvalidBitDepth` now carries `path: PathBuf`. `SensorBitDepth::new(path, bits)`.
All call sites and test pattern-matches updated.

---

## Disposition summary

| Theme | R1 severity | R2 status |
|---|---|---|
| A — Error-path tests | HIGH | CLOSED |
| B — UnsupportedFormat dead code | HIGH | CLOSED (TD-021) |
| C — Partial-zero WB | MEDIUM | CLOSED |
| D — All-zero color matrix | MEDIUM | CLOSED |
| E — C shim comment | MEDIUM | CLOSED |
| F — RawInvalidBitDepth path | MEDIUM | CLOSED |
| G — SensorLevels undiscriminated | MEDIUM | DEFERRED → TD-022 (low priority) |
| LOW themes | LOW | DEFERRED → TD-022 (acceptable for v0.1) |

**TD-011 is now fully CLOSED.** Session-02 post-hoc review complete (R1 + R2).

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 6
  verified: 6
  hallucinated: 0
  discard_rate: 0.00
```
