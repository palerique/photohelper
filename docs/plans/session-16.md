# Session 16 — `windows-release` — plan v1

> **Branch**: `session-16/windows-release`
> **Goal**: Produce a self-contained `x86_64-pc-windows-msvc` binary release archive
> and wire it into the existing GitHub Actions release pipeline, closing TD-029
> and DN-013.

---

## Goal

Ship a Windows x86_64 binary (`photohelper.exe` + `onnxruntime.dll` +
`models/`) bundled as a `.zip` archive from GitHub Actions.

**By end of session:**
- A `build-windows-x86_64` job exists in `.github/workflows/release.yml`.
- The job succeeds on `windows-latest` with target `x86_64-pc-windows-msvc`.
- The release job's `needs` array includes the Windows job.
- `build.rs` in `photohelper-raw` has a Windows MSVC branch that uses
  vcpkg-installed LibRaw instead of autoconf.
- TD-029 is closed; DN-013 is reconciled.
- `just ci` stays GREEN on macOS (no regressions to existing builds).

---

## Why this scope

TD-029 (filed 2026-06-02 in session 15) documented the two investigation findings:
(a) no `x86_64-pc-windows-gnu` ORT prebuilt on pyke CDN; (b) MSVC is the only
supported Windows target. Option A of TD-029 is chosen: `x86_64-pc-windows-msvc`
+ vcpkg for LibRaw.

DN-013 (filed 2026-05-28, session 02) deferred Windows LibRaw cross-compile audit
to v0.2. This session performs that audit (D0) and closes it.

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

---

## Technical context

### ORT on Windows MSVC

`ort` v2.0.0-rc.12 with `download-binaries` + `copy-dylibs` features:
- Pyke CDN **does** host an `x86_64-pc-windows-msvc` ORT prebuilt.
- `ort-sys` handles download + extraction internally (pure Rust); no Python needed.
- `copy-dylibs` copies `onnxruntime.dll` to the cargo target directory
  (`target/x86_64-pc-windows-msvc/release/onnxruntime.dll`) automatically.
- D0 pre-flight must confirm this by inspecting the cargo build output.

### LibRaw on Windows MSVC

`build.rs` currently uses `sh ./configure && make` (autoconf). On MSVC:
- `sh` is available only if MSYS2 is installed; `windows-latest` has it in Git Bash
  but not on the MSVC PATH.
- Solution: vcpkg `libraw:x64-windows-static`. GitHub Actions `windows-latest`
  has vcpkg pre-installed at `C:\vcpkg`; `VCPKG_ROOT` is set by default.
- `vcpkg install libraw:x64-windows-static` installs LibRaw static lib + headers
  + transitive dependencies (zlib, libjpeg-turbo).
- Rust `vcpkg` build-crate (v0.2) calls `vcpkg.exe` and emits correct
  `cargo:rustc-link-*` directives.

### Our C shim on Windows MSVC

`cpp/photohelper_libraw_shim.c` includes `<libraw/libraw.h>`. On Windows, the
vcpkg-installed headers are at
`C:\vcpkg\installed\x64-windows-static\include\libraw\libraw.h`.
`cc::Build` with `.include(vcpkg_include_dir)` will compile the shim using
`cl.exe` (the MSVC C compiler) automatically.

### C++ and zlib link directives

The current `run()` tail emits `cargo:rustc-link-lib=c++` (Apple) or
`cargo:rustc-link-lib=stdc++` (other unix) and `cargo:rustc-link-lib=z`.
On MSVC:
- The C++ runtime is managed by the linker via `/MT` or `/MD` — no explicit emit.
- zlib is a transitive dependency of the vcpkg libraw package and is emitted by
  the vcpkg Rust crate automatically.
- Both explicit directives must be **skipped** in the Windows path.

---

## Deliverables

### D0 — Pre-flight audit: MSVC ORT + vcpkg LibRaw viability (ABORT gate)

**Commit**: `chore(windows): pre-flight MSVC ORT + vcpkg LibRaw audit (D0)`

**Steps**:
1. Read ort-sys 2.0.0-rc.12 source or docs to confirm:
   - `download-binaries` downloads `onnxruntime.dll` for MSVC target.
   - `copy-dylibs` copies it to the target release directory.
   - No Python / external extractor needed by ort-sys.
2. Verify vcpkg `libraw` port availability:
   - Run `vcpkg search libraw` or check vcpkg registry online.
   - Confirm `x64-windows-static` triplet is supported.
3. Document findings in `docs/analysis/ANL-004-windows-release-preflight.md`.
4. Decision gate: if ORT MSVC download is broken OR vcpkg libraw is absent →
   ABORT (surface alternative plan to user). Proceed only if both confirm.

