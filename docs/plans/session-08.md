# Session 08 Plan — Export Pipeline Integration
**Branch**: `session-08/export-integration`
**Date**: 2026-05-30
**Status**: v3 — remediated (incorporating Round-2 plan-review findings)

---

## Session goal

Implement the high-performance non-destructive `photohelper-export` crate for image resizing, watermark composition, and MozJPEG encoding, and integrate it into the `export` CLI subcommand. This completes the v0.1 non-destructive workflow by allowing users to select cataloged photos above a minimum rating threshold, decode them, apply proportional watermarking and resizing, and output highly-optimized JPEGs.

---

## What will exist by end of session

1. **`photohelper-export` Crate with Core Pipeline**:
   - **Public API Contract**:
     - Exposes a clean, thread-safe library interface. A central `export_photo(options: &ExportOptions, row: &DevelopRow, metadata: &ExportMetadata) -> Result<(), ExportError>` function.
     - Struct `ExportOptions` containing fields for target output path, quality, long edge PX limit, watermark text, position, and `--force` overwrite flag.
     - Struct `ExportMetadata` encapsulates strongly-typed validated domain objects: `rating: Rating` (clamped `-1..=5` bounds) and `nima_score: Option<NimaScore>`, avoiding primitive obsession.
   - **Aspect-Ratio Preserving Resizing**:
     - Resizes sRGB `RgbImage` using high-quality bilinear or bicubic interpolation via `tiny-skia`'s matrix transformation.
     - Fully supports landscape, portrait, and square images without aspect-ratio distortion or cropping.
     - `--long-edge` limits the output size so that the larger of `width` or `height` is exactly the target dimension, leaving the shorter dimension scaled proportionally.
     - If `--long-edge` is omitted, the image remains at its original decoded resolution.
     - **Dimension & Division-by-Zero Safeguards**: Returns `ExportError::InvalidDimensions` if input image dimensions are 0. If computed proportional short-edge dimension rounds to 0 (on extremely thin panoramic strips), the dimension is clamped to a minimum of `1` pixel to prevent downstream allocation or encoder failures.
     - **Safe Allocation**: Map any `None` returned from `tiny_skia::Pixmap::new` directly to a dedicated error variant `ExportError::AllocationFailed`.
   - **Proportional Text Watermarking**:
     - Combines `cosmic-text` (pure-Rust glyph layout) and `tiny-skia` (2D software canvas) to compose crisp watermarks.
     - **Configurable Placements**: Supports bottom-left or top-right positioning via CLI `--watermark-position`.
     - **Proportional Sizing & Padding**: Font size and padding are calculated dynamically relative to the scaled image's long edge (font size is 2% of the long edge, padding is 1.5% of the scaled long edge, ensuring uniform spacing regardless of aspect ratio), guaranteeing identical relative positioning and size across mixed-resolution exports.
     - **Watermark Coordinate Alignment & Layout Checks**: Coordinates are computed in signed `i32`/`f32` space using integer-aligned pixel offsets (`1px` or `2px` drop shadow instead of subpixel `1.5px`) to prevent subpixel text blurring. If the watermark text width exceeds the scaled image width, or if coordinates overflow, the watermark is omitted with a logged warning rather than over-engineering complex scaling logic.
     - **Portability & Headless CI Fallbacks**: Embeds a lightweight, high-quality open-source sans-serif TrueType font (Roboto Mono) inside the binary via `include_bytes!`. To prevent slow disk scans of host font folders, the thread-local cache instantiates `FontSystem::new_with_locale_and_db` with an empty font database and manually registers our embedded font. This guarantees fast, deterministic rendering on headless CI runners without host system fonts.
   - **MozJPEG Optimized Compression**:
     - Converts scaled sRGB premultiplied RGBA pixels from `tiny-skia::Pixmap` into standard, un-premultiplied 3-channel sRGB RGB.
     - **Format Conversion & Demultiplication**: Utilizes `tiny_skia::Pixmap::take_demultiplied()` to retrieve safely demultiplied pixels, or checks if alpha is 0 (to return 0) or 255 (fast-path optimization to skip CPU division math entirely) to prevent division-by-zero panics. Drops the 4th alpha byte to form a contiguous 3-channel sRGB RGB buffer.
     - **Safe MozJPEG FFI Boundary**: Wrapped in a panic-safe, error-catching boundary that catches unwind panics or C-level error codes and returns an `ExportError`. To prevent memory corruption, utilize MozJPEG's high-level safe scanning APIs (`Compress::write_scanlines`) or construct fully pinned row-pointer arrays inside Rust instead of flat 1D raw pointers.
     - **Fast-Path Omission**: If `--long-edge` is omitted and no watermark text is provided, the pipeline bypasses `tiny-skia` entirely and passes decoded pixels directly to MozJPEG to maximize processing speed.
     - Utilizes optimized Huffman coding, progressive scans, and 4:2:0 chroma downsampling to deliver maximum visual fidelity per byte.
     - Supports `--quality` (1 to 100, default 80).

