//! `RawExif` + `read_cr3` — LibRaw EXIF extraction surface.
//!
//! The Deliverable 1b body commit adds the `RawExif` type with private
//! fields, a fallible constructor, and accessor methods per
//! `docs/plans/session-02.md § Deliverable 1b`, plus the `read_cr3`
//! entry point that drives the FFI through to a validated `RawExif`.
//! This file currently only carries the lint setup.

#![forbid(unsafe_code)]
