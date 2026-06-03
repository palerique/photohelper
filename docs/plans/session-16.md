# Session 16 — `windows-release` — plan v3

> **Branch**: `session-16/windows-release`
> **Goal**: Produce a self-contained `x86_64-pc-windows-msvc` binary release archive
> and wire it into the existing GitHub Actions release pipeline, closing TD-029
> and DN-013.

---

## Goal

Ship a Windows x86_64 binary (`photohelper.exe` + `models/`) bundled as a `.zip`
archive from GitHub Actions. ORT is **statically linked** on MSVC (matching Linux
x86_64 per DN-041); no `onnxruntime.dll` is needed in the archive.

**By end of session:**
- A `build-windows-x86_64` job exists in `.github/workflows/release.yml`.
- The job succeeds on `windows-latest` with target `x86_64-pc-windows-msvc`.
- The release job's `needs` array includes the Windows job.
- `build.rs` in `photohelper-raw` has a Windows MSVC branch that uses
  vcpkg-installed LibRaw instead of autoconf, with a refactored `compile_shim`.
- TD-029 is closed; DN-013 is partially reconciled (item (a) addressed; items
  (b)+(c) tracked as TD-042 + TD-043).
- `just ci` stays GREEN on macOS (no regressions to existing builds).

---

## Why this scope

TD-029 (filed 2026-06-02 in session 15) documented: (a) no `x86_64-pc-windows-gnu`
ORT prebuilt on pyke CDN; (b) MSVC is the only supported Windows target. Option A
of TD-029 is chosen: `x86_64-pc-windows-msvc` + vcpkg for LibRaw.

DN-013 (filed 2026-05-28, session 02) deferred Windows LibRaw cross-compile audit
to v0.2. This session performs that audit (D0 → confirmed in first CI run) and
partially reconciles it; the two remaining audit items (static-link verification,
long-path) are filed as TDs.

---

## Out of scope (explicit deferrals)