2. **CLI `export` Subcommand**:
   - **Clap Options**:
     - `--output <DIR>` (required): Output directory for compiled JPEGs. Checked for write access and created recursively upfront on the main thread.
     - `--long-edge <PX>` (optional): Long-edge resize limit in pixels. Strictly validated to be $\ge 16$ pixels.
     - `--quality <QUALITY>` (optional, default 80): JPEG quality level (range `1..=100`).
     - `--watermark <TEXT>` (optional): Watermark text.
     - `--watermark-position <POS>` (optional, default `bottom-left`): Position of watermark (`bottom-left` or `top-right`).
     - `--min-rating <RATING>` (optional, default 3): Minimum rating to export (range `0..=5`). Setting to `0` exports all photos (including unrated ones) while still respecting explicit user rejections.
     - `--force` (optional): Force overwriting of existing output JPEGs. By default, existing JPEGs are skipped with a warning.
     - `--strict` (optional): Treat any single-photo export failure as fatal, causing immediate pipeline cancellation and a non-zero exit code.
   - **Effective Rating Evaluation**:
     - Resolves each photo's rating non-destructively:
       1. **XMP Sidecar first**: Checks if a corresponding `.xmp` file exists next to the raw photo. If it exists, parses the rating and validates/clamps it to the `-1..=5` bounds. If parsing of an existing XMP fails, treat it as a per-photo export failure rather than silently falling back.
       2. **NIMA score second**: If no XMP rating exists, looks up the NIMA score in the catalog (if finite and not `NaN`) and maps it to a rating (Score $< 4.0 \rightarrow 1$; $[4.0, 5.5) \rightarrow 2$; $[5.5, 7.0) \rightarrow 3$; $[7.0, 8.5) \rightarrow 4$; $\ge 8.5 \rightarrow 5$).
       3. **Fallback**: Defaults to Unrated (0) if neither is found.
     - If a photo evaluates to `-1` (Rejected) or is strictly less than `--min-rating`, it is excluded from export.
   - **Upfront Pre-Flight Checks & Batched SQLite Queries**:
     - Validates write permissions and recursively creates `--output <DIR>` on the main thread *before* processing begins.
     - Performs a single, batched SQLite query on the main thread to retrieve all active catalog photos, building a in-memory collection of `Vec<DevelopRow>` (retaining the unique `photo_id`, `path`, database ratings, and NIMA scores). Do NOT perform rating/score exclusion in SQL, as that would miss on-disk XMP rating overrides.
   - **Rayon Parallel Processing**:
     - Distributes RAW decoding, resizing, watermarking, and JPEG encoding across physical CPU cores by calling `.into_par_iter()` on the collected queue of `DevelopRow` rows. Retaining `DevelopRow` ensures `photo_id` is available for suffix-based unique collision mapping.
     - **Deterministic Collision Resolution**: Path collisions are resolved upfront on the main thread, generating an immutable `HashMap<PathBuf, PathBuf>` mapping each source RAW path to a unique, fully-suffixed target output path. This eliminates concurrent directory-checking race hazards (TOCTOU).
     - **Safe Atomic Temp Writes**: Writes JPEGs to `<output-path>.tmp` first. Uses a RAII `TempFileGuard` wrapper that deletes the temporary `.tmp` file in its `Drop` implementation unless it is explicitly committed upon successful compression, preventing orphaned files on unwinding panics or abrupt cancellation.
     - **Strict Cancellation**: Under `--strict`, an `AtomicBool` cancellation flag is flipped upon any failure, signaling remaining Rayon worker threads to abort processing early, preventing resource waste.
     - **Cooperative Progress Heartbeat**: Spawns a background progress heartbeat thread on `stderr` that outputs progress stats (walked, written, errored) every 10 seconds. It listens on a shutdown channel or condvar timeout to terminate instantly on command completion, preventing CLI exit hangs.
     - **Exit Codes**: Returns a non-zero exit code if `--strict` is enabled and any failure occurs, or if ANY export error is encountered across the run, ensuring pipeline issues are always reported.

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
$$\text{rating}(P) = \begin{cases}
\text{Rating}(P_{\text{xmp}}) & \text{if } P_{\text{xmp}} \text{ exists, parses successfully, and is clamped } \in [-1, 5] \\
\text{Rating}_{\text{nima}}(P_{\text{catalog}}) & \text{else if NIMA score exists and is finite} \\
0 & \text{otherwise}
\end{cases}$$
- If $\text{rating}(P) == -1$ (Rejected), $P$ is immediately skipped.
- Else if $\text{rating}(P) \ge \text{min\_rating}$, $P$ is exported.

