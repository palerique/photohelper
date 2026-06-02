//! Build script: extract + autoconf-build the vendored LibRaw `=0.22.1`
//! tarball as a static library and emit the link directives.
//!
//! ## Platform strategy
//!
//! * **macOS / Linux** (Unix path): extracts the vendored tarball via `tar`,
//!   runs `sh ./configure && make`, and links the resulting `libraw.a`.
//! * **Windows MSVC** (`x86_64-pc-windows-msvc`): uses vcpkg-installed LibRaw
//!   (`x64-windows-static-md` triplet); skips tarball + autoconf entirely.
//!   Requires `VCPKG_ROOT` to be set at build time
//!   (`vcpkg install libraw:x64-windows-static-md`). See TD-042 for the
//!   future native `cc::Build` path that eliminates the vcpkg dependency.
//!
//! Plan-v3.2 chose `=0.22.1` after the Deliverable 0 pre-flight
//! (`docs/analysis/ANL-001-libraw-cr3-preflight.md`). The plan originally
//! prescribed `cmake` as the build driver, but LibRaw 0.22.1's tarball
//! ships ONLY the autoconf scripts (`configure`, `Makefile.am`); the
//! cmake build files live in a separate `LibRaw/LibRaw-cmake` repo
//! per the bundled `README.cmake`. Vendoring a second repo for cmake
//! rules would double the §6(a) tarball-shipping surface, so this
//! build.rs invokes the official autoconf path (`./configure && make`)
//! instead (on Unix), or vcpkg (on Windows MSVC).
//!
//! ## Unix system prerequisites
//!
//! * `pkg-config` (or `pkgconf` shim) on `PATH` — LibRaw's `configure`
//!   probes for it unconditionally.
//! * GNU make (or BSD make on macOS, which suffices for LibRaw's
//!   Makefile.am-generated Makefile).
//! * A C++ compiler — Xcode CLT on macOS (`xcode-select --install`),
//!   `build-essential` on Debian/Ubuntu, `gcc-c++` on Fedora.
//!
//! ## Windows MSVC prerequisites
//!
//! * vcpkg installed and `VCPKG_ROOT` set to its root directory.
//! * `vcpkg install libraw:x64-windows-static-md` run once.
//!
//! Missing tools fail the build with a `cargo:warning=` line naming
//! the exact package to install on each platform.

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the vendored tarball, relative to `CARGO_MANIFEST_DIR`.
const TARBALL_REL_PATH: &str = "vendor/libraw-0.22.1.tar.gz";

/// SHA-256 of `libraw-0.22.1.tar.gz` as downloaded from libraw.org on
/// 2026-05-28 during the Deliverable 0 pre-flight commit. Verified
/// against the upstream-released artifact at the same commit.
const EXPECTED_SHA256: &str = "a789dc4e2409e2901d93793a4e0b80c7b49d0d97cf6ad71c850eb7616acfd786";

/// Directory inside the tarball.
const EXTRACTED_DIR_NAME: &str = "LibRaw-0.22.1";

fn main() {
    // Re-run only when the tarball or this script changes.
    println!("cargo:rerun-if-changed=vendor/libraw-0.22.1.tar.gz");
    println!("cargo:rerun-if-changed=vendor/libraw-0.22.1.tar.gz.sha256");
    println!("cargo:rerun-if-changed=build.rs");
    // Re-run if vcpkg env vars change so the Windows MSVC path relinks correctly.
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
    println!("cargo:rerun-if-env-changed=VCPKGRS_TRIPLET");
    println!("cargo:rerun-if-env-changed=VCPKGRS_DYNAMIC");
    println!("cargo:rerun-if-env-changed=VCPKGRS_DISABLE");

    if let Err(e) = run() {
        eprintln!("photohelper-raw build.rs: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|_| "CARGO_MANIFEST_DIR not set")?);

    // Dispatch to the Windows MSVC path before touching out_dir (which is
    // only needed for tarball extraction on Unix). Avoids an unused-variable
    // warning on Windows builds.
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        return run_windows_msvc(&manifest_dir);
    }

    // ── Unix path (macOS + Linux): extract tarball, autoconf, make ────────
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|_| "OUT_DIR not set")?);

    let tarball = manifest_dir.join(TARBALL_REL_PATH);
    verify_tarball_sha256(&tarball)?;

    let src_dir = out_dir.join(EXTRACTED_DIR_NAME);
    if !src_dir.exists() {
        extract_tarball(&tarball, &out_dir)?;
    }

    // Cache: if the static lib already exists, skip configure + make.
    let static_lib = src_dir.join("lib").join(".libs").join("libraw.a");
    if !static_lib.exists() {
        ensure_pkg_config()?;
        run_configure(&src_dir)?;
        run_make(&src_dir)?;
        if !static_lib.exists() {
            return Err(format!(
                "after `make`, expected static library not found at {}",
                static_lib.display()
            ));
        }
    }

    // Tell the linker where to find libraw.a and to statically link it.
    println!(
        "cargo:rustc-link-search=native={}",
        src_dir.join("lib").join(".libs").display()
    );
    println!("cargo:rustc-link-lib=static=raw");

    // Compile our minimal C ABI shim against LibRaw's public headers.
    // The tarball root contains `libraw/libraw.h` at `src_dir/libraw/libraw.h`,
    // so passing `src_dir` as the include root resolves `<libraw/libraw.h>`.
    compile_shim(&manifest_dir, &src_dir)?;

    // LibRaw is C++; we need the C++ standard library at the final link.
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
    // LibRaw uses zlib (the configure-time default we accept).
    println!("cargo:rustc-link-lib=z");

    Ok(())
}

