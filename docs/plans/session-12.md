# Session 12 Plan — Native ISP Engine & Dynamic Watermarking

## 1. Goal
Elevate `photohelper export` from a proxy-renderer to a true standalone Image Signal Processor (ISP) by establishing a 16-bit linear pipeline (DN-033), and implement dynamic, auto-scaling image-based watermarks (DN-036).

## 2. Deliverables
- **Workspace Cleanup**:
  - Remove leftover discovery files: `crates/photohelper-export/src/bin.rs` and `crates/photohelper-export/src/pixmap_test.rs`.
  - Remove the dangling `mod pixmap_test;` declaration from `crates/photohelper-export/src/lib.rs`.

- **DN-036: Dynamic Image Watermarks**
  - **CLI Updates**: Add `--badge path=<PATH>,pos=<POSITION>[,scale=<PERCENT>]` key-value argument. Introduce a strong newtype `struct Scale(f32)` enforcing `0.0 < scale <= 1.0` (or `100.0`) at construction to guarantee affine bounds safety.
  - **Auto-Scaling**: Automatically calculate a default `Scale` (e.g., 5% of target long edge) if omitted, tightly bounding the geometry mathematics.
  - **O(1) Validation via Sum Type**: Parse directly into a `HashMap<WatermarkPosition, Watermark>`, where `enum Watermark { Text(String), Image(BadgeConfig) }`. Use the `Entry` API to trigger `ExportError::DuplicateWatermarkPosition` if any collision occurs, strictly enforcing the one-watermark-per-position invariant structurally.
  - **Error Propagation**: Return explicit `ExportError::BadgeLoadFailed { path, reason }` upon IO/Decode failures. Ensure the export function is wrapped in `#[instrument(skip(options))]` for tracing.
  - **Rendering**: Leverage `tiny-skia` to render the badge into a small, temporary RGBA buffer (affine transform). Use a trivial, fast 1:1 pixel premultiplied-over blending loop (`dst = src + dst * (1 - src_alpha)`) to composite it directly onto the massive RGB background array. Ignore EXIF orientation in Rust since LibRaw pre-rotates the buffer.

- **DN-033: Standalone ISP Pipeline (Phase 1)**
  - **Data Boundary**: Expand `ExportOptions` with `ToneMappingOptions` carrying XMP edits.
  - **C-Shim Updates**: Replace individual setters with a declarative C-shim struct/function (`photohelper_decode_with_options(ctx, options)`) handling 16-bit, linear gamma, and no-auto-bright flags.
  - **Unified Extraction**: Replace separate methods with a single `read_raw(options)` FFI wrapper returning `Result<ImageBuffer, LibRawError>`, where `ImageBuffer { data: Vec<T>, width, height, channels }`. This encapsulates the buffer with its spatial bounds, preventing dimensional mismatch bugs. Use strict `// SAFETY:` justifications.
  - **Rust ISP Engine (LUT-accelerated)**:
    - Normalization: **Do not manually subtract black**. Treat the `u16` buffer from LibRaw as a clean, normalized linear array spanning `0..=65535`.
    - LUT Generation: Precalculate Exposure, S-Curve, and OETF into a 1D Lookup Table mapping `0..=65535` to `u8`. This delivers `O(1)` cache-coherent evaluation per pixel.
    - Strict Bounds: The LUT generator must rigidly clamp multiplier outputs to prevent `f32 -> u8` overflows.

## 3. Out of Scope (Deferred Tasks & Tech Debt)
- **Complex White Balance (Temp/Tint) & ACEScg**: Translating XMP `Temperature/Tint` to XYZ matrices is deferred.
- *Binding Trigger*: Add to `TECH-DEBT.md` as **TD-023**. Fields: Title: Full ACEScg Color Science & Temp/Tint Adaptation. Binding Trigger: Tackle before `session-14`. Estimated LoC: ~400. Stop-gap location: `// TD-023` in `photohelper-export`. Consequence of Inaction: Sub-optimal color rendering for extreme white balance shifts.

## 4. Testing Plan
- **Unit Tests**:
  - Verify graceful rejection of invalid badge paths, and out-of-bounds scales.
  - Verify position collision handling triggers `DuplicateWatermarkPosition`.
  - Simulate a 1x1 image target to assert auto-scale zero-clamping logic correctly bounds to 1px.
- **Integration Tests**:
  - Programmatically assert on the exported JPEG's color/luma statistics to verify the ISP tone-mapping actively modified the image.
  - Assert end-to-end rendering of multiple distinct badges at different positions.

## 5. Synchronization Compliance
- Update `SESSION-STATE.md`: "Component progress" table, "Last session", "Next action", and "Status" fields.
- Update `README.md` and CLI documentation with the new `--badge` key-value syntax.

## 6. Checkpoints
- Plan Review (Round 1 & 2)
- Implementation
- Session-End review
