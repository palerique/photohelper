//! AI inference for photohelper — NIMA aesthetic scorer via ONNX Runtime.
//!
//! ONNX model: `crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx`
//! (Apache-2.0; converted from idealo/image-quality-assessment via tf2onnx).
//!
//! Threading model: `Session::run` is `&mut self` (verified in ANL-002);
//! one `Session` per rayon worker via `thread_local!` (plan §D1c).

pub mod error;
pub mod model_bytes;
pub mod nima;

pub use error::Error;
pub use model_bytes::{MODEL_SLUG, VerifiedModelBytes};
pub use nima::{Nima, NimaScore};
