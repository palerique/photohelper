# ANL-002 — ort + NIMA Pre-flight Feasibility Probe

**Session**: 03 (`ai-culling-skeleton`)
**Date**: 2026-05-28
**Author**: Claude Code session 03 (Paulo Henrique Lerbach Rodrigues)
**Status**: **ABORT — model provenance/license requirement not met. See § ABORT Decision.**

---

## § ort version

**Chosen pin**: `=2.0.0-rc.12` (latest release candidate as of 2026-05-28).

| Field | Value |
|---|---|
| Crate | `ort` on crates.io |
| Pinned version | `2.0.0-rc.12` |
| Released | 2026-03-05 |
| Wrapped ONNX Runtime | 1.24 |
| Required Rust MSRV | 1.88 (matches our pinned toolchain) |
| Release status | Release Candidate (not stable 2.0.0) |
| Upgrade trigger | TD-014: upgrade to stable ort 2.0.0 when released |

**Notes**:
- `=` pin (exact version) is required because ort RC versions have no semver stability guarantee.
- MSRV 1.88 matches `rust-toolchain.toml` (`channel = "1.88.0"`); no toolchain bump needed.
- Cross-ref: TD-014 (filed session 03 plan-review) tracks the RC→stable upgrade.

---

## § CVE-posture-as-of-pin

**Posture as of 2026-05-28: CLEAN.**

| Advisory feed | Query | Result |
|---|---|---|
| RustSec advisory DB | `ort` package | 0 advisories |
| GitHub Security Advisories (pykeio/ort) | Repository advisories page | "There aren't any published security advisories." |
| OSV.dev (crates.io ecosystem) | `ort` package | 0 results |

No open CVE affects `ort =2.0.0-rc.12` as of the probe date.

The upstream ONNX Runtime C++ library (v1.24) was not separately queried against NVD/MITRE; the ort crate's advisory feeds are the authoritative surface for the Rust consumer.

---

## § NIMA model provenance

**Result: ABORT — no NIMA ONNX model with explicitly stated permissive license found.**

### Search conducted

| Source | Model candidate | License | Result |
|---|---|---|---|
| HuggingFace (ONNX + aesthetic filter) | `cromsc/nima-mobilenet-aesthetic` | None specified | ❌ ABORT |
| HuggingFace (ONNX + nima filter) | — | — | 0 results |
| GitHub (nima + onnx query) | — | — | 0 results |
| PINTO0309 model zoo | — | — | Not present |
| onnx-model-zoo | — | — | Not present |

### Candidate detail: `cromsc/nima-mobilenet-aesthetic`

- **Repository**: `https://huggingface.co/cromsc/nima-mobilenet-aesthetic`
- **File**: `nima_mobilenet_aesthetic.onnx`
- **License**: **None specified** (no LICENSE file, no license tag, empty model card)
- **Provenance**: Two commits on 2026-03-31 (`initial commit` + `Upload nima_mobilenet_aesthetic.onnx with huggingface_hub`); no description of weight source
- **Verdict**: Fails the plan's "clear license + provenance" requirement. Under US copyright law a work without an explicit license is assumed to be all-rights-reserved; distribution and use for inference would require the author's explicit permission.

### Likely derivative source (not confirmed)

The model architecture name (`nima_mobilenet_aesthetic`) and the MobileNet-on-AVA framing closely matches `idealo/image-quality-assessment` (Apache-2.0, 2241 stars). However:
- The `idealo` repo provides Keras `.hdf5` weights, not ONNX.
- No ONNX export script exists in the `idealo` repo.
- The `cromsc` upload contains no documentation linking it to `idealo`.

If the `cromsc` model is an unreleased derivative of the `idealo` weights, it would inherit the Apache-2.0 license, but this is **unconfirmed**.

### ABORT condition triggered

Per `docs/plans/session-03.md § D0 pre-flight`:
> ABORT if license is not in {MIT, Apache-2.0, CC-BY-4.0}

The only candidate has **no stated license**. The ABORT condition fires.

---

## § Threading semantics (BINDING — PR1-T5 + T-ε remediation)

**Finding: `Session::run` is `&mut self` → per-worker `thread_local!` path (plan option b) is CONFIRMED.**

Verified from `pykeio/ort` main branch source (`src/session/mod.rs`):

```rust
pub fn run<'s, 'i, 'v: 'i, const N: usize>(
    &'s mut self,
    input_values: impl Into<SessionInputs<'i, 'v, N>>,
) -> Result<SessionOutputs<'s>>
```

The receiver is `&mut self` (exclusive mutable reference). This means:
- `Session` is **not** `Sync` for concurrent callers on a shared `Arc<Session>`.
- Sharing one `Arc<Session>` across rayon workers and calling `run()` from multiple threads simultaneously would require a `Mutex<Session>`, which would serialize all inference calls (no parallelism benefit).
- **Correct approach**: construct one `Session` per rayon worker thread via `thread_local!`. The worker captures `Arc<VerifiedModelBytes>` cheaply; each thread builds its own `Session` from those bytes on first use. `VerifiedModelBytes` wraps `Arc<[u8]>` and is `Send + Sync`; the per-thread `Session` is `!Sync` and stays thread-local.

**Binding impact on D4 design** (recorded for when D0 ABORT is resolved in a future session):
- `run_cull` signature must be: `fn run_cull(cli: &Cli, args: &CullArgs, model: &VerifiedModelBytes) -> ExitCode`
- `thread_local!` block holds `RefCell<Option<Session>>` with lazy initialization from `model`
- See plan v4 § D4 option-b spec for full details.

---

## § Inference end-to-end

**Not conducted** — blocked by the model provenance ABORT (§ NIMA model provenance). No unlicensed model file was downloaded.

---

## § Per-photo wall-clock

**Not measured** — blocked by model provenance ABORT.

---

## § ABORT Decision

**D0 ABORT fires on condition: model license/provenance.**

Per `docs/plans/session-03.md § D0 pre-flight ABORT procedure`:
> session 03 narrows to D5 (TD-010 closure) + D6 (stub messages) + D7 (docs) only.
> No ort dep is wired; no model binary is committed.
> File a blocker discovery note and halt D0 through D4.

**Deliverables halted**: D0 (partial), D1a–D1d, D2a–D2c, D3, D4.
**Deliverables remaining**: D5 (TD-010 closure), D6 (**DONE** this session), D7 (docs).

Blocker discovery note: **DN-026** (filed in `docs/discovery-notes.md`).

---

## § Path forward (for future session)

Two viable resolution paths exist:

**Path A — Convert from Apache-2.0 source (recommended)**:
1. Export `idealo/image-quality-assessment` MobileNet aesthetic Keras weights to ONNX via `tf2onnx` (Python tooling required; can be a separate pre-session step).
2. Record: source repo = `idealo/image-quality-assessment` (Apache-2.0), export tool = `tf2onnx` (Apache-2.0), output license = Apache-2.0 (derivative).
3. SHA-256-verify the output; commit the derivation script + SHA-256 sidecar alongside the model binary.
4. Re-run D0 with the self-generated ONNX; proceed to D1–D4.

**Path B — Contact `cromsc` for license declaration**:
1. Open a HuggingFace discussion on `cromsc/nima-mobilenet-aesthetic` requesting an explicit Apache-2.0 or MIT license declaration.
2. If the author confirms and adds the LICENSE file, the ABORT condition lifts.
3. Re-run D0 verification; proceed to D1–D4.

**Recommendation**: Path A is self-sufficient and reproducible. Path B depends on a third party responding. Both paths require a fresh D0 + D1–D4 sequence; they slot naturally into session 04.