| Deferred item | Reason | Where tracked |
|---|---|---|
| Windows ARM (aarch64-pc-windows-msvc) | GHA has no arm64 Windows runner | Future TD |
| `cargo test` on Windows in CI | Integration tests require Windows runner; adds 10+ min per CI run | Future TD |
| Windows Store / MSIX packaging | Not requested; no binding trigger | Backlog |
| Virtual copy + XMP crop (DN-042) | Separate feature, different scope | Session 17 |
| `cc::Build` native Windows LibRaw (no vcpkg) | ~200 extra LoC, low priority vs. vcpkg | TD-042 (filed in D4) |
| Windows PATH >260 chars (`\\?\` prefix) | DN-013 audit item; separate FFI change | TD-043 (filed in D4) |
| LibRaw autoconf path on Windows | autoconf not available without MSYS2 on MSVC | Replaced by vcpkg |
| Static-link verification via `dumpbin` | DN-013 audit item (b) | TD-042 |

---

## Technical context

### ORT on Windows MSVC

`ort` v2.0.0-rc.12 with `download-binaries` + `copy-dylibs` features:
- Pyke CDN hosts an `x86_64-pc-windows-msvc` ORT prebuilt (confirmed via DN-041).
- `ort-sys` handles LZMA2 archive extraction internally via the `lzma-rust2`
  pure-Rust crate — no Python or external tools needed for any target.
- `ort-sys/build/main.rs:160` emits `cargo:rustc-link-lib=static=onnxruntime` for
  **all** platforms including MSVC. ORT is **statically linked**; no `onnxruntime.dll`
  is present in the Pyke MSVC archive or needed in the release archive.
- `copy-dylibs` only copies `.dll` files if they exist in the extracted archive;
  this feature is a no-op for the MSVC static prebuilt.
- On Windows, ort-sys also links against system DirectX DLLs (`dxguid`, `DXCORE`,
  `DXGI`, `D3D12`, `DirectML`). These ship with Windows 10+ and do not require
  bundling — the Windows SDK on `windows-latest` provides them at link time.

### LibRaw on Windows MSVC

`build.rs` currently uses `sh ./configure && make` (autoconf). On MSVC:
- `sh` / autoconf are not on the MSVC PATH; MSYS2 is available but not the target.
- Solution: vcpkg `libraw:x64-windows-static-md`. GitHub Actions `windows-latest`
  has vcpkg pre-installed; `VCPKG_INSTALLATION_ROOT` provides the canonical path.
- The Rust `vcpkg` crate (v0.2) scans the `VCPKG_ROOT` filesystem directly (reads
  `installed/<triplet>/lib` and `installed/<triplet>/include`); it does NOT invoke
  `vcpkg.exe`.
- The Rust vcpkg crate defaults to the `x64-windows-static-md` triplet for
  `x86_64-pc-windows-msvc` (confirmed at `vcpkg-0.2.15/src/lib.rs:1363`). The
  `-md` suffix means statically-linked library + DLL-linked CRT (UCRT, which ships
  with Windows 10+). This is correct — no CRT DLLs need bundling.
- `vcpkg install libraw:x64-windows-static-md` installs LibRaw + transitive deps
  (zlib). The vcpkg crate emits `cargo:rustc-link-*` directives for all of them.

### Our C shim on Windows MSVC

`cpp/photohelper_libraw_shim.c` includes `<libraw/libraw.h>`. The vcpkg-installed
headers are at `%VCPKG_ROOT%\installed\x64-windows-static-md\include\`. After the
plan-review finding, `compile_shim` is refactored to accept a generic
`include_dir: &Path` so both the Unix and Windows paths call one function.

### C++ and zlib link directives (Windows path)

On MSVC, the C++ runtime is handled automatically by the `cl.exe` / `link.exe`
toolchain — no `cargo:rustc-link-lib` emit needed (unlike Unix where we emit
`c++` or `stdc++`). zlib is pulled in as a transitive vcpkg dependency and its
`cargo:rustc-link-lib` directive is emitted by the vcpkg crate automatically.
Both explicit link directives in `run()` must be **skipped** in the Windows path.

---

## Deliverables

### D0 — Pre-flight audit: MSVC ORT + vcpkg LibRaw viability

**Commit**: `chore(windows): pre-flight MSVC ORT + vcpkg LibRaw audit (ANL-004)`

D0 is a documentation-plus-CI gate. The concrete ABORT signal is the first CI run
of D2's `build-windows-x86_64` job — if it fails, that is the abort. D0 produces
`docs/analysis/ANL-004-windows-release-preflight.md` recording the decisions made
from source inspection before implementation.

**Pre-implementation source inspection** (not a blocking gate on its own):
1. Inspect `~/.cargo/registry/src/.../ort-sys-2.0.0-rc.12/build/main.rs` to
   confirm static linking model for MSVC. Record the linking model in ANL-004.
2. Inspect `~/.cargo/registry/src/.../vcpkg-0.2.15/src/lib.rs` triplet-selection
   logic to confirm `x64-windows-static-md` is the correct triplet.
3. Document in ANL-004: ORT is static on MSVC (no DLL bundling needed); vcpkg
   triplet is `x64-windows-static-md`; DirectML system deps listed.

**Acceptance**: `ANL-004` committed. The real gate is D2's CI run — a first-run
failure triggers manual re-evaluation before D3/D4.

---

### D1 — `build.rs` Windows MSVC branch (with `compile_shim` refactor)

**Commit**: `feat(raw): build.rs Windows MSVC path via vcpkg + cc shim`

**Changes to `crates/photohelper-raw/Cargo.toml`**:
```toml
[build-dependencies]
sha2.workspace = true
cc.workspace = true
# vcpkg: used only for Windows MSVC LibRaw path; compiles on all platforms (pure Rust).
vcpkg = "0.2"
```

**Changes to `crates/photohelper-raw/build.rs`**:

```rust
fn run() -> Result<(), String> {
    // TARGET is always set by Cargo in build scripts.
    let target = env::var("TARGET").unwrap_or_default();
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|_| "CARGO_MANIFEST_DIR not set")?);

    if target.contains("windows-msvc") {
        return run_windows_msvc(&manifest_dir);
    }

    // ── Unix path (existing autoconf — unchanged) ─────────────────────────
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|_| "OUT_DIR not set")?);
    let tarball = manifest_dir.join(TARBALL_REL_PATH);
    verify_tarball_sha256(&tarball)?;
    let src_dir = out_dir.join(EXTRACTED_DIR_NAME);
    if !src_dir.exists() { extract_tarball(&tarball, &out_dir)?; }
    let static_lib = src_dir.join("lib").join(".libs").join("libraw.a");
    if !static_lib.exists() {
        ensure_pkg_config()?;
        run_configure(&src_dir)?;
        run_make(&src_dir)?;
        if !static_lib.exists() { return Err(format!(
            "after `make`, expected static library not found at {}",
            static_lib.display())); }
    }
    println!("cargo:rustc-link-search=native={}", src_dir.join("lib").join(".libs").display());
    println!("cargo:rustc-link-lib=static=raw");
    // Unified shim compilation — unix path uses the extracted tarball as include root.
    compile_shim(&manifest_dir, &src_dir)?;
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
    println!("cargo:rustc-link-lib=z");
    Ok(())
}