/// Windows MSVC build path: use vcpkg-installed LibRaw.
///
/// The Rust `vcpkg` crate defaults to the `x64-windows-static-md` triplet for
/// `x86_64-pc-windows-msvc` (see vcpkg crate docs; overridable via `VCPKGRS_TRIPLET`).
/// That triplet links libraries statically with DLL-linked CRT (UCRT, ships
/// with Windows 10+). `VCPKG_ROOT` must be set; on GHA `windows-latest`,
/// `VCPKG_INSTALLATION_ROOT` provides the canonical path.
///
/// The vcpkg crate scans `$VCPKG_ROOT/installed/<triplet>/` directly and emits
/// `cargo:rustc-link-lib` directives for LibRaw + transitive deps (zlib).
/// The C++ runtime is managed automatically by `cl.exe`/`link.exe` — no
/// explicit `cargo:rustc-link-lib=c++` emit is needed on MSVC.
///
/// # TD-042 (stop-gap)
///
/// This vcpkg path requires `VCPKG_ROOT` at build time; local Windows
/// development needs vcpkg installed. A future native `cc::Build` path
/// (TD-042) will compile LibRaw sources directly, eliminating the dependency.
fn run_windows_msvc(manifest_dir: &Path) -> Result<(), String> {
    let lib = vcpkg::find_package("libraw").map_err(|e| {
        // cargo:warning lines are always visible in cargo output (even without -vv).
        println!(
            "cargo:warning=vcpkg could not find `libraw`. \
             Install: `vcpkg install libraw:x64-windows-static-md` \
             and set VCPKG_ROOT to the vcpkg installation root."
        );
        format!("vcpkg find libraw: {e}")
    })?;

    // vcpkg crate emits cargo:rustc-link-lib for libraw + transitive deps (zlib).
    // C++ runtime: cl.exe / link.exe handle it implicitly — no explicit emit needed.

    let include_dir = lib.include_paths.first().ok_or_else(|| {
        "vcpkg found `libraw` but returned no include paths. \
         The vcpkg installation may be incomplete. \
         Try: `vcpkg remove libraw:x64-windows-static-md && \
         vcpkg install libraw:x64-windows-static-md`"
            .to_string()
    })?;

    // Compile our C ABI shim against the vcpkg-installed LibRaw headers.
    // cc::Build selects cl.exe on MSVC automatically.
    compile_shim(manifest_dir, include_dir)
}

// Compile the C ABI shim against LibRaw headers from `include_dir`.
//
// Called from both Unix (passing the extracted tarball root, which contains
// `libraw/libraw.h` as a subdirectory) and Windows MSVC (passing the vcpkg
// installed include directory, which also contains `libraw/libraw.h`).
// Cargo collects all `cargo:rustc-link-lib` directives before invoking the linker,
// so the order of println! calls within build.rs does not affect link ordering.
fn compile_shim(manifest_dir: &Path, include_dir: &Path) -> Result<(), String> {
    // Fail early with an actionable message if the expected header is missing,
    // rather than letting the compiler emit a confusing "No such file" diagnostic.
    let expected_header = include_dir.join("libraw").join("libraw.h");
    if !expected_header.exists() {
        return Err(format!(
            "LibRaw header not found at {}. \
             Windows: re-run `vcpkg install libraw:x64-windows-static-md`. \
             Unix: delete OUT_DIR and rebuild to re-extract the tarball.",
            expected_header.display()
        ));
    }
    let shim_src = manifest_dir.join("cpp").join("photohelper_libraw_shim.c");
    println!("cargo:rerun-if-changed={}", shim_src.display());
    cc::Build::new()
        .file(&shim_src)
        // Include LibRaw's public headers from the given root directory.
        .include(include_dir)
        // Suppress warnings from LibRaw's headers — they're upstream's problem.
        .warnings(false)
        .try_compile("photohelper_libraw_shim")
        .map_err(|e| format!("cc::Build for shim failed: {e}"))?;
    Ok(())
}

