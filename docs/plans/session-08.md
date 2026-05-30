# Session 08 Plan — Export Pipeline Integration
**Branch**: `session-08/export-integration`
**Date**: 2026-05-30
**Status**: v1 — draft (pending Multi-Agent Plan Review)

---

## Session goal

Implement the high-performance non-destructive `photohelper-export` crate for image resizing, watermark composition, and MozJPEG encoding, and integrate it into the `export` CLI subcommand. This completes the v0.1 non-destructive workflow by allowing users to select cataloged photos above a minimum rating threshold, decode them, apply proportional watermarking and resizing, and output highly-optimized JPEGs.

---

## What will exist by end of session

1. **`photohelper-export` Crate with Core Pipeline**:
   - **Aspect-Ratio Preserving Resizing**:
     - Resizes sRGB `RgbImage` using high-quality bilinear or bicubic interpolation via `tiny-skia`'s matrix transformation.
     - Fully supports landscape, portrait, and square images without aspect-ratio distortion or cropping.
     - `--long-edge` limits the output size so that the larger of `width` or `height` is exactly the target dimension, leaving the shorter dimension scaled proportionally.
     - If `--long-edge` is omitted, the image remains at its original decoded resolution.
   - **Proportional Text Watermarking**:
     - Combines `cosmic-text` (pure-Rust glyph layout/rasterization) and `tiny-skia` (2D hardware-accelerated vector canvas) to compose beautiful, crisp watermarks.
     - Portrait/landscape aware placements: supports bottom-left or top-right positioning.
     - **Proportional Sizing & Padding**: Font size and padding are calculated dynamically relative to the scaled image's long edge (e.g. font size is 2% of the long edge, padding is 1.5% of the respective edge), ensuring identical relative positioning and size across mixed-resolution exports.
     - **Legibility Design**: Semi-transparent white text (70% opacity, `rgba(255, 255, 255, 0.7)`) with a subtle dark drop shadow or stroke (30% opacity, `rgba(0, 0, 0, 0.3)`) to guarantee perfect readability on both extremely light (snow/sky) and extremely dark (night) backgrounds.
     - **Font Fallbacks**: Standard, widely-available system font collections are automatically loaded/matched via `cosmic-text`'s system fallback mechanism.
   - **MozJPEG Optimized Compression**:
     - Encodes final sRGB buffers into standard JPEGs using the `mozjpeg` crate.
     - Utilizes optimized Huffman coding, progressive scans, and custom chroma downsampling (e.g. 4:2:0 default) to deliver maximum visual fidelity per byte.
     - Supports `--quality` (1 to 100, default 80).

2. **CLI `export` Subcommand**:
   - **Clap Options**:
     - `--output <DIR>` (required): Output directory for compiled JPEGs. Created recursively if missing.
     - `--long-edge <PX>` (optional): Long-edge resize limit in pixels.
     - `--quality <QUALITY>` (optional, default 80): JPEG quality level (1 to 100).
     - `--watermark <TEXT>` (optional): Watermark text.
     - `--min-rating <RATING>` (optional, default 3): Minimum rating to export (range `1..=5`).
     - `--strict` (optional): Treat any single-photo export failure as fatal for the overall run.
   - **Effective Rating Evaluation**:
     - Resolves each photo's rating non-destructively:
       1. **XMP Sidecar first**: Checks if a corresponding `.xmp` file exists next to the raw photo. If it has `xmp:Rating`, use that.
       2. **NIMA score second**: If no XMP rating exists, look up the NIMA score in the catalog and map it to a rating (Score $< 4.0 \rightarrow 1$; $[4.0, 5.5) \rightarrow 2$; $[5.5, 7.0) \rightarrow 3$; $[7.0, 8.5) \rightarrow 4$; $\ge 8.5 \rightarrow 5$).
       3. **Fallback**: Default to Unrated (0) if neither is found.
     - Filters out all photos whose effective rating is strictly less than `--min-rating`.
   - **Rayon Parallel Processing**:
     - Distributes RAW decoding, resizing, watermarking, and JPEG encoding across all physical CPU cores using `rayon::par_bridge`.
     - Maintains complete resilience: print per-photo errors to stderr and continue processing remaining photos. Exit non-zero if `--strict` is active and errors occurred.

3. **Workspace-Wide Verification**:
   - Fully compiles under Rust `1.88` with clean clippy lints and `-D warnings` enabled.
   - Passing `just ci` with comprehensive unit and integration test coverage across the entire export pipeline.