fn run_windows_msvc(manifest_dir: &Path) -> Result<(), String> {
    // Use vcpkg-installed LibRaw (x64-windows-static-md triplet — the Rust vcpkg
    // crate's default for x86_64-pc-windows-msvc without +crt-static).
    // The crate scans VCPKG_ROOT/installed/<triplet>/ directly; does not call vcpkg.exe.
    let lib = vcpkg::find_package("libraw")
        .map_err(|e| {
            // Always-visible warning (cargo:warning lines are shown even without -vv).
            println!(
                "cargo:warning=vcpkg could not find `libraw`. \
                 Install: `vcpkg install libraw:x64-windows-static-md` \
                 and set VCPKG_ROOT to the vcpkg installation root."
            );
            format!("vcpkg find libraw: {e}")
        })?;
    // vcpkg crate emits cargo:rustc-link-lib for libraw + transitive deps (zlib).
    // C++ runtime: cl.exe / link.exe handle it implicitly — no explicit emit needed.

    let include_dir = lib.include_paths.first()
        .ok_or_else(|| {
            "vcpkg found `libraw` but returned no include paths. \
             The vcpkg installation may be incomplete. \
             Try: `vcpkg remove libraw:x64-windows-static-md && \
             vcpkg install libraw:x64-windows-static-md`".to_string()
        })?;

    // Unified shim compilation — Windows path uses the vcpkg include dir.
    // cc::Build selects cl.exe on MSVC automatically.
    compile_shim(manifest_dir, include_dir)?;
    Ok(())
}

