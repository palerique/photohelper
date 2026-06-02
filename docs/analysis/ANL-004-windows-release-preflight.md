# ANL-004 — Windows x86_64 Release Pre-flight (2026-06-02, session 16)

## Purpose

Pre-implementation audit confirming the technical decisions for `x86_64-pc-windows-msvc`
release support. Findings locked the plan v3 design choices.

---

## Finding 1: ORT is statically linked on MSVC (no DLL bundling needed)

**Source inspected**: `~/.cargo/registry/src/.../ort-sys-2.0.0-rc.12/build/main.rs:160`

```rust
println!("cargo:rustc-link-lib=static=onnxruntime");
```

This line is emitted for all targets without a platform guard, including
`x86_64-pc-windows-msvc`. ORT is **statically** linked on all platforms when using
the `download-binaries` feature.

The `copy-dylibs` feature (main.rs line 157) copies `.dll`/`.so`/`.dylib` files from
the extracted archive if any exist. The `dist.txt` inspection below confirms no DLL is
present in the MSVC archive — `copy-dylibs` is a no-op for this target.

**Consequence for the release archive**: Only `photohelper.exe` + `models/` are needed.
No `onnxruntime.dll` should be included.

---

## Finding 2: MSVC ORT prebuilt archive format and URL

**Source inspected**: `~/.cargo/registry/src/.../ort-sys-2.0.0-rc.12/build/download/dist.txt`

The base (CPU-only, `none` execution provider) MSVC entry:
```
none  x86_64-pc-windows-msvc  https://cdn.pyke.io/0/pyke:ort-rs/ms@1.24.2/x86_64-pc-windows-msvc.tar.lzma2  b685bfc8d336e0ba95c066a7a982c03aa6dedd528a492eb99ca4ccb7f3af9e7a
```

- **Format**: `.tar.lzma2` (LZMA2-compressed tarball) — identical to all other platforms
- **URL**: cdn.pyke.io/0/pyke:ort-rs/ms@1.24.2/... — confirmed reachable
- **Extraction**: ort-sys uses `lzma-rust2` (pure-Rust crate, declared in ort-sys
  `Cargo.toml` as a build-dependency) to extract the archive internally. No Python,
  7-zip, or external tool required. The "Python needed for lzma2" claim in the original
  TD-029 referred to MSYS2 shell tools failing on the archive during the windows-gnu
  attempt — not to ort-sys's own extraction mechanism.
- **Contents**: Static library `libonnxruntime.a` (and platform-specific DirectML system
  libs linked via `build/static_link/mod.rs:55-60`). No `onnxruntime.dll`.

### DirectML system dependencies

`ort-sys/build/static_link/mod.rs` emits link directives for Windows targets:
```
dxguid, DXCORE, DXGI, D3D12, DirectML
```
These are Windows 10+ system DLLs — always present on the target OS, no bundling needed.
The `windows-latest` GHA runner has the Windows SDK which satisfies these at link time.

---

## Finding 3: vcpkg Rust crate default triplet for x86_64-pc-windows-msvc

**Source inspected**: `~/.cargo/registry/src/.../vcpkg-0.2.15/src/lib.rs:35`

```
//! The default 64-bit configuration is `x64-windows-static-md` which is a
```

The Rust `vcpkg` crate (v0.2.15) defaults to **`x64-windows-static-md`** for
`x86_64-pc-windows-msvc` builds without `CARGO_CFG_TARGET_FEATURE=crt-static` and
without `VCPKGRS_DYNAMIC=1`. The `-md` suffix means: statically-linked library +
DLL-linked CRT (UCRT). UCRT ships with Windows 10+ and is always available; no CRT
DLL needs bundling.

**Consequence**: `vcpkg install libraw:x64-windows-static-md` is the correct install
command (not `:x64-windows-static`, which is the C-runtime-static variant). The Rust
`vcpkg` crate will find the library at the correct triplet path automatically.

**Environment variable override**: `VCPKGRS_TRIPLET` allows overriding the default.
For session 16, the default is correct and no override is needed.

**Filesystem scanning**: The Rust `vcpkg` crate scans `$VCPKG_ROOT/installed/<triplet>/`
directly. It does NOT invoke `vcpkg.exe`. `VCPKG_ROOT` must be set; on GHA
`windows-latest`, `VCPKG_INSTALLATION_ROOT` provides the canonical path.

---

## Summary and Decision Record

| Question | Answer | Confidence |
|---|---|---|
| Is ORT statically linked on MSVC? | Yes (`cargo:rustc-link-lib=static=onnxruntime`) | Confirmed from source |
| Does the MSVC archive contain onnxruntime.dll? | No (`.tar.lzma2` with `.a` only) | Confirmed from dist.txt |
| Is Python needed for archive extraction? | No (ort-sys uses lzma-rust2 internally) | Confirmed from Cargo.toml |
| Correct vcpkg triplet for Rust crate? | `x64-windows-static-md` | Confirmed from lib.rs:35 |
| Does vcpkg crate call vcpkg.exe? | No (scans filesystem via VCPKG_ROOT) | Confirmed from docs |
| DirectML system DLLs need bundling? | No (Win10+ system DLLs) | Confirmed from static_link |

**D0 decision: PROCEED** — no abort conditions found.

The real gate is D2's first CI run on `windows-latest` (confirmed executable verification).
