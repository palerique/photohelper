#!/usr/bin/env bash
# Defense-in-depth: ensure `unsafe` only appears inside
# `crates/photohelper-raw/src/ffi.rs`. The crate Cargo.toml allows
# `unsafe_code` for FFI; every other file in the crate carries
# `#![forbid(unsafe_code)]`. This grep is the third layer — catches
# files added without the forbid attribute.
#
# Pattern matches:
#   - `unsafe { ... }` blocks
#   - `unsafe fn` declarations
#   - `unsafe trait` declarations
#   - `unsafe impl` declarations
# Without matching the word `unsafe` in a comment or string.
set -euo pipefail

RG_BIN="${RG_BIN:-rg}"
if ! command -v "$RG_BIN" >/dev/null 2>&1; then
    echo "check-unsafe-isolation.sh: ripgrep ($RG_BIN) not found; install via 'brew install ripgrep'" >&2
    exit 2
fi

# Exit 0 means "no match found" which is the green path here. `rg` exits
# 1 on no-match, so we invert: any matches → fail.
if "$RG_BIN" --type rust --glob '!ffi.rs' \
    '\bunsafe\s*(\{|fn\b|trait\b|impl\b)' \
    crates/photohelper-raw/src/; then
    echo ""
    echo "ERROR: unsafe code found outside crates/photohelper-raw/src/ffi.rs"
    echo "  - The FFI module is the only place \`unsafe\` is allowed."
    echo "  - If you genuinely need unsafe elsewhere, file an ADR first."
    echo "  - Otherwise: move the code into ffi.rs and expose a safe wrapper."
    exit 1
fi

echo "unsafe-isolation: clean (no unsafe outside crates/photohelper-raw/src/ffi.rs)"
