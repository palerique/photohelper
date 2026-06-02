# Session 16 — Implementation review, Round 1

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

**Scope**: All code written in session 16 — `crates/photohelper-raw/build.rs`, `crates/photohelper-raw/Cargo.toml`, `.github/workflows/release.yml`, `scripts/README-install-windows.md`, `TECH-DEBT.md`, `docs/discovery-notes.md`, `docs/analysis/ANL-004-windows-release-preflight.md`.

9th-agent verification: 5/5 verified (0 hallucinated). discard_rate: 0.00.

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 0 | — |
| HIGH | 4 | IR1-A, IR1-B, IR1-C, IR1-D |
| MEDIUM | 5 | IR1-E, IR1-F, IR1-G, IR1-H, IR1-I |
| LOW | 2 | IR1-J, IR1-K |

---

## Theme IR1-A — `compile_shim` doc comment claims MSVC performs multi-pass symbol resolution (HIGH)

**Severity: HIGH** — Agents 6 (CRITICAL) + 8 (MEDIUM)

`build.rs:181`:
```
/// The MSVC linker performs multi-pass symbol resolution, so the ordering
```

This is factually wrong. `link.exe` processes `.lib` files in a single pass (like GNU `ld`). The real reason the ordering of `cargo:rustc-link-lib` directives is safe is that Cargo collects all directives emitted during `build.rs` execution before invoking the linker — the call order within `build.rs` does not map directly to linker command-line ordering. Additionally, `compile_shim` is a private function — its `///` doc comment will never appear in rustdoc output.

**Verified**: present=yes. Line 181: `"/// The MSVC linker performs multi-pass symbol resolution..."`.

**Remediation**: Convert `///` to `//` (private fn, no rustdoc value) and replace the incorrect linker claim with an accurate explanation of why ordering is safe.

---

## Theme IR1-B — `release.yml` header still says "Windows support deferred to v0.2 (TD-029)" (HIGH)

**Severity: HIGH** — Agents 1+6

`release.yml:16`: `#                  Windows support deferred to v0.2 (TD-029).`

TD-029 is CLOSED. The `build-windows-x86_64` job is implemented 144 lines below. A contributor reading the header would believe Windows is unsupported. Also misleading: the original gap was windows-gnu toolchain, but the implementation uses windows-msvc.

**Verified**: present=yes. Line 16 contains the exact stale claim.

**Remediation**: Update lines 15–16 to describe the MSVC static-link architecture.

---

## Theme IR1-C — TD-043 has no in-source label in `ffi.rs` (HIGH)

**Severity: HIGH** — Agent 1

CLAUDE.md requires: "The stop-gap MUST be labeled in-source. A comment at the stop-gap site cites the `TD-N` identifier." TECH-DEBT.md TD-043 cites `crates/photohelper-raw/src/ffi.rs` (CString::new path conversions) but `grep -n "TD-043" ffi.rs` returns zero matches.

**Verified**: present=yes (absence confirmed).

**Remediation**: Add `// TD-043 (stop-gap): no \\?\ prefix for Windows long paths` at the primary `CString::new(path_str)` call site in `ffi.rs`.

---

## Theme IR1-D — Zip file handle not protected by `try/finally` (HIGH)

**Severity: HIGH** — Agent 5

`release.yml:254-256` (Verify step):
```powershell
$zip = [IO.Compression.ZipFile]::OpenRead($zipPath)
$entries = $zip.Entries | Select-Object -ExpandProperty FullName
$zip.Dispose()
```

If `$zip.Entries | Select-Object` throws (corrupted zip, I/O error), `$zip.Dispose()` is never called with `$ErrorActionPreference='Stop'`. The file handle leaks until the PowerShell process exits. While GHA exits the process anyway, the pattern is incorrect and will fail if a cleanup step is ever added.

**Verified**: present=yes. `$zip.Dispose()` at line 256, assertions at lines 260–266.

**Remediation**: Wrap in `try { ... } finally { $zip.Dispose() }`.

---

## Theme IR1-E — Missing `cargo:rerun-if-env-changed` for vcpkg env vars (MEDIUM)

**Severity: MEDIUM** — Agent 2

`build.rs` emits `cargo:rerun-if-changed` for the tarball and build.rs itself but no `cargo:rerun-if-env-changed` for `VCPKG_ROOT`, `VCPKGRS_TRIPLET`, or `VCPKGRS_DYNAMIC`. If a developer changes `VCPKG_ROOT`, Cargo won't re-run the build script, potentially linking against the wrong vcpkg installation silently.

**Verified**: present=yes (absence confirmed). Only tarball/sha/build.rs rerun-if-changed directives exist.

**Remediation**: Add in `main()`:
```rust
println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
println!("cargo:rerun-if-env-changed=VCPKGRS_TRIPLET");
println!("cargo:rerun-if-env-changed=VCPKGRS_DYNAMIC");
println!("cargo:rerun-if-env-changed=VCPKGRS_DISABLE");
```

---

## Theme IR1-F — `compile_shim` lacks header existence validation (MEDIUM)

**Severity: MEDIUM** — Agent 4

