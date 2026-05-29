// D1a declares ort/thiserror/static_assertions/photohelper_core as deps;
// they are referenced in D1b/c/d code that lands in the same session.
// This allow suppresses the lint while D1a is the only committed commit.
#![allow(unused_crate_dependencies)]
//! AI inference for photohelper — NIMA aesthetic scorer via ONNX Runtime.
//!
//! D1a: ort dep wired (session 04). D1b/c/d/e follow with the full type
//! hierarchy: `VerifiedModelBytes`, `NimaScore`, `Nima`, `Error`.
//!
//! ONNX model: `crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx`
//! (Apache-2.0; converted from idealo/image-quality-assessment via tf2onnx).
//! SHA-256: `f181fa8911dad2c4d5c8fbced3056c30b617d12b00cd411fd40eecd047752228`
//!
//! Threading model: `Session::run` is `&mut self` (verified in ANL-002);
//! one `Session` per rayon worker via `thread_local!` (plan §Binding decisions A5).