### D2 — Proportional Watermark Rendering Invariants
- Let $L$ be the long edge length of the final output image.
- Font size $F = \max(12, \text{round}(L \times 0.02))$ pixels.
- Padding $D = \max(8, \text{round}(L \times 0.015))$ pixels.
- Watermark position is aligned relative to top-right or bottom-left (configurable via CLI, default bottom-left).
- Layout coordinates are computed using signed `i32` or `f32` space:
  - For `bottom-left`:
    $$x_{pos} = D$$
    $$y_{pos} = H - D - H_{text}$$
  - For `top-right`:
    $$x_{pos} = W - D - W_{text}$$
    $$y_{pos} = D$$
  - Coordinate Safeguards: If $x_{pos} < 0$ or $y_{pos} < 0$ or $x_{pos} + W_{text} > W$, the watermark is omitted with a logged warning rather than causing underflows or out-of-bounds draws.
- **Legibility blend**: Render text 4-way offset by integer `1px` or `2px` using black fill (30% opacity) behind the main white fill (70% opacity).

---

## Proposed Implementation Steps

### Step 1: Dependency Setup & Compilation Safety
Override the global workspace's `unsafe` restriction inside `crates/photohelper-export/Cargo.toml` by explicitly allowing unsafe code and clippy lints where FFI takes place:
```toml
# photohelper-export/Cargo.toml
[package]
name = "photohelper-export"
version = "0.1.0"
edition = "2021"

[dependencies]
tiny-skia.workspace = true
cosmic-text.workspace = true
mozjpeg.workspace = true
tracing.workspace = true
thiserror.workspace = true

[lints.rust]
unsafe_code = "allow" # Explicit override for MozJPEG FFI bindings
```
Add the workspace dependency `photohelper-export = { path = "../photohelper-export" }` to `crates/photohelper-cli/Cargo.toml`.

### Step 2: Resizing Pipeline & Error Safety
Implement aspect-ratio preserving scaling. Convert sRGB `RgbImage` to `tiny-skia::Pixmap` (padding alpha to 255). Validate `--long-edge` to be $\ge 16$ at parse time, compute optimal dimensions, clamp them to $\ge 1$ pixel, apply matrix scale transform, and render into a new `Pixmap`.
- Verify input image dimensions are non-zero, returning `ExportError::InvalidDimensions` otherwise.
- Safely handle `Pixmap::new` OOM/invalid dimensions, mapping `None` to `ExportError::AllocationFailed`.