fn verify_tarball_sha256(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| {
        format!(
            "could not read vendored tarball at {}: {e}. \
             Re-download from libraw.org and re-record the SHA-256.",
            path.display()
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = format!("{:x}", hasher.finalize());
    if actual != EXPECTED_SHA256 {
        return Err(format!(
            "tarball SHA-256 mismatch at {}: expected {EXPECTED_SHA256}, got {actual}. \
             Tampered or wrong file — refuse to build. Re-download from libraw.org.",
            path.display()
        ));
    }
    Ok(())
}

fn ensure_pkg_config() -> Result<(), String> {
    // LibRaw's `configure` aborts immediately if `pkg-config` is not on
    // PATH. Emitting an actionable error here saves a confusing
    // mid-configure failure later.
    let status = Command::new("pkg-config")
        .arg("--version")
        .status()
        .map_err(|e| {
            println!(
                "cargo:warning=`pkg-config` (or `pkgconf`) is required to build LibRaw. \
                 Install via: macOS `brew install pkgconf`; \
                 Debian/Ubuntu `sudo apt install pkg-config`; \
                 Fedora `sudo dnf install pkgconf-pkg-config`."
            );
            format!("could not spawn pkg-config: {e}")
        })?;
    if !status.success() {
        return Err("`pkg-config --version` returned non-zero exit code".to_string());
    }
    Ok(())
}

fn extract_tarball(tarball: &Path, out_dir: &Path) -> Result<(), String> {
    let tarball_str = tarball.to_str().ok_or("non-UTF-8 tarball path")?;
    let out_str = out_dir.to_str().ok_or("non-UTF-8 OUT_DIR")?;
    let status = Command::new("tar")
        .args(["xzf", tarball_str, "-C", out_str])
        .status()
        .map_err(|e| {
            println!(
                "cargo:warning=`tar` not found on PATH. Install: macOS ships `tar` by default; \
                 Linux `apt install tar` or `dnf install tar`."
            );
            format!("could not spawn tar: {e}")
        })?;
    if !status.success() {
        return Err(format!("`tar xzf {tarball_str}` failed"));
    }
    Ok(())
}

fn run_configure(src_dir: &Path) -> Result<(), String> {
    // Run `./configure` through `sh` so that: (a) macOS and Linux work as before,
    // and (b) Windows + MSYS2 works — Windows cannot execute the shebang script
    // directly but MSYS2's `sh.exe` (from PATH) can.
    let status = Command::new("sh")
        .arg("./configure")
        .args([
            // Static-only — no shared library; matches §6(a) commitment.
            "--disable-shared",
            "--enable-static",
            // Drop optional features that drag in extra system deps
            // (libjpeg, lcms, OpenMP). LibRaw's bundled CR3 parser
            // does not need these for EXIF + Bayer decode.
            "--disable-jpeg",
            "--disable-lcms",
            "--disable-openmp",
            // Skip building the bin/ sample executables — we don't ship
            // them and they bloat the build by ~15s.
            "--disable-examples",
        ])
        .current_dir(src_dir)
        .status()
        .map_err(|e| {
            println!(
                "cargo:warning=could not run LibRaw's `sh ./configure`. \
                 Confirm that `sh` is on PATH (macOS/Linux: built-in; \
                 Windows: install MSYS2 and add MINGW64 to PATH)."
            );
            format!("could not spawn sh ./configure: {e}")
        })?;
    if !status.success() {
        return Err(format!(
            "LibRaw `sh ./configure` failed in {}. Rerun `cargo build -vv` to see logs.",
            src_dir.display()
        ));
    }
    Ok(())
}

fn run_make(src_dir: &Path) -> Result<(), String> {
    // GNU/BSD make difference is irrelevant for LibRaw's autoconf
    // Makefile; both work. Use `-j` for parallel compilation.
    let status = Command::new("make")
        .arg("-j")
        .current_dir(src_dir)
        .status()
        .map_err(|e| {
            println!(
                "cargo:warning=`make` not found on PATH. Install: macOS `xcode-select --install`; \
                 Debian/Ubuntu `sudo apt install build-essential`; \
                 Fedora `sudo dnf install make gcc-c++`."
            );
            format!("could not spawn make: {e}")
        })?;
    if !status.success() {
        return Err(format!(
            "LibRaw `make` failed in {}. Rerun `cargo build -vv` to see logs.",
            src_dir.display()
        ));
    }
    Ok(())
}
