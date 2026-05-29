//! Integration tests for `photohelper_ai::MobileClip` against CC0 CR3 fixtures.
//!
//! Lives in `photohelper-raw/tests/` because this crate already has LibRaw
//! compiled and the CR3 fixture infrastructure set up.
//!
//! Tests D1c acceptance criteria (docs/plans/session-05.md):
//! - embed on CC0 fixture: dim=512, L2-norm ∈ [0.99, 1.01]
//! - two fixtures: cosine_sim ≥ 0.90 (empirical D0: ~0.923)
//! - determinism: second embed → cosine_sim ≥ 0.999

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    unused_crate_dependencies
)]

mod common;

use common::{fixture_is_real_cr3, fixture_path};
use photohelper_ai::{CLIP_MODEL_MANIFEST_NAME, MobileClip, VerifiedModelBytes};
use photohelper_raw::decode::read_raw_rgb;
use std::path::PathBuf;

fn models_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/photohelper-raw; workspace root is ../../
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("crates/photohelper-ai/models")
}

fn load_clip() -> MobileClip {
    let dir = models_dir();
    let verified = VerifiedModelBytes::from_manifest(&dir, CLIP_MODEL_MANIFEST_NAME)
        .expect("CLIP model must load and pass SHA-256 check");
    MobileClip::new(&verified)
}

/// D1c test 1: embed on CC0 CR3 fixture verifies dim=512 and L2-norm ≈ 1.0.
#[test]
fn clip_embed_cc0_fixture_dim_and_norm() {
    let clip = load_clip();
    let path = fixture_is_real_cr3(&fixture_path("CRAW_FULL_FRAME.CR3"));
    let rgb = read_raw_rgb(&path).expect("LibRaw must decode the CC0 R8 CR3");
    let emb = clip.embed(&rgb, &path).expect("CLIP embed must succeed");

    assert_eq!(
        emb.dim(),
        512,
        "CLIP ViT-B/32 must produce 512-dim embeddings"
    );
    // norm is baked into the ONNX model (baked L2-normalize layer).
    // ImageEmbedding::from_raw already verified norm ∈ [0.99, 1.01] at construction.
    // Re-verify via round-trip bytes for belt-and-suspenders.
    let bytes = emb.as_f32_le_bytes();
    assert_eq!(bytes.len(), 512 * 4);
    let norm: f32 = bytes
        .chunks_exact(4)
        .filter_map(|c| <[u8; 4]>::try_from(c).ok().map(f32::from_le_bytes))
        .map(|v| v * v)
        .sum::<f32>()
        .sqrt();
    assert!(
        (norm - 1.0).abs() < 0.02,
        "embedding norm must be near 1.0, got {norm}"
    );
}

/// D1c test 2: two CC0 fixtures produce cosine_sim in the expected band.
///
/// Empirical results (Apple Silicon CPU):
/// - Python + PIL bicubic (D0 probe): cosine_sim ≈ 0.923
/// - Rust + bicubic center-crop (TD-020 fix, session 06): cosine_sim ≥ 0.90
///
/// Threshold tightened from ≥0.80 (bilinear stop-gap) to ≥0.90 (bicubic,
/// matching Python OpenCLIP reference more closely). Cross-arch f32 variation
/// (DN-027) is absorbed by the 0.90 lower bound.
/// Lower bound of 0.98 guards against identical-image false positives.
#[test]
fn clip_embed_two_fixtures_golden_cosine_similarity() {
    let clip = load_clip();

    let path_craw = fixture_is_real_cr3(&fixture_path("CRAW_FULL_FRAME.CR3"));
    let rgb_craw = read_raw_rgb(&path_craw).expect("decode CRAW");
    let emb_craw = clip.embed(&rgb_craw, &path_craw).expect("embed CRAW");

    let path_raw = fixture_is_real_cr3(&fixture_path("RAW_FULL_FRAME.CR3"));
    let rgb_raw = read_raw_rgb(&path_raw).expect("decode RAW");
    let emb_raw = clip.embed(&rgb_raw, &path_raw).expect("embed RAW");

    let sim = emb_craw
        .cosine_similarity(&emb_raw)
        .expect("same-model embeddings must have equal dim");

    // Similar Canon R8 scenes → meaningful (not random) embedding similarity.
    // Threshold tightened from ≥0.80 (bilinear TD-020 stop-gap) to ≥0.90 (bicubic).
    assert!(
        sim >= 0.90,
        "cosine_sim(CRAW, RAW) must be ≥ 0.90 (bicubic preprocessing); got {sim:.6}"
    );
    // Distinct images (different exposures / content) → not identical.
    assert!(
        sim < 0.98,
        "cosine_sim(CRAW, RAW) must be < 0.98 (these are different shots); got {sim:.6}"
    );
}

/// D1c test 3: second embed on same image is deterministic (cosine_sim ≥ 0.999).
#[test]
fn clip_embed_is_deterministic() {
    let clip = load_clip();
    let path = fixture_is_real_cr3(&fixture_path("CRAW_FULL_FRAME.CR3"));
    let rgb = read_raw_rgb(&path).expect("decode CR3");

    let emb1 = clip.embed(&rgb, &path).expect("first embed");
    let emb2 = clip.embed(&rgb, &path).expect("second embed");

    let sim = emb1.cosine_similarity(&emb2).expect("same-dim embeddings");

    assert!(
        sim >= 1.0 - 1e-3,
        "same-image embeds must be near-identical; cosine_sim={sim:.8}"
    );
}
