//! RAW decode pipeline for photohelper.
//!
//! Wraps the LibRaw C library (vendored at `=0.22.1`; see
//! `docs/adr/0002-libraw-lgpl-static-link-mechanics.md` once the
//! Deliverable 2 build-system commit authors it) to extract EXIF metadata
//! and decode Bayer-pattern sensor data from Canon CR3 files. Canon
//! R8 is the first supported body; other Canon bodies + non-Canon
//! formats land per `docs/discovery-notes.md § DN-014`.
//!
//! ## Module layout
//!
//! * [`exif`] — `RawExif` + `read_cr3` (LibRaw EXIF extraction).
//! * [`decode`] — `RawImage` + `read_raw` (LibRaw sensor decode).
//! * `ffi` (private) — the single module that may use `unsafe`. Every
//!   other module in the crate carries `#![forbid(unsafe_code)]` and a
//!   workspace-level `rg` grep gate in `just ci` catches any accidental
//!   `unsafe` outside `ffi.rs`.

mod ffi;

pub mod decode;
pub mod exif;