**Acceptance**: `ANL-004` committed; no ABORT condition triggered.

---

### D1 — `build.rs` Windows MSVC branch

**Commit**: `feat(raw): build.rs Windows MSVC path via vcpkg + cc shim`

**Changes to `crates/photohelper-raw/Cargo.toml`**:
```toml
[build-dependencies]
sha2.workspace = true
cc.workspace = true
# vcpkg: used only for Windows MSVC LibRaw; platform-agnostic no-op on unix.
vcpkg = "0.2"
```

**Changes to `crates/photohelper-raw/build.rs`**:

```rust
fn run() -> Result<(), String> {
    let manifest_dir = ...;
    let out_dir = ...;
    let target = env::var("TARGET").unwrap_or_default();

    if target.contains("windows-msvc") {
        return run_windows_msvc(&manifest_dir);
    }

    // ── Unix path (existing autoconf) ──────────────────────────────────
    let tarball = manifest_dir.join(TARBALL_REL_PATH);
    verify_tarball_sha256(&tarball)?;
    let src_dir = out_dir.join(EXTRACTED_DIR_NAME);
    if !src_dir.exists() { extract_tarball(&tarball, &out_dir)?; }
    let static_lib = src_dir.join("lib").join(".libs").join("libraw.a");
    if !static_lib.exists() {
        ensure_pkg_config()?;
        run_configure(&src_dir)?;
        run_make(&src_dir)?;
        if !static_lib.exists() { return Err(...); }
    }
    println!("cargo:rustc-link-search=native={}", ...);
    println!("cargo:rustc-link-lib=static=raw");
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
    // Use vcpkg-installed LibRaw (static, x64-windows-static triplet).
    // vcpkg crate reads VCPKG_ROOT, finds libraw, emits cargo:rustc-link-* directives.
    let lib = vcpkg::find_package("libraw")
        .map_err(|e| format!("vcpkg find libraw: {e}. \
            Install: `vcpkg install libraw:x64-windows-static` \
            and set VCPKG_ROOT."))?;
    // Compile our C shim against the vcpkg-installed LibRaw headers.
    // cc::Build selects cl.exe on MSVC automatically.
    let include_dir = lib.include_paths.first()
        .ok_or("vcpkg returned no include paths for libraw")?;
    let shim_src = manifest_dir.join("cpp").join("photohelper_libraw_shim.c");
    println!("cargo:rerun-if-changed={}", shim_src.display());
    cc::Build::new()
        .file(&shim_src)
        .include(include_dir)
        .warnings(false)
        .try_compile("photohelper_libraw_shim")
        .map_err(|e| format!("cc::Build shim (MSVC): {e}"))?;
    // C++ runtime + zlib are pulled in as transitive vcpkg deps — no explicit emit needed.
    Ok(())
}
```

**Testing**:
- `just ci` must stay GREEN on macOS (the unix path is unchanged).
- The Windows path cannot be tested locally (cross-compile requires MSVC toolchain on
  the host); CI is the only test gate.

**Stop-gap**: This vcpkg path requires `VCPKG_ROOT` at build time. Local Windows
development needs MSYS2 or a manually installed vcpkg. A future native `cc::Build`
path (TD-042) would avoid the vcpkg dependency entirely.

---

### D2 — `release.yml` Windows job