/// Compile the C ABI shim against LibRaw headers from `include_dir`.
///
/// Called from both Unix (passing the extracted tarball root) and Windows
/// (passing the vcpkg installed include directory). The shim source only
/// includes `<libraw/libraw.h>`, which is present in both locations.
fn compile_shim(manifest_dir: &Path, include_dir: &Path) -> Result<(), String> {
    let shim_src = manifest_dir.join("cpp").join("photohelper_libraw_shim.c");
    println!("cargo:rerun-if-changed={}", shim_src.display());
    cc::Build::new()
        .file(&shim_src)
        .include(include_dir)
        .warnings(false)
        .try_compile("photohelper_libraw_shim")
        .map_err(|e| format!("cc::Build for shim failed: {e}"))
}
```

**Key design notes:**
- `compile_shim` is refactored from `compile_shim(manifest_dir, libraw_src_dir)` to
  `compile_shim(manifest_dir, include_dir)`. The Unix caller passes `&src_dir` (the
  extracted tarball root, which has `libraw/` headers). The Windows caller passes the
  vcpkg include directory. The MSVC linker resolves symbol order automatically
  (multi-pass), so the vcpkg-emit-before-shim ordering is safe.
- `out_dir` is declared AFTER the Windows early return to avoid an `unused_variable`
  clippy warning on Windows builds.

**Testing**:
- `just ci` must stay GREEN on macOS (the unix path is unchanged; `compile_shim`
  signature change is backward-compatible).
- The vcpkg crate compiles on macOS/Linux (pure Rust; no Windows APIs at compile
  time). Confirmed: `vcpkg 0.2` only invokes filesystem scanning at runtime when
  `find_package` is called.
- The Windows path is tested only via CI (no local cross-compile to MSVC from macOS).

---

### D2 — `release.yml` Windows job

**Commit**: `ci(release): build-windows-x86_64 job (MSVC + vcpkg LibRaw, static ORT)`

**New job** (add after `build-linux-x86_64`):

```yaml
build-windows-x86_64:
  name: Build Windows x86_64
  runs-on: windows-latest
  env:
    TARGET: x86_64-pc-windows-msvc

  steps:
    - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5  # v4.3.1
      with:
        lfs: true

    - name: Set version from tag
      shell: pwsh
      run: |
        $ErrorActionPreference = 'Stop'
        $version = $env:GITHUB_REF_NAME -replace '^v', ''
        (Get-Content Cargo.toml) -replace '^version = "0.0.0"', "version = `"$version`"" |
          Set-Content Cargo.toml
        # Add-Content writes UTF-8 NoBOM (correct for GITHUB_ENV) on PowerShell 7.
        Add-Content -Path $env:GITHUB_ENV -Value "RELEASE_VERSION=$version"

    - uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9
      with:
        toolchain: ${{ env.RUST_TOOLCHAIN }}
        targets: x86_64-pc-windows-msvc

    - uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4
      with:
        key: release-windows-x86_64-v1

    - name: Install LibRaw via vcpkg
      shell: pwsh
      run: |
        $ErrorActionPreference = 'Stop'
        if (-not $env:VCPKG_INSTALLATION_ROOT) {
            throw "VCPKG_INSTALLATION_ROOT is not set. Expected on GHA windows-latest runners. " +
                  "If the runner image changed, file a runner-images issue."
        }
        # x64-windows-static-md: static lib + DLL-linked CRT (UCRT ships with Win10+).
        # Matches the Rust vcpkg crate's default triplet for x86_64-pc-windows-msvc.
        vcpkg install libraw:x64-windows-static-md

    - name: Build release binary
      shell: pwsh
      run: cargo build --release -p photohelper-cli --target ${{ env.TARGET }}
      env:
        # Use the GHA-provided canonical vcpkg location (robust across runner updates).
        VCPKG_ROOT: ${{ env.VCPKG_INSTALLATION_ROOT }}

    - name: Smoke test binary
      shell: pwsh
      run: |
        $ErrorActionPreference = 'Stop'
        $env:PHOTOHELPER_MODEL_DIR = "crates/photohelper-ai/models"
        & "target/${{ env.TARGET }}/release/photohelper.exe" --help

    - name: Bundle archive
      shell: pwsh
      run: |
        $ErrorActionPreference = 'Stop'
        $VERSION = $env:RELEASE_VERSION
        $TARGET = "${{ env.TARGET }}"
        $DIR = "photohelper-${VERSION}-${TARGET}"
        New-Item -ItemType Directory -Path "$DIR/models" -Force

        # Binary (ORT is statically linked — no onnxruntime.dll needed)
        Copy-Item "target/${TARGET}/release/photohelper.exe" "$DIR/"

        # Models — use Get-ChildItem for explicit count validation
        Copy-Item "crates/photohelper-ai/models/manifest.toml" "$DIR/models/"
        $onnxFiles = Get-ChildItem "crates/photohelper-ai/models/*.onnx"
        if ($onnxFiles.Count -eq 0) {
            throw "No .onnx model files found — verify `lfs: true` in checkout step"
        }
        $onnxFiles | Copy-Item -Destination "$DIR/models/"

        # Install README
        Copy-Item "scripts/README-install-windows.md" "$DIR/README-install.md"

        # Create zip (Windows convention; PowerShell Compress-Archive is built-in)
        Compress-Archive -Path "$DIR" -DestinationPath "${DIR}.zip"
        $hash = (Get-FileHash -Algorithm SHA256 "${DIR}.zip").Hash.ToLower()
        "${hash}  ${DIR}.zip" | Set-Content "${DIR}.zip.sha256"

    - name: Verify and log archive contents
      shell: pwsh
      run: |
        $ErrorActionPreference = 'Stop'
        Add-Type -Assembly System.IO.Compression.FileSystem
        # Reconstruct deterministic path (Resolve-Path wildcard is ambiguous across steps).
        $VERSION = $env:RELEASE_VERSION
        $TARGET = "${{ env.TARGET }}"
        $zipPath = "photohelper-${VERSION}-${TARGET}.zip"
        if (-not (Test-Path $zipPath)) { throw "Expected archive not found: $zipPath" }
        $zip = [IO.Compression.ZipFile]::OpenRead($zipPath)
        $entries = $zip.Entries | Select-Object -ExpandProperty FullName
        $zip.Dispose()
        Write-Output "Archive contents:"
        $entries | ForEach-Object { Write-Output "  $_" }
        # Assert required files
        $required = @("photohelper.exe", "models/manifest.toml", "README-install.md")
        foreach ($f in $required) {
            $found = $entries | Where-Object { $_ -like "*/$f" -or $_ -eq $f }
            if (-not $found) { throw "Required file missing from archive: $f" }
        }
        $onnxCount = ($entries | Where-Object { $_ -like "*.onnx" }).Count
        if ($onnxCount -eq 0) { throw "No .onnx model files in archive" }
        $sizeMB = [math]::Round((Get-Item $zipPath).Length / 1MB, 1)
        Write-Output "Archive verified: ${onnxCount} ONNX model(s), ${sizeMB}MB total"

    - uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a  # HEAD
      with:
        name: windows-x86_64
        path: photohelper-*.zip*
```

