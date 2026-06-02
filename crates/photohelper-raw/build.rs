//! Build script: extract + autoconf-build the vendored LibRaw `=0.22.1`
//! tarball as a static library and emit the link directives.
//!
//! Plan-v3.2 chose `=0.22.1` after the Deliverable 0 pre-flight
//! (`docs/analysis/ANL-001-libraw-cr3-preflight.md`). The plan originally
//! prescribed `cmake` as the build driver, but LibRaw 0.22.1's tarball
//! ships ONLY the autoconf scripts (`configure`, `Makefile.am`); the
//! cmake build files live in a separate `LibRaw/LibRaw-cmake` repo
//! per the bundled `README.cmake`. Vendoring a second repo for cmake
//! rules would double the §6(a) tarball-shipping surface, so this
//! build.rs invokes the official autoconf path (`./configure && make`)
//! instead.
//!
//! ## System prerequisites
//!
//! * `pkg-config` (or `pkgconf` shim) on `PATH` — LibRaw's `configure`
//!   probes for it unconditionally.
//! * GNU make (or BSD make on macOS, which suffices for LibRaw's
//!   Makefile.am-generated Makefile).
//! * A C++ compiler — Xcode CLT on macOS (`xcode-select --install`),
//!   `build-essential` on Debian/Ubuntu, `gcc-c++` on Fedora.
//!
//! Missing tools fail the build with a `cargo:warning=` line naming
//! the exact package to install on macOS and Linux.

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

    if let Err(e) = run() {
        eprintln!("photohelper-raw build.rs: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|_| "CARGO_MANIFEST_DIR not set")?);
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

    // Compile our minimal C ABI shim against LibRaw's headers. `cc::Build`
    // emits a sibling static lib and the matching `cargo:rustc-link-lib`
    // directive automatically.
    compile_shim(&manifest_dir, &src_dir)?;

    // LibRaw is C++; we need the C++ standard library at the final link.
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
    // LibRaw uses zlib (the configure-time default we accept).
    println!("cargo:rustc-link-lib=z");

    Ok(())
}

fn compile_shim(manifest_dir: &Path, libraw_src_dir: &Path) -> Result<(), String> {
    let shim_src = manifest_dir.join("cpp").join("photohelper_libraw_shim.c");
    println!("cargo:rerun-if-changed={}", shim_src.display());
    cc::Build::new()
        .file(&shim_src)
        // Include LibRaw's public headers (extracted into OUT_DIR).
        .include(libraw_src_dir)
        // Suppress warnings from LibRaw's headers — they're upstream's
        // problem, not ours.
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
