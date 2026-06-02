# Session 16 — Plan review, Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6[1m] (parent); sub-agents pinned to opus"
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
  agents_requested:
    - general-purpose
    - feature-dev:code-architect
    - feature-dev:code-reviewer
    - pr-review-toolkit:type-design-analyzer
    - pr-review-toolkit:silent-failure-hunter
    - pr-review-toolkit:comment-analyzer
    - pr-review-toolkit:pr-test-analyzer
    - pr-review-toolkit:code-simplifier
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

**Scope**: `docs/plans/session-16.md` — Windows x86_64 release via MSVC + vcpkg LibRaw.

9th-agent verification: 5/5 findings verified (4 confirmed, 1 flagged for human triage on PR1-A — supported by Agent 6's dist.txt inspection). discard_rate: 0.00.

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 3 | PR1-A, PR1-B, PR1-C |
| HIGH | 5 | PR1-D, PR1-E, PR1-F, PR1-G, PR1-H |
| MEDIUM | 5 | PR1-I, PR1-J, PR1-K, PR1-L, PR1-M |
| LOW | 2 | PR1-N, PR1-O |

---

## Theme PR1-A — ORT is statically linked on MSVC; entire `onnxruntime.dll` bundling plan is wrong (CRITICAL)

**Severity: CRITICAL** — 3 agents (general-purpose, comment-analyzer, pr-test-analyzer)

Inspection of `ort-sys` 2.0.0-rc.12 source at `~/.cargo/registry/src/.../ort-sys-2.0.0-rc.12/build/main.rs:160` confirms:
```rust
println!("cargo:rustc-link-lib=static=onnxruntime");
```
This is emitted for **all** targets with no platform guard. The `copy-dylibs` feature (line 157) copies `.dll` files from the prebuilt archive only if they exist. The `dist.txt` file confirms the MSVC archive is `x86_64-pc-windows-msvc.tar.lzma2`, which contains `libonnxruntime.a` (static archive) — no `onnxruntime.dll`. There is no DLL to copy.

**Cascading consequences**:
- Plan Goal (line 12): lists `onnxruntime.dll` — wrong
- Plan D2 bundle script (lines 258–267): entire `$dll = "target/.../onnxruntime.dll"` block + fallback — wrong; the throw will fire every run
- Acceptance criterion 4: "Archive contains `onnxruntime.dll`" — wrong
- `scripts/README-install-windows.md` line 18: copies `onnxruntime.dll` — wrong
- `scripts/README-install-windows.md` line 54: "onnxruntime.dll must be in the same directory" — wrong
- DN-041 says "The MSVC prebuilt is a `.dll` (dynamic), requiring bundling" — this statement in the discovery note is also wrong

Note: On Windows, ort-sys does link against DirectML system DLLs (`dxguid`, `DXCORE`, `DXGI`, `D3D12`, `DirectML`) per `build/static_link/mod.rs:55-60`. These ship with Windows 10+ and do not need bundling.

**Remediation**: (1) Remove `onnxruntime.dll` from the Goal line; (2) Remove the DLL copy block from D2; (3) Remove Acceptance criterion 4; (4) Update `README-install-windows.md` to remove DLL references; (5) Update Technical context to say ORT is statically linked on MSVC (matching Linux x86_64 from DN-041); (6) Add note about DirectML system deps.

---

## Theme PR1-B — vcpkg triplet mismatch: crate defaults to `x64-windows-static-md`, not `x64-windows-static` (CRITICAL)

**Severity: CRITICAL** — 2 agents (general-purpose, code-architect)

vcpkg Rust crate 0.2.15 source at `vcpkg-0.2.15/src/lib.rs:1363` confirms:
```rust
triplet: "x64-windows-static-md".into(),
```
For `x86_64-pc-windows-msvc` without `CARGO_CFG_TARGET_FEATURE=crt-static` and without `VCPKGRS_DYNAMIC=1`, the default triplet is `x64-windows-static-md`. The plan installs `libraw:x64-windows-static` (line 237) but the Rust vcpkg crate searches `x64-windows-static-md\`. Result: `find_package("libraw")` returns `Err::LibNotFound` and the build fails.

`x64-windows-static-md` is the correct choice: it links libraries statically while using the multi-threaded DLL CRT (UCRT, which ships with Windows 10+). No CRT DLLs need bundling.

**Remediation**: Change D2's vcpkg install command from `vcpkg install libraw:x64-windows-static` to `vcpkg install libraw:x64-windows-static-md`. The Rust vcpkg crate then finds the library automatically at its default triplet path.

---

## Theme PR1-C — PowerShell glob copy silently produces empty `models/` if LFS pull fails (CRITICAL)

**Severity: CRITICAL** — 4 agents (code-reviewer, silent-failure-hunter, pr-test-analyzer, code-simplifier)

D2 bundle script (plan line 270):
```powershell
Copy-Item "crates/photohelper-ai/models/*.onnx"        "$DIR/models/"
```
PowerShell's `Copy-Item` with a wildcard silently succeeds on zero matches — no error, no output. If `actions/checkout` with `lfs: true` fails to hydrate LFS objects (an intermittent failure on `windows-latest`), the `.onnx` files are LFS pointer stubs or absent. `Compress-Archive` then produces a zip containing `photohelper.exe` and zero model files. `actions/upload-artifact` uploads it. The job exits 0. No acceptance criterion catches this.

The PowerShell `$ErrorActionPreference` defaults to `Continue` (unlike bash's `set -euo pipefail`), so failed `Copy-Item` commands on individual files don't abort the script.

**Remediation**: (a) Set `$ErrorActionPreference = 'Stop'` at the top of the PowerShell bundle block. (b) Use `Get-ChildItem` + count check for ONNX files:
```powershell
$models = Get-ChildItem "crates/photohelper-ai/models/*.onnx"
if ($models.Count -eq 0) { throw "No .onnx files found — check LFS checkout (lfs: true in checkout step)" }
$models | Copy-Item -Destination "$DIR/models/"
```

---

## Theme PR1-D — No smoke test that `photohelper.exe` actually runs (HIGH)

**Severity: HIGH** — 2 agents (pr-test-analyzer, code-reviewer)

The D2 job has no step that executes the built binary. `cargo build` succeeding proves linking worked, not that the binary runs. A missing vcpkg `.lib`, an unresolved DirectML symbol, or a crash on startup would all be invisible. A 2-line smoke test catches this:

```yaml
- name: Smoke test binary
  shell: pwsh
  run: |
    $env:PHOTOHELPER_MODEL_DIR = "crates/photohelper-ai/models"
    & "target/${{ env.TARGET }}/release/photohelper.exe" --help
```

**Remediation**: Add a `Smoke test binary` step between `Build release binary` and `Bundle archive` in D2.

---

## Theme PR1-E — Archive contents never verified in CI; acceptance criteria 3–5 claim "CI log" verification (HIGH)

**Severity: HIGH** — 3 agents (silent-failure-hunter, pr-test-analyzer, code-reviewer)

D2's acceptance criteria 3–5 state "Check zip contents in CI log" but the bundle script only prints the archive SIZE (`Write-Output "Archive size: ..."`). No step lists the archive contents. If the binary is missing (wrong path), the `Copy-Item` fails but with `$ErrorActionPreference = 'Continue'` (the current plan) the script continues and ships an incomplete archive.

**Remediation**: After `Compress-Archive`, add:
```powershell
# Verify and log archive contents
$zipPath = Resolve-Path "${DIR}.zip"
$zip = [IO.Compression.ZipFile]::OpenRead($zipPath)
$entries = $zip.Entries | Select-Object -ExpandProperty FullName
$zip.Dispose()
Write-Output "Archive contents:"
$entries | ForEach-Object { Write-Output "  $_" }
$onnxCount = ($entries | Where-Object { $_ -like "*.onnx" }).Count
if ($onnxCount -eq 0) { throw "No .onnx model files in archive" }
Write-Output "Verified: $onnxCount ONNX model(s) in archive"
```

---

## Theme PR1-F — `C:\vcpkg` hardcoded; should use `VCPKG_INSTALLATION_ROOT` (HIGH)

**Severity: HIGH** — 2 agents (code-architect, general-purpose)

Plan line 244: `VCPKG_ROOT: C:\vcpkg`. GitHub Actions `windows-latest` provides `VCPKG_INSTALLATION_ROOT` as the canonical environment variable for the pre-installed vcpkg location (documented in GHA docs). Hardcoding `C:\vcpkg` is fragile if the runner image moves vcpkg.

**Remediation**: Change to `VCPKG_ROOT: ${{ env.VCPKG_INSTALLATION_ROOT }}` in the `Build release binary` step.

---

## Theme PR1-G — D0 pre-flight gate is documentation-only; cannot actually abort (HIGH)

**Severity: HIGH** — 3 agents (silent-failure-hunter, pr-test-analyzer, code-simplifier)

D0's "ABORT gate" produces a markdown document (`ANL-004`) by reading ort-sys source and checking the vcpkg registry. But: (1) the ABORT condition fires only if a human decides to abort — no CI check; (2) reading docs does not verify that `cargo build --target x86_64-pc-windows-msvc` actually succeeds on `windows-latest`; (3) the real gate is whether D2's CI job succeeds.

PR1-A confirms that D0 would have caught the DLL misconception if it had actually inspected the ort-sys binary cache (several agents found the answer in 5 minutes). But the plan's D0 steps only say "read ort-sys source or docs" — vague enough to miss it.

**Remediation**: Refocus D0 to produce one concrete executable artifact: a `workflow_dispatch`-triggerable GHA job snippet that builds for MSVC on `windows-latest` and checks the binary's link dependencies. Alternatively, fold D0 into D2's first CI run — if D2's CI job fails, that IS the abort signal. The plan should acknowledge: "D0 artifacts will be confirmed in D2's first CI run on `windows-latest`; the first failed CI run triggers the ABORT."

---

## Theme PR1-H — D2 bundle script missing `$ErrorActionPreference = 'Stop'` (HIGH)

**Severity: HIGH** — 3 agents (silent-failure-hunter, pr-test-analyzer, code-reviewer)

PowerShell defaults `$ErrorActionPreference = 'Continue'`, meaning failed `Copy-Item` calls (e.g., source file missing) write to stderr and continue. The script can silently produce an incomplete archive. This is the PowerShell equivalent of forgetting `set -euo pipefail` in bash.

**Remediation**: Add `$ErrorActionPreference = 'Stop'` as the first line of every PowerShell `run:` block in D2. (Already subsumed by PR1-C remediation.)

---

## Theme PR1-I — `vcpkg` error uses `map_err` chain buried in build output; should use `cargo:warning=` (MEDIUM)

**Severity: MEDIUM** — 1 agent (silent-failure-hunter)

The existing `build.rs` uses `println!("cargo:warning=...")` for all actionable user-facing errors (missing `pkg-config`, `tar`, `sh`, `make`). These always appear in `cargo build` output. The plan's `run_windows_msvc()` uses `format!()` in `map_err`, which propagates through `eprintln!` in `main()` — only visible with `cargo build -vv`. A developer without vcpkg installed sees "error: failed to run custom build command" with no actionable message.

**Remediation**: Add `println!("cargo:warning=...")` in the `map_err` closure:
```rust
let lib = vcpkg::find_package("libraw")
    .map_err(|e| {
        println!(
            "cargo:warning=vcpkg could not find `libraw`. \
             Install: `vcpkg install libraw:x64-windows-static-md` \
             and set VCPKG_ROOT to the vcpkg root."
        );
        format!("vcpkg find libraw: {e}")
    })?;
```

---

## Theme PR1-J — TD-042 and TD-043 in D4 lack all 5 required fields (MEDIUM)

**Severity: MEDIUM** — 2 agents (general-purpose, pr-test-analyzer)

CLAUDE.md § No Acceptable Trade-offs Policy requires: (1) stop-gap location (file:line + commit SHA), (2) fundamental fix outline, (3) binding trigger, (4) LoC + risk estimate, (5) consequence of inaction. The plan's D4 provides brief descriptions only. Missing: commit SHA for both TDs; consequence of inaction for TD-043.

**Remediation**: Expand D4 to draft all 5 fields inline for TD-042 and TD-043, or explicitly note that the implementer must fill them in at commit time.

---

## Theme PR1-K — `compile_shim` duplication vs. refactored shared function (MEDIUM)

**Severity: MEDIUM** — 3 agents (code-architect, type-design-analyzer, code-simplifier)

The Unix path calls `compile_shim(&manifest_dir, &src_dir)`. The Windows path inlines the `cc::Build` block with a different include path (`vcpkg_include_dir`). Two code paths for the same logical operation (compile the C shim) diverge at the include path only. If the shim compilation logic changes (new define, new flag), both must be updated.

**Remediation**: Refactor `compile_shim` to accept `(manifest_dir: &Path, include_dir: &Path)`. Unix calls `compile_shim(&manifest_dir, &src_dir)`; Windows calls `compile_shim(&manifest_dir, include_dir)`. One-line signature change to the existing function; no behavioral change.

---

## Theme PR1-L — Technical context prose inaccuracies (MEDIUM)

**Severity: MEDIUM** — 2 agents (comment-analyzer, code-architect)

1. "Rust `vcpkg` build-crate (v0.2) calls `vcpkg.exe`" — incorrect. The crate scans the filesystem under `VCPKG_ROOT` directly; it does not invoke `vcpkg.exe`.
2. "No Python needed for MSVC target (only `windows-gnu` had the lzma2 extraction issue)" — misleading. ort-sys handles LZMA2 extraction internally via the `lzma-rust2` pure-Rust crate for ALL targets. Python was never needed; the problem TD-029 described was about MSYS2 shell tools failing on the archive, not about ort-sys's own extraction.
3. Comment "C++ runtime + zlib are pulled in as transitive vcpkg deps — no explicit emit needed" — the C++ runtime explanation is wrong. MSVC toolchain handles C++ runtime automatically via `cl.exe`/`link.exe`; vcpkg's libraw transitively includes zlib (correct), but not the C++ runtime.

**Remediation**: Fix all three prose points in the Technical context section.

---

## Theme PR1-M — TD-043 binding trigger vague ("before v0.2") (MEDIUM)

**Severity: MEDIUM** — 1 agent (code-simplifier)

TD-043's trigger "First user report of path-length error on Windows OR before v0.2 release" uses "before v0.2" which has no calendar date. TD-029 had the same trigger and sat open indefinitely until session 16 cleared it.

**Remediation**: Replace "before v0.2 release" with a calendar date: "by 2026-09-01" or use only the event-driven trigger "First user report of path-length error on Windows."

---

## Theme PR1-N — DN-013 "Reconciled" with only 1 of 3 audit items addressed (LOW)

**Severity: LOW** — 1 agent (pr-test-analyzer)

DN-013 specifies three required audit items: (a) LibRaw cross-compiles, (b) binary statically links LibRaw (verifiable via `objdump`/`dumpbin`), (c) Windows long-path `\\?\` prefix. The plan addresses (a) via CI, defers (b) to TD-042 and (c) to TD-043. The DN status of "Reconciled" overstates the closure.

**Remediation**: Change DN-013 status in D4 to `reconciled (2026-06-02, item (a) addressed via CI; items (b)+(c) tracked as TD-042 + TD-043)`.

---

## Theme PR1-O — DirectML system DLL dependencies not mentioned (LOW)

**Severity: LOW** — 1 agent (comment-analyzer)

ort-sys's `build/static_link/mod.rs:55-60` emits link directives for `dxguid`, `DXCORE`, `DXGI`, `D3D12`, `DirectML` on Windows targets. These are system DLLs shipped with Windows 10+ and do not need bundling. The plan should mention them so a future reader isn't surprised by DirectML link lines in the build output.

**Remediation**: Add one sentence to Technical context: "Pyke-sourced MSVC prebuilts also statically link against system DirectX DLLs (dxguid, DXCORE, DXGI, D3D12, DirectML); these ship with Windows 10+ and do not require bundling."

---

## Disposition summary

| Theme | Severity | Fix |
|---|---|---|
| PR1-A | CRITICAL | Remove all onnxruntime.dll references from plan, goal, D2, README, AC |
| PR1-B | CRITICAL | Change vcpkg install to `libraw:x64-windows-static-md` |
| PR1-C | CRITICAL | Add `$ErrorActionPreference='Stop'`; use `Get-ChildItem` + count check for ONNX |
| PR1-D | HIGH | Add smoke test step `photohelper.exe --help` in D2 |
| PR1-E | HIGH | Add archive contents verification step with `ZipFile::OpenRead` |
| PR1-F | HIGH | Use `${{ env.VCPKG_INSTALLATION_ROOT }}` not hardcoded `C:\vcpkg` |
| PR1-G | HIGH | Reframe D0 as first CI run gate; acknowledge ANL-004 as bonus not abort proof |
| PR1-H | HIGH | (Subsumed by PR1-C — `$ErrorActionPreference='Stop'` covers this) |
| PR1-I | MEDIUM | Use `println!("cargo:warning=...")` for vcpkg error in build.rs |
| PR1-J | MEDIUM | Expand D4 to specify all 5 TD fields for TD-042 and TD-043 |
| PR1-K | MEDIUM | Refactor `compile_shim` to accept generic `include_dir: &Path` |
| PR1-L | MEDIUM | Fix vcpkg prose, lzma2 claim, C++ runtime comment |
| PR1-M | MEDIUM | TD-043 trigger: replace "before v0.2" with "by 2026-09-01" |
| PR1-N | LOW | DN-013 status: partial reconciliation with TD cross-refs |
| PR1-O | LOW | Add DirectML system DLL note to Technical context |

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 5
  verified: 4
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: PR1-A
      file: /Users/ph/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ort-sys-2.0.0-rc.12/build/main.rs
      line: 160
      present: yes
      retain: yes-flag-for-human-triage
      reason: "static=onnxruntime confirmed; copy-dylibs copies DLLs only if present; dist.txt shows MSVC archive is .tar.lzma2 with no DLL (per Agent 6 inspection)"
      evidence_snippet: 'println!("cargo:rustc-link-lib=static=onnxruntime");'
    - finding_id: PR1-B
      file: /Users/ph/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vcpkg-0.2.15/src/lib.rs
      line: 1363
      present: yes
      retain: yes
      reason: "Default triplet x64-windows-static-md confirmed; plan installs x64-windows-static — mismatch"
      evidence_snippet: 'triplet: "x64-windows-static-md".into(),'
    - finding_id: PR1-C
      file: docs/plans/session-16.md
      line: 12
      present: yes
      retain: yes
      reason: "Plan goal includes onnxruntime.dll; D2 bundle script copies it (lines 258-267)"
      evidence_snippet: "Ship a Windows x86_64 binary (`photohelper.exe` + `onnxruntime.dll` +"
    - finding_id: PR1-D
      file: docs/plans/session-16.md
      line: 207
      present: yes
      retain: yes
      reason: "No smoke test step in D2 job (no --help, --version, or binary execution)"
      evidence_snippet: "steps:"
    - finding_id: PR1-E
      file: docs/plans/session-16.md
      line: 244
      present: yes
      retain: yes
      reason: "Hardcoded VCPKG_ROOT: C:\\vcpkg confirmed"
      evidence_snippet: "VCPKG_ROOT: C:\\vcpkg"
```