**Commit**: `ci(release): build-windows-x86_64 job (MSVC + vcpkg LibRaw + ORT DLL)`

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
        $version = $env:GITHUB_REF_NAME -replace '^v', ''
        (Get-Content Cargo.toml) -replace '^version = "0.0.0"', "version = `"$version`"" |
          Set-Content Cargo.toml
        echo "RELEASE_VERSION=$version" >> $env:GITHUB_ENV

    - uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9
      with:
        toolchain: ${{ env.RUST_TOOLCHAIN }}
        targets: x86_64-pc-windows-msvc

    - uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4
      with:
        key: release-windows-x86_64-v1

    - name: Install LibRaw via vcpkg
      run: vcpkg install libraw:x64-windows-static
      env:
        VCPKG_DEFAULT_TRIPLET: x64-windows-static

    - name: Build release binary
      run: cargo build --release -p photohelper-cli --target ${{ env.TARGET }}
      env:
        VCPKG_ROOT: C:\vcpkg

    - name: Bundle archive
      shell: pwsh
      run: |
        $VERSION = $env:RELEASE_VERSION
        $TARGET = "${{ env.TARGET }}"
        $DIR = "photohelper-${VERSION}-${TARGET}"
        New-Item -ItemType Directory -Path "$DIR/models" -Force

        # Binary
        Copy-Item "target/${TARGET}/release/photohelper.exe" "$DIR/"

        # ORT DLL (copy-dylibs places it next to the binary)
        $dll = "target/${TARGET}/release/onnxruntime.dll"
        if (Test-Path $dll) {
            Copy-Item $dll "$DIR/"
        } else {
            # Fallback: search the ORT cache
            $cached = (Get-ChildItem -Recurse -Filter "onnxruntime.dll" `
                       "$HOME/.cache/ort.pyke.io" 2>$null | Select-Object -First 1).FullName
            if ($cached) { Copy-Item $cached "$DIR/" }
            else { throw "onnxruntime.dll not found; check ORT download step" }
        }

        # Models
        Copy-Item "crates/photohelper-ai/models/manifest.toml" "$DIR/models/"
        Copy-Item "crates/photohelper-ai/models/*.onnx"        "$DIR/models/"

        # Install README
        Copy-Item "scripts/README-install-windows.md" "$DIR/README-install.md"

        # Create zip (Windows-convention; PowerShell has Compress-Archive built-in)
        Compress-Archive -Path "$DIR" -DestinationPath "${DIR}.zip"
        $hash = (Get-FileHash -Algorithm SHA256 "${DIR}.zip").Hash.ToLower()
        "${hash}  ${DIR}.zip" | Set-Content "${DIR}.zip.sha256"

        Write-Output "Archive size: $((Get-Item "${DIR}.zip").Length / 1MB)MB"

    - uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
      with:
        name: windows-x86_64
        path: photohelper-*.zip*
```

**Update** the `release` job's `needs` array:
```yaml
needs: [build-macos-arm64, build-linux-x86_64, build-windows-x86_64]
```

**Testing**: GHA run on a `v*.*.*` tag (or workflow_dispatch for CI-only testing).
The job must exit 0; the archive must contain the four required files.

---

### D3 — `README-install-windows.md` update

**Commit**: `docs(windows): update README-install-windows.md for MSVC target`

**Changes**:
- Replace `x86_64-pc-windows-gnu` with `x86_64-pc-windows-msvc` throughout.
- Archive format: `.zip` (not `.tar.gz`).
- Note that `onnxruntime.dll` must live in the same directory as `photohelper.exe`
  (already present in the current README).
- Remove "MinGW-compiled binary" note; replace with "MSVC-compiled native Windows binary".

---

### D4 — Ledger: close TD-029 + DN-013; file TD-042 + TD-043

**Commit**: `docs(session-16): close TD-029 + DN-013; file TD-042 + TD-043`

- **TECH-DEBT.md**: Mark TD-029 Closed.
- **docs/discovery-notes.md**: Mark DN-013 Reconciled (2026-06-02).
- **TECH-DEBT.md**: File TD-042 — "vcpkg path is a stop-gap; native cc::Build LibRaw
  compilation for Windows MSVC eliminates the VCPKG_ROOT dependency."
- **TECH-DEBT.md**: File TD-043 — "Windows long-path (>260 chars) `\\?\` prefix for
  LibRaw `open_file_w`" (from DN-013's open audit item).

---

## Checkpoints

| Checkpoint | Fires when |
|---|---|
| D0 pre-flight ABORT gate | Before any code; if ORT/vcpkg blocker found, halt |
| Sub-component review (build.rs boundary) | After D1 is implemented |
| Session-end review (R1 + R2) | After D1–D4 are complete |

---

## Acceptance criteria

| # | Criterion | How verified |
|---|---|---|
| 1 | `just ci` GREEN on macOS (no regressions) | Local `just ci` run |
| 2 | `build-windows-x86_64` GHA job exits 0 | CI run on `windows-latest` |
| 3 | Archive contains `photohelper.exe` | Check zip contents in CI log |
| 4 | Archive contains `onnxruntime.dll` | Check zip contents in CI log |
| 5 | Archive contains `models/manifest.toml` + `*.onnx` | Check zip contents in CI log |
| 6 | TD-029 marked Closed | TECH-DEBT.md |
| 7 | DN-013 marked Reconciled | docs/discovery-notes.md |

---

## Stop-gaps declared

| Stop-gap | TD | Trigger |
|---|---|---|
| vcpkg path for LibRaw (VCPKG_ROOT required at build time for Windows) | TD-042 | First PR from a contributor hitting "vcpkg not found" OR before Windows native-build effort in v0.3 |
| Windows long-path `\\?\` prefix for LibRaw `open_file_w` | TD-043 | First user report of path-length error on Windows OR before v0.2 release |
