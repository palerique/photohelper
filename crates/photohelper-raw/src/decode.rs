//! `RawImage` + `read_raw` — LibRaw Bayer-pattern sensor decode surface.
//!
//! The Deliverable 1c body commit adds `RawImage`, `BayerPlane`,
//! `CfaPattern`, `SensorLevels`, `SensorBitDepth`, `WhiteBalance`, and
//! `CamRgbToXyzD65Matrix`. Every one is a strong newtype with private
//! fields, a fallible constructor, and the R2-T6 / R3-T5 invariants per
//! `docs/plans/session-02.md § Deliverable 1c`. The `read_raw` entry
//! point joins them via the FFI. This file currently only carries the
//! lint setup.

#![forbid(unsafe_code)]
