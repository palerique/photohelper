# Session 16 — Plan review, Round 2

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
    - feature-dev:code-reviewer
    - pr-review-toolkit:silent-failure-hunter
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

**Scope**: `docs/plans/session-16.md` (plan v2 post-R1 remediation). Cadence A tier-graduated — 3-agent suite appropriate for targeted R2 remediation verification.

R1 watch-list: All 15 items verified CLOSED.

3 new findings from v2 edits. 9th-agent verification not required (agents independently
confirmed findings from source inspection). discard_rate: 0.00.

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 1 | R2-1 |
| MEDIUM | 2 | R2-2, R2-3 |
| LOW | 1 | R2-4 (ruled out of scope) |

---

## R1 Watch-list: ALL 15 ITEMS CLOSED

| Theme | Verdict |
|---|---|
| PR1-A (ORT static) | CLOSED — all onnxruntime.dll refs removed |
| PR1-B (vcpkg triplet) | CLOSED — x64-windows-static-md throughout |
| PR1-C (ONNX glob) | CLOSED — $ErrorActionPreference='Stop' + Get-ChildItem count |
| PR1-D (smoke test) | CLOSED — photohelper.exe --help step present |
| PR1-E (archive verify) | CLOSED — ZipFile::OpenRead verification step |
| PR1-F (VCPKG_ROOT) | CLOSED — ${{ env.VCPKG_INSTALLATION_ROOT }} used |
| PR1-G (D0 gate) | CLOSED — reframed as first CI run |
| PR1-H (ErrorActionPreference) | CLOSED (subsumed by PR1-C) |
| PR1-I (cargo:warning) | CLOSED — println!("cargo:warning=...") present |
| PR1-J (TD fields) | CLOSED — all 5 fields present for TD-042 + TD-043 |
| PR1-K (compile_shim) | CLOSED — refactored to compile_shim(manifest_dir, include_dir) |
| PR1-L (prose) | CLOSED — vcpkg filesystem, lzma-rust2, MSVC CRT prose fixed |
| PR1-M (TD-043 trigger) | CLOSED — "by 2026-09-01" calendar date |
| PR1-N (DN-013 partial) | CLOSED — partial reconciliation noted |
| PR1-O (DirectML) | CLOSED — DirectML system deps documented |

---

## Theme R2-1 — `Resolve-Path "photohelper-*.zip"` in separate step where `$DIR` is out of scope (CRITICAL)

**Severity: CRITICAL** — 2 agents (general-purpose, silent-failure-hunter)

The "Verify and log archive contents" step is a separate `run:` block (separate PowerShell process). The variable `$DIR` set in the "Bundle archive" step is not available. The plan used `Resolve-Path "photohelper-*.zip"` as a workaround, but:

1. If multiple `.zip` files exist, `Resolve-Path` returns an array, and `[IO.Compression.ZipFile]::OpenRead($zipPath)` receives a `System.Object[]` and throws a misleading "argument count" error.
2. If zero files match (file named differently), throws "Cannot find path."

The correct approach is to reconstruct the deterministic path from environment variables available across steps.

**Remediation**: Replace `$zipPath = (Resolve-Path "photohelper-*.zip").Path` with:
```powershell
$VERSION = $env:RELEASE_VERSION
$TARGET = "${{ env.TARGET }}"
$zipPath = "photohelper-${VERSION}-${TARGET}.zip"
if (-not (Test-Path $zipPath)) { throw "Expected archive not found: $zipPath" }
```

---

## Theme R2-2 — `Out-File` without explicit encoding for `GITHUB_ENV` (MEDIUM)

**Severity: MEDIUM** — 1 agent (silent-failure-hunter)

`"RELEASE_VERSION=$version" | Out-File -FilePath $env:GITHUB_ENV -Append` uses `Out-File` without `-Encoding`. In PowerShell 7 (`pwsh` on GHA), the default is `utf8NoBOM` — correct. But if someone ports this to `shell: powershell` (PowerShell 5.1), the default is UTF-16 LE with BOM, which corrupts `GITHUB_ENV` silently. The version string becomes empty, naming the archive `photohelper--x86_64-pc-windows-msvc.zip`.

**Remediation**: Use `Add-Content` (simpler, deterministically UTF-8 NoBOM in PS7):
```powershell
Add-Content -Path $env:GITHUB_ENV -Value "RELEASE_VERSION=$version"
```

---

## Theme R2-3 — `VCPKG_INSTALLATION_ROOT` unguarded; empty value produces misleading error (MEDIUM)

**Severity: MEDIUM** — 1 agent (silent-failure-hunter)

`VCPKG_ROOT: ${{ env.VCPKG_INSTALLATION_ROOT }}` in the Build step. If `VCPKG_INSTALLATION_ROOT` is unset (runner image change), `VCPKG_ROOT=""` and the Rust vcpkg crate emits `Could not find Vcpkg tree` — correct symptom, wrong actionable message. A guard in the install step surfacing "VCPKG_INSTALLATION_ROOT is not set" is more diagnostic.

**Remediation**: Add at top of "Install LibRaw via vcpkg":
```powershell
if (-not $env:VCPKG_INSTALLATION_ROOT) {
    throw "VCPKG_INSTALLATION_ROOT is not set. Expected on GHA windows-latest runners."
}
```

---

## Theme R2-4 — macOS/Linux jobs lack explicit ONNX count check (LOW, out of scope)

Ruled out of scope for session 16. Windows job now has count check; macOS/Linux rely on `set -euo pipefail` which makes `cp path/*.onnx` fail with "No such file" when glob fails — safe enough for now. Future session should add parity.

---

## Disposition summary

| Theme | Severity | Fix |
|---|---|---|
| R2-1 | CRITICAL | Reconstruct deterministic path from $env:RELEASE_VERSION + target |
| R2-2 | MEDIUM | Replace Out-File with Add-Content |
| R2-3 | MEDIUM | Add VCPKG_INSTALLATION_ROOT guard in install step |
| R2-4 | LOW | Out of scope |
