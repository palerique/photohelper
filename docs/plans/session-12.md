# Session 12 Plan — Native ISP Engine & Dynamic Watermarking

## 1. Goal
Elevate `photohelper export` from a proxy-renderer to a true standalone Image Signal Processor (ISP) by establishing a 16-bit linear pipeline (DN-033), and implement dynamic, auto-scaling image-based watermarks (DN-036).

## 2. Deliverables
- **DN-036: Dynamic Image Watermarks**
  - **CLI Updates**: Add `--badge <PATH>:<POSITION>[:SCALE_PERCENT]` to the `export` subcommand.
  - **Auto-Scaling**: If `SCALE_PERCENT` is omitted, the engine automatically calculates a harmonious default scale (e.g., 5% of the target long edge).
  - **Validation**: Fail fast (throw an error) if the user provides multiple badges targeted at the exact same `WatermarkPosition`.
  - **Rendering**: Utilize `tiny-skia`'s native PNG decoding and affine transforms to composite the badge onto the export image.

- **DN-033: Standalone ISP Pipeline (Phase 1)**
  - **C-Shim Updates**: Extend `cpp/photohelper_libraw_shim.c` with setters for LibRaw's `output_bps` (set to 16-bit), `gamm` (set to `[1.0, 1.0]` for linear), and `no_auto_bright` (to disable LibRaw's auto-exposure).
  - **16-bit Linear Extraction**: Introduce `photohelper_raw::decode::read_raw_linear_16bit`, replacing the default 8-bit sRGB extraction.
  - **Rust ISP Engine**: Create a new pipeline in `photohelper-export` that takes the 16-bit linear sensor data and applies:
    - **Exposure**: True linear multiplier derived from `crs:Exposure2012`.
    - **Tone Mapping**: Custom S-curves applied to the linear data to emulate `crs:Contrast2012`, `crs:Highlights2012`, and `crs:Shadows2012`.
    - **OETF (Gamma)**: Convert the fully-graded linear data back into standard 8-bit sRGB for MozJPEG encoding and watermarking.

## 3. Out of Scope (Deferred)
- **Complex White Balance (Temp/Tint)**: Translating Lightroom's `Temperature/Tint` slider values into accurate `cam_mul` multipliers requires full chromatic adaptation (CAT02) and camera-specific XYZ calibration matrices. For Phase 1, we will apply the *camera's as-shot white balance* via LibRaw, deferring `Temp/Tint` sliders to a dedicated color-science session.
- **Full ACEScg Color Space**: While the data will be processed in linear RGB, migrating the entire pipeline to the ACEScg wide-gamut working space is deferred until the foundational linear pipeline lands securely.

## 4. Testing Plan
- **Unit Tests**: Ensure the CLI parser correctly rejects duplicate badge positions and defaults the scale when omitted.
- **Unit Tests**: Verify the Rust ISP tone curves and linear-to-sRGB OETF mathematical correctness.
- **Integration Tests**: Run `just ci` to ensure `run_export` respects the new `--badge` flag and correctly outputs JPEGs via the 16-bit linear pipeline.

## 5. Discoveries and Assumptions
- **Discovery**: `tiny-skia` v0.12.0 natively compiles with `decode_png`, removing the need to add external dependencies like `image`.
- **Assumption**: A simple mathematically-defined S-curve will be used to approximate Lightroom's contrast and tonal mapping on the linear data. This provides immediate value while establishing the architecture for future precise LUTs or profiling.
- **Synchronization Compliance**: All modifications and struct names (e.g., `ExportOptions`, `ImageBadge`) adhere strictly to `docs/quality-assurance.md § State & Context Synchronization Discipline`. Any deferred tasks (ACEScg, Temp/Tint) will be logged in `TECH-DEBT.md`.

## 6. Checkpoints
- Plan Review (Round 1 & 2)
- Implementation
- Session-End review