**Update** the `release` job's `needs` array:
```yaml
needs: [build-macos-arm64, build-linux-x86_64, build-windows-x86_64]
```

**Testing**: First CI run of this job on `windows-latest` is the D0 executable gate.
If the job fails, it triggers manual re-evaluation before D3/D4 proceed.

---

### D3 — `README-install-windows.md` update

**Commit**: `docs(windows): update README-install-windows.md for MSVC static binary`

**Changes**:
- Replace `x86_64-pc-windows-gnu` with `x86_64-pc-windows-msvc` throughout
  (lines 13–14 of the current file: `Expand-Archive` filename and `cd` path).
- Remove all `onnxruntime.dll` references (lines 18, 54): ORT is statically linked;
  no DLL is included in or needed from the archive.
- Remove "MinGW-compiled binary" note (line 60); replace with "MSVC-compiled native
  Windows binary; statically links ORT (no onnxruntime.dll required)".
- Remove from the PowerShell install command: `Copy-Item photohelper.exe, onnxruntime.dll`.
  Replace with: `Copy-Item photohelper.exe`.

---

### D4 — Ledger: close TD-029 + DN-013; file TD-042 + TD-043

**Commit**: `docs(session-16): close TD-029 + DN-013; file TD-042 + TD-043`

**TECH-DEBT.md**: Mark TD-029 Closed with closure note:
> "Closed 2026-06-02 by session-16 (PR #17). ORT is statically linked on MSVC;
> Python was not needed (ort-sys uses lzma-rust2 internally). LibRaw via vcpkg
> x64-windows-static-md. Archive: photohelper.exe + models/ only."

**docs/discovery-notes.md**: Mark DN-013 Reconciled with partial note:
> "Reconciled 2026-06-02 (session 16, item (a) — LibRaw builds via vcpkg on
> windows-latest; CI green). Items (b) and (c) deferred as TD-042 and TD-043."
> Also add addendum to DN-041: "Addendum 2026-06-02: The MSVC prebuilt is a
> STATIC archive (libonnxruntime.a via LZMA2 tar); DN-041's original claim that
> it is a DLL requiring bundling was incorrect."

