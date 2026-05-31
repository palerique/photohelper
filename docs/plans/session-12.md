# Session 12 Plan — Native ISP Engine & Dynamic Watermarking

## 1. Goal
Elevate `photohelper export` from a proxy-renderer to a true standalone Image Signal Processor (ISP) by establishing a 16-bit linear pipeline (DN-033), and implement dynamic, auto-scaling image-based watermarks (DN-036).

## 2. Deliverables
- **Workspace Cleanup**:
  - Remove leftover discovery files: `crates/photohelper-export/src/bin.rs` and `crates/photohelper-export/src/pixmap_test.rs`.
  - Revert the unstaged rogue `uuid` dependency addition in `crates/photohelper-sidecar/Cargo.toml`.

- **DN-036: Dynamic Image Watermarks**
  - **CLI Updates**: Add paired arguments `--badge <PATH>` and `--badge-pos <POSITION>` (and optionally `--badge-scale <PERCENT>`) instead of a colon-delimited string to prevent Windows path collisions (`C:\`).
  - **Auto-Scaling**: If `--badge-scale` is omitted, automatically calculate a default scale (e.g., 5% of the target long edge), enforcing a strict clamp of `max(computed_size, 1.0)` to prevent zero-pixel affine matrix panics.
  - **O(1) Validation**: Parse CLI options directly into a `HashMap<WatermarkPosition, Badge>` to detect duplicate positions instantly. Fail fast if ANY watermark (text or image) shares the same position. Explicitly define `ExportError::DuplicateWatermarkPosition` and `ExportError::InvalidBadgeFormat`.
  - **Rendering**: Utilize `tiny-skia` (via `png` crate) to load badges. Composite badges linearly or securely after OETF. Use a custom affine blending loop onto the target RGB buffer rather than coercing the massive 24MP image into an RGBA `Pixmap`, saving memory.

- **DN-033: Standalone ISP Pipeline (Phase 1)**
  - **Data Boundary**: Expand `ExportOptions` with a `ToneMappingOptions` struct carrying XMP edits from the CLI layer into `photohelper-export`.
  - **C-Shim Updates**: Extend `cpp/photohelper_libraw_shim.c` with setters for `output_bps` (16-bit), `gamm` (`[1.0, 1.0]`), and `no_auto_bright` (1). Update the header comment to document the shift to a state-mutating API.
  - **16-bit Linear Extraction**: Introduce `photohelper_raw::decode::read_raw_linear_16bit` as an *additional* method exclusively for export, preserving `read_raw_rgb` to prevent breaking the `photohelper-ai` processing pipeline. All new `unsafe` FFI blocks must carry strict `// SAFETY:` justifications.
  - **Rust ISP Engine (LUT-accelerated)**:
    - Normalization: Extract `imgdata.color.maximum` and `imgdata.color.black` to anchor the 16-bit array to a strict normalized `0.0..=1.0` or `0..=65535` baseline before tone mapping.
    - LUT Generation: Precalculate the Exposure, S-Curve tone mapping, clipping, and OETF gamma pass into a single 1D Lookup Table mapping `0..=65535` to `u8`. This condenses millions of transcendental math operations into an `O(N)` cache-coherent map.
    - Strict Clipping: Ensure the LUT generation explicitly clamps values exceeding `1.0` (or `65535`) to prevent `f32` -> `u8` integer overflow panics.

## 3. Out of Scope (Deferred Tasks & Tech Debt)
- **Complex White Balance (Temp/Tint) & ACEScg**: Translating Lightroom's `Temperature/Tint` sliders to XYZ matrices is too complex for this session. We will rely on LibRaw's as-shot white balance. The ACEScg colorspace and Temp/Tint integration are deferred.
- *Binding Trigger*: Added to `TECH-DEBT.md` (TD-015: "Full ACEScg Color Science & Temp/Tint Adaptation") with an explicit constraint to tackle before `session-14`, estimated at ~400 LoC.

## 4. Testing Plan
- **Unit Tests**:
  - Verify graceful rejection of invalid badge paths, unreadable PNGs, non-numeric scales, and out-of-bounds scales.
  - Supply extreme exposure multipliers to the LUT generator and verify integer bounds are clamped without panics.
- **Integration Tests**:
  - Programmatically assert that `photohelper export` produces a valid JPEG file with correct dimensions.
  - Verify that watermark composite calculations respect the raw image's orientation flag.
  - Verify successful end-to-end rendering of multiple distinct badges at different positions.

## 5. Synchronization Compliance
- Update the `SESSION-STATE.md` "Component progress" table to reflect `photohelper-export` native ISP capabilities.
- Update `README.md` and CLI documentation with the new `--badge` and `--badge-pos` usage instructions.
- Ensure all variable names and domain terms match `docs/quality-assurance.md`.

## 6. Checkpoints
- Plan Review (Round 1 & 2)
- Implementation
- Session-End review