`compile_shim` passes `include_dir` directly to `cc::Build::include()` without verifying `include_dir.join("libraw/libraw.h")` exists. If vcpkg returns a path to a non-existent or header-less include directory, the error is a raw compiler diagnostic ("libraw/libraw.h: No such file or directory") with no indication of which path was expected or how to fix it.

**Remediation**: Add a precondition check at the top of `compile_shim`:
```rust
let expected_header = include_dir.join("libraw").join("libraw.h");
if !expected_header.exists() {
    return Err(format!(
        "LibRaw header not found at {}. \
         Windows: re-run `vcpkg install libraw:x64-windows-static-md`. \
         Unix: delete the OUT_DIR and rebuild to re-extract the tarball.",
        expected_header.display()
    ));
}
```

---

## Theme IR1-G — Smoke test uses `--help` (clap-only, no app logic) (MEDIUM)

**Severity: MEDIUM** — Agent 5

`release.yml:214`: `& "target/${{ env.TARGET }}/release/photohelper.exe" --help`

`--help` is handled by clap before any application initialization. A binary with wrong ORT linkage or missing runtime dependencies would still exit 0. This proves the binary loads but not that it functions.

**Remediation**: Change to `--version` (documents intent more clearly) or add a `ingest --help` invocation that exercises at least the subcommand parser. Note: full functional test is out of plan scope (deferred to Windows CI gate in future).

---

## Theme IR1-H — Citing internal crate line number in production code comment (MEDIUM)

**Severity: MEDIUM** — Agent 6

`build.rs:133-134`: `(confirmed: vcpkg-0.2.15/src/lib.rs:35)` — cites an internal source line that will rot on any patch version bump.

**Remediation**: Replace with a reference to the crate's public documentation: `(see vcpkg crate docs: default triplet; overridable via VCPKGRS_TRIPLET)`.

---

## Theme IR1-I — TD-042 in-source label format mismatch (MEDIUM)

**Severity: MEDIUM** — Agent 1

`TECH-DEBT.md:510` cites `` `// TD-042 (stop-gap)` `` but the actual code at `build.rs:144` uses `/// # TD-042 (stop-gap)` (a doc-comment heading). The citation format is inaccurate.

**Remediation**: Update TECH-DEBT.md to cite the actual format: `` `/// # TD-042 (stop-gap)` ``.

---

## Theme IR1-J — No Windows CI gate on PRs (LOW — defer to TD)

**Severity: LOW** — Agent 7. Out of plan scope.

The Windows MSVC build is only exercised in `release.yml` (on tag push), not in `ci.yml` (on PR). A future PR could break the Windows path invisibly. Adding a Windows CI job to `ci.yml` is explicitly deferred (plan Out of Scope table). File as TD.

---

## Theme IR1-K — `GITHUB_ENV` guard inconsistency (LOW)

**Severity: LOW** — Agent 5. `Add-Content -Path $env:GITHUB_ENV` lacks a null guard matching the `VCPKG_INSTALLATION_ROOT` pattern. Low risk (GHA always sets `GITHUB_ENV`).

---

## Disposition summary

| Theme | Severity | Fix |
|---|---|---|
| IR1-A | HIGH | Fix compile_shim comment: remove multi-pass claim, convert to // |
| IR1-B | HIGH | Update release.yml header lines 15-16 |
| IR1-C | HIGH | Add TD-043 in-source label to ffi.rs |
| IR1-D | HIGH | Wrap $zip in try/finally in release.yml |
| IR1-E | MEDIUM | Add rerun-if-env-changed directives to build.rs main() |
| IR1-F | MEDIUM | Add header existence check to compile_shim |
| IR1-G | MEDIUM | Change --help to --version in smoke test |
| IR1-H | MEDIUM | Fix fragile internal crate line citation |
| IR1-I | MEDIUM | Fix TD-042 label citation format in TECH-DEBT.md |
| IR1-J | LOW | File as TD (Windows PR CI gate) |
| IR1-K | LOW | Defer (GITHUB_ENV always set on GHA) |

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 5
  verified: 5
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: IR1-A
      file: crates/photohelper-raw/build.rs
      line: 181
      present: yes
      retain: yes
      reason: MSVC multi-pass claim confirmed at line 181
      evidence_snippet: "/// The MSVC linker performs multi-pass symbol resolution, so the ordering"
    - finding_id: IR1-B
      file: .github/workflows/release.yml
      line: 16
      present: yes
      retain: yes
      reason: "Windows support deferred to v0.2 (TD-029)" confirmed at line 16
      evidence_snippet: "#                  Windows support deferred to v0.2 (TD-029)."
    - finding_id: IR1-C
      file: crates/photohelper-raw/src/ffi.rs
      line: 0
      present: yes
      retain: yes
      reason: grep confirmed zero matches for TD-043 in ffi.rs
      evidence_snippet: ""
    - finding_id: IR1-D
      file: .github/workflows/release.yml
      line: 256
      present: yes
      retain: yes
      reason: $zip.Dispose() at line 256, assertions follow — no try/finally
      evidence_snippet: "$zip.Dispose()"
    - finding_id: IR1-E
      file: crates/photohelper-raw/build.rs
      line: 0
      present: yes
      retain: yes
      reason: No rerun-if-env-changed directives found
      evidence_snippet: ""
```