**TECH-DEBT.md**: File TD-042 with all 5 required fields:
- **Status**: Open
- **Opened**: 2026-06-02 (session 16, D1 vcpkg stop-gap)
- **Stop-gap location**: `crates/photohelper-raw/build.rs::run_windows_msvc` (new
  function added in session-16 commit); commit SHA: filled at commit time
- **Fundamental fix**: Remove vcpkg dependency entirely. Enumerate LibRaw 0.22.1
  source files in `build.rs` (lib/libraw_c_api.cpp, lib/libraw_cxx.cpp,
  lib/libraw_datastream.cpp) and compile them via `cc::Build` directly, with
  Windows-specific defines (`LIBRAW_NODLL`, `WIN32`, `/EHsc`). This eliminates
  the `VCPKG_ROOT` requirement for local Windows development.
- **Binding trigger**: First contributor issue reporting "vcpkg not found" OR first
  session that modifies `run_windows_msvc` for any other reason.
- **Scope estimate**: ~80 LoC (source file enumeration + Windows defines + test) /
  medium risk (needs verification on actual Windows hardware or CI).
- **Consequence of inaction**: Windows developers must install vcpkg and set
  `VCPKG_ROOT` to build from source; this is a non-standard Rust build requirement
  that will surprise contributors.

**TECH-DEBT.md**: File TD-043 with all 5 required fields:
- **Status**: Open
- **Opened**: 2026-06-02 (session 16, from DN-013 audit item (c))
- **Stop-gap location**: `crates/photohelper-raw/src/ffi.rs` — all path-passing
  functions use standard `CString::new(path.to_str()?)`, which silently fails on
  Windows paths > 260 chars without the `\\?\` prefix; commit SHA: filled at commit time
- **Fundamental fix**: Add a `windows_long_path(p: &Path) -> Result<CString, ...>`
  helper that prepends `\\?\` for paths > 260 chars on Windows targets. Apply to
  all `CString` conversions in `ffi.rs` that accept user-supplied paths.
- **Binding trigger**: First user report of a path-length error on Windows OR
  by 2026-09-01, whichever comes first.
- **Scope estimate**: ~30 LoC (helper + conditional application) / low risk.
- **Consequence of inaction**: Users with photos in deep directory trees (>260 chars
  total path) cannot ingest/process files on Windows; error is non-obvious.

---

## Checkpoints

| Checkpoint | Fires when |
|---|---|
| D0 ANL-004 commit | Before D1 code; documents linking model + triplet decisions |
| D2 first CI run | ABORT gate — if job fails, re-evaluate before D3/D4 |
| Sub-component review (build.rs boundary) | After D1 is implemented |
| Session-end review (R1 + R2) | After D1–D4 are complete |

---

## Acceptance criteria

| # | Criterion | How verified |
|---|---|---|
| 1 | `just ci` GREEN on macOS (no regressions) | Local `just ci` run |
| 2 | `build-windows-x86_64` GHA job exits 0 | CI run on `windows-latest` |
| 3 | Archive contains `photohelper.exe` | Verify and log step in D2 job |
| 4 | Archive contains `models/manifest.toml` + `*.onnx` | Verify and log step counts ONNX files |
| 5 | Smoke test (`photohelper.exe --help`) exits 0 | Smoke test step in D2 job |
| 6 | TD-029 marked Closed | TECH-DEBT.md |
| 7 | DN-013 marked Reconciled (partial, with TD cross-refs) | docs/discovery-notes.md |
| 8 | `README-install-windows.md` has no `onnxruntime.dll` or `windows-gnu` refs | Manual check |

---

## Stop-gaps declared

| Stop-gap | TD | Trigger |
|---|---|---|
| vcpkg path for LibRaw (VCPKG_ROOT required at build time for Windows) | TD-042 | First contributor issue "vcpkg not found" OR first session modifying `run_windows_msvc` |
| Windows long-path `\\?\` prefix for LibRaw path calls | TD-043 | First user report of path-length error on Windows OR by 2026-09-01 |