### Step 3: Thread-Local FontSystem and Watermark Composition
Implement pure-Rust text layout and rasterization:
- Bundle the TTF font Roboto Mono as an internal byte array `include_bytes!`.
- Use a `thread_local!` cache for `cosmic-text::FontSystem` and `cosmic-text::SwashCache` to prevent scanning directories per-photo. Initialize the `FontSystem` with an empty font database using `FontSystem::new_with_locale_and_db(locale, FontDatabase::new())` to bypass system-level I/O directory scans.
- Compute dynamic font size and padding based on scaled long-edge size. Compute position in signed space (`i32`/`f32`), applying coordinates bounds checks and integer-aligned offsets (`1px` or `2px`).
- Render text in 4-way offset drop shadow behind the white text on the `tiny-skia` scaled canvas.

### Step 4: MozJPEG Encoding wrapper
Wrap MozJPEG's write loop:
- Extract and demultiply alpha channels from `tiny-skia::Pixmap`. Use `tiny_skia::Pixmap::take_demultiplied()` to retrieve demultiplied bytes safely, fast-pathing the loop to skip the division logic entirely for opaque pixels (alpha = 255) to conserve CPU cycles.
- Pack RGB bytes into a contiguous buffer of size $3 \times W \times H$.
- Convert the RGB buffer to progressive-scan JPEG bytes utilizing MozJPEG's high-level safe scanning interfaces (`Compress::write_scanlines`) to prevent flat-pointer memory corruption. Wrap compression inside a panic-safe catch boundary to capture unwinds and return `ExportError`.
- If no long edge limit and no watermark text is provided, implement a fast-path that bypasses `tiny-skia` entirely and streams pixels straight to MozJPEG.

### Step 5: CLI Subcommand Integration
Modify `photohelper-cli`'s command processor to wire up `export`:
- Parse and validate CLI flags, validating output directory recursively on the main thread upfront.
- Batch query all active catalog photo records upfront into a `Vec<DevelopRow>` on the main thread.
- Perform deterministic collision resolution upfront on the main thread, generating an immutable map `HashMap<PathBuf, PathBuf>` mapping RAW paths to unique output paths.
- Collect targets into a `Vec` and process using `.into_par_iter()`.
- Implement `AtomicBool` cancellation token and safe atomic file writes using `TempFileGuard` (deleting `.tmp` file in its `Drop` implementation if not committed).
- Spawn stderr progress heartbeat thread with cooperative exit checks using channel receiver timeouts or condvar wait-timeouts.
- Treat sidecar XML parse errors as export failures. Clamp/validate ratings.
- Ensure the process returns a non-zero exit code on any `--strict` failure, or if any photo export fails.
- Document `TD-025` entry in `TECH-DEBT.md`.

---

## Test Plan & Verification

### Automated Unit Tests
1. **Aspect-Ratio Preserving Resize**:
   - Verify scaling logic for landscape, portrait, and square inputs.
   - Verify that output dimensions are correct, and clamped to $\ge 1$ pixel.
2. **Proportional Watermark Bounds & Coordinate Safety**:
   - Verify text placement arithmetic in signed space does not panic on extremely small dimensions (e.g., $10 \times 10$).
   - Verify drop-shadow offset integer alignment.
3. **MozJPEG Format Conversion**:
   - Assert that demultiplexing RGBA to RGB correctly drops alpha and un-premultiplies pixel colors safely without division-by-zero panics.
   - Verify fast-path opaque pixel handling (alpha = 255) skipping division.
   - Ensure JPEGs contain standard markers.
4. **Piecewise Rating Fallback & Validation**:
   - Mock XMP rejections (`-1`), unrated states, and high NIMA scores, asserting that explicit manual settings take absolute precedence.
   - Assert that invalid/corrupt XMP ratings are validated/clamped or raise errors instead of silent fallbacks.

### Integration Tests
- Run `photohelper export` command end-to-end on synthetic RAW photo catalogs.
- Verify target directories are created and output JPEGs are written atomically.
- Assert that missing RAW files skip gracefully and under `--strict` trigger exit-code fatal failures.
- Assert that strict cancellation short-circuits further processing upon the first error.
