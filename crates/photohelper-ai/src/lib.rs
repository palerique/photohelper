//! AI inference for photohelper — NIMA aesthetic scorer + CLIP embedding via ONNX Runtime.
//!
//! Models:
//! - NIMA: `nima_mobilenet_aesthetic.onnx` (Apache-2.0; idealo/image-quality-assessment)
//! - CLIP: `clip_vit_b32_laion2b_int8.onnx` (MIT; laion/CLIP-ViT-B-32-laion2B-s34B-b79K)
//!
//! Threading model: `Session::run` is `&mut self` (verified in ANL-002 + ANL-003);
//! one `Session` per rayon worker via `thread_local!`.

pub mod embedding;
pub mod error;
pub mod model_bytes;
pub mod nima;

pub use embedding::ImageEmbedding;
pub use error::Error;
pub use model_bytes::{MODEL_MANIFEST_NAME, MODEL_SLUG, VerifiedModelBytes};
pub use nima::{Nima, NimaScore};

/// Model slug for the CLIP ViT-B/32 LAION2B image embedder (catalog `model_slug` column).
pub const CLIP_MODEL_SLUG: &str = "clip-vit-b32-laion2b-v1";

/// Manifest.toml section name for the CLIP model (matches filename stem before `_int8.onnx`).
pub const CLIP_MODEL_MANIFEST_NAME: &str = "clip_vit_b32_laion2b";