---

## What is explicitly OUT OF SCOPE (deferred TDs)

| TD | Trigger | Rationale for deferral |
|---|---|---|
| TD-012 | Fires when develop does demosaic | Export-only demosaic via `photohelper-raw`'s FFI is sufficient; no custom slider-based demosaic needed in v0.1. |
| TD-022 | First session adding non-crs fields | Pure-Rust templates are still used for XMP sidecars. |
| TD-024 | Dedup visual-matching enhancements | Dedup algorithm is stable in v0.1; no active changes. |

---

## Stop-gap declarations

| # | Stop-gap | TD | Introducing commit | Location | Binding trigger |
|---|---|---|---|---|---|
| S2 | MozJPEG uses wrapper FFI linking rather than a pure-Rust crate | TD-025 | `crates/photohelper-export/Cargo.toml` | `photohelper-export` | First session requiring static musl compilation without C-toolchain deps. |

---

## Design decisions locked by this plan

### D1 — Effective Rating Evaluation Logic
To decide whether photo $P$ gets exported:
$$\text{rating}(P) = \max(\text{Rating}(P_{\text{xmp}}), \text{Rating}_{\text{nima}}(P_{\text{catalog}}), 0)$$
If $\text{rating}(P) \ge \text{min\_rating}$, $P$ is exported.

### D2 — Proportional Watermark Rendering Invariants
- Let $L$ be the long edge length of the final output image.
- Font size $F = \max(12, \text{round}(L \times 0.02))$ pixels.
- Padding $D = \max(8, \text{round}(L \times 0.015))$ pixels.
- Watermark position is aligned relative to bottom-right or bottom-left (configurable or hardcoded default bottom-right/bottom-left). For v0.1, we hardcode bottom-left as standard, portrait/landscape aware.
- Text uses a black outline (stroke width = 1.5px, 30% opacity) surrounding semi-transparent white text (70% opacity) to remain legible across any background.

---

## Proposed Implementation Steps

### Step 1: Dependency Setup
Add necessary dependencies to `photohelper-export/Cargo.toml`:
```toml
[dependencies]
photohelper-core = { path = "../photohelper-core" }
tiny-skia = "0.12"
cosmic-text = "0.19"
mozjpeg = "0.10"
tracing.workspace = true
thiserror.workspace = true
```

And update workspace `Cargo.toml` to register these versions.

### Step 2: Resizing Pipeline
Implement aspect-ratio preserving scaling. Convert sRGB `RgbImage` to `tiny-skia::Pixmap`, compute optimal target dimensions based on `--long-edge`, apply matrix scale transform, and render into a new `Pixmap`.

### Step 3: Cosmic-Text Watermark Composition
Implement pure-Rust text layout and rasterization:
- Load default system sans-serif font using `cosmic-text::FontSystem` and `Buffer`.
- Compute dynamic font size and padding.
- Render text into mask/outline, blend using `tiny-skia` over the scaled image canvas.

### Step 4: MozJPEG Encoding wrapper
Wrap MozJPEG's write loop:
- Convert scaled sRGB pixel buffer to progressive-scan JPEG bytes.
- Apply standard optimized coding and chroma downsampling.

### Step 5: CLI Subcommand Integration
Modify `photohelper-cli`'s command processor to wire up `export`:
- Parse CLI flags.
- Query non-superseded catalog photos.
- Perform the 3-tier rating resolution per photo.
- Feed matching images into the Rayon-parallel export pipeline.

---

## Test Plan & Verification

### Automated Unit Tests
1. **Aspect-Ratio Preserving Resize**:
   - Verify scaling logic for landscape, portrait, and square inputs.
   - Verify that output dimensions are correct when `--long-edge` is set and when it is omitted.
2. **Proportional Watermark Bounds**:
   - Verify font size and padding computations are proportional.
   - Verify text bounding-box bounds check to ensure no out-of-bounds painting.
3. **MozJPEG Write Verification**:
   - Ensure JPEGs are written successfully and contain standard JPEG SOI/EOI markers.
4. **Rating Fallback Integration**:
   - Unit test that mocks both XMP existence and NIMA database records, verifying the effective rating maps perfectly to expectations.

### Integration Tests
- Run `photohelper export` command end-to-end on synthetic RAW photo catalogs (CR3 mocks or actual minimal raw photos).
- Verify target directories are created and output JPEGs are valid, readable, and properly sized.
