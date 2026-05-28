//! LibRaw C-API FFI shim — the **only** module in `photohelper-raw` that
//! contains `unsafe` code. Defense-in-depth carries this contract three
//! ways: the crate-level `[lints.rust] unsafe_code = allow` is paired
//! with `#![forbid(unsafe_code)]` on every other source file, and `just
//! ci` runs an `rg`-based grep gate that fails the build if any other
//! `crates/photohelper-raw/src/*.rs` ever sprouts an `unsafe { ... }`
//! block.
//!
//! All FFI calls go through LibRaw's documented C-API accessor functions
//! (`libraw_init`, `libraw_open_file`, `libraw_get_iparams`, …) rather
//! than direct `#[repr(C)]` field access against `libraw_data_t`. LibRaw
//! upstream documents the accessors as ABI-stable across version bumps;
//! direct field access is silently fragile across 0.21.x → 0.22.x.
//!
//! The full FFI surface (~15 functions) lands in the Deliverable 1a body
//! commit. This file currently only carries the lint setup.

#![deny(unsafe_op_in_unsafe_fn)]
