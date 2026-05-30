# session-08 plan-review round 2

> Per docs/quality-assurance.md § Plan-review protocol.

## gp findings

- **Finding**: Forbidden Unsafe Code in `photohelper-export` Crate
  - **Finding ID**: da9bf84ceab2d1e281e8bcaebd461f818f30a733
  - **Location**: [session-08.md:116](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L116)
  - **Severity**: CRITICAL
  - **Problem**: The global workspace prevents `unsafe` code by default, but direct FFI bindings for `mozjpeg` or custom FFI wrapping in `photohelper-export` require explicit permissions. Without a crate-level `unsafe_code = "allow"` override in `Cargo.toml`, compilation will fail.
  - **Remediation**: Add a crate-level exception inside `crates/photohelper-export/Cargo.toml` and state clippy exceptions if necessary.

- **Finding**: Unused Crate Dependency `photohelper-core` Triggers Compiler Error
  - **Finding ID**: 2ed4d98a80ace26d8a4b273feb5b39d6c18fd956
  - **Location**: [session-08.md:120](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L120)
  - **Severity**: HIGH
  - **Problem**: Referencing `photohelper-core` in `crates/photohelper-export/Cargo.toml` without actively consuming its items triggers an unused dependency clippy/compiler warning, which fails the build under `-D warnings`.
  - **Remediation**: Either remove `photohelper-core` from dependencies of `photohelper-export` if it is unused, or actively consume a shared model (e.g. types/errors).

- **Finding**: Unspecified S2 Stop-Gap Tech-Debt Ledger Entry
  - **Finding ID**: dc5ffdaaf29f50c919f3a011d9b889780cabdd29
  - **Location**: [session-08.md:84](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L84)
  - **Severity**: CRITICAL
  - **Problem**: The plan declares Stop-Gap `S2` mapping to `TD-025` for FFI bindings, but the correspond tech debt item does not actually exist in the global ledger (`TECH-DEBT.md`). This violates state-tracking invariants.
  - **Remediation**: Append a detailed `TD-025` entry to `TECH-DEBT.md` under S2 stop-gap tracking, specifying S2, a binding trigger, and remediation steps.

## arch findings

- **Finding**: Unsafe FFI Memory Corruption in Direct `mozjpeg` C-Bindings
  - **Finding ID**: d12c36b2101c5817106b900f3667440dc1979e44
  - **Location**: [session-08.md:34](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L34)
  - **Severity**: HIGH
  - **Problem**: Direct FFI with `mozjpeg` requires passing raw scanline rows as `*const *const u8`. Trying to pass a flat 1D raw pointer can corrupt memory and cause segfaults.
  - **Remediation**: Utilize `mozjpeg` high-level safe API (`Compress::set_scanlines` or `Compress::write_scanlines`) or construct pinned row-pointer arrays properly in Rust.

- **Finding**: Concurrency Race Hazard in Filename Collision Resolution
  - **Finding ID**: 7a0cb7e674c683444286f3353a8e927e8c6f191e
  - **Location**: [session-08.md:60](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L60)
  - **Severity**: HIGH
  - **Problem**: Resolving path collisions inside the Rayon loop using file-existence checks introduces time-of-check-to-time-of-use (TOCTOU) race conditions. Multiple threads might simultaneously decide that a filename is free and overwrite each other.
  - **Remediation**: Calculate unique targets upfront on the main thread, producing an immutable `HashMap<PathBuf, PathBuf>` of source paths to fully-suffixed unique output paths, which is then safely read inside the Rayon parallel loop.

## rev findings

- **Finding**: Non-existent `.demultiply()` Method on `tiny_skia::PremultipliedColorU8`
  - **Finding ID**: 8213d0aba70a0cabefb1edc098450d8c0d07a825
  - **Location**: [session-08.md:34](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L34)
  - **Severity**: CRITICAL
  - **Problem**: The plan claims we will iterate over each pixel and call `.demultiply()` on each pixel channel. `PremultipliedColorU8` does not expose any `.demultiply()` method. Instead, `tiny_skia::Pixmap` has `take_demultiplied()` which demultiplies the entire pixel buffer into a new `Vec<u8>`.
  - **Remediation**: Update the plan to consume the `Pixmap` via `take_demultiplied()` and drop the 4th alpha byte (making it 3-channel RGB) rather than manual per-pixel struct mutation.

- **Finding**: Division-by-Zero Panic in Premultiplied Alpha Demultiplication
  - **Finding ID**: 5ab2348fc351dccd99dc28442ccb061449e8bf2b
  - **Location**: [session-08.md:33](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L33)
  - **Severity**: HIGH
  - **Problem**: If the plan attempts manual demultiplication of color channels (RGB) by dividing by the alpha value, a fully transparent pixel (alpha = 0) will trigger a division-by-zero panic in Rust.
  - **Remediation**: When implementing manual pixel demultiplication, check if alpha is 0 and output 0, or utilize `tiny_skia::Pixmap::take_demultiplied()` which already handles zero-alpha safely.

- **Finding**: Process Crashes from Uncaught C-Level MozJPEG Errors
  - **Finding ID**: 9c6d138e7837f3e6df25afa5018a00e567ee013d
  - **Location**: [session-08.md:37](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L37)
  - **Severity**: HIGH
  - **Problem**: MozJPEG C-bindings may trigger internal errors (such as out-of-memory or corrupted buffers) which can bubble up as uncatchable panics or process crashes.
  - **Remediation**: Wrap the MozJPEG compression step in a thread-safe error-catching boundary, mapping MozJPEG error codes or catching unwind panics to return `ExportError`.

- **Finding**: Missing Validation of XMP Rating Values Extracted From Disk
  - **Finding ID**: 75bcfb5da7d139f8a9c03faf7f131171538e79ea
  - **Location**: [session-08.md:49](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L49)
  - **Severity**: HIGH
  - **Problem**: A rating value read from an XMP sidecar may contain invalid values (e.g. out of `-1..=5` bounds, or extremely high/low integers) due to file corruption or external modification, breaking rating-evaluation invariants.
  - **Remediation**: Explicitly validate and clamp extracted XMP ratings to `-1..=5` before utilizing them in evaluation.

## type findings

- **Finding**: Loose Type Safety on Ratings in `export_photo` Signature
  - **Finding ID**: b86dd0fb19c5bb0d5134ad4d4b5ecccf22a19fc0
  - **Location**: [session-08.md:18](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L18)
  - **Severity**: MEDIUM
  - **Problem**: The function `export_photo` accepts rating as `Option<i32>`, leading to primitive obsession and possible out-of-bounds ratings.
  - **Remediation**: Utilize a strongly-typed domain representation like `Option<Rating>` or `Option<XmpRating>` to enforce constraints.

- **Finding**: Mismatched Floating-Point Types for NIMA Score
  - **Finding ID**: e9882d45236073579b4b6b56c4d33cf05f87b601
  - **Location**: [session-08.md:18](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L18)
  - **Severity**: MEDIUM
  - **Problem**: `export_photo` defines `nima_score` as `Option<f64>`, whereas other parts of the application and the catalog database model NIMA scores using `f32` (or a strongly-typed wrapper). This mismatch causes type conversions.
  - **Remediation**: Standardize on `f32` or use a strongly-typed wrapper `Option<NimaScore>` for the score.

- **Finding**: Data Flow Gap in Central `export_photo` Signature
  - **Finding ID**: 22b727b9b8816f2590a4c1e05750471153aa3b5f
  - **Location**: [session-08.md:18](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L18)
  - **Severity**: HIGH
  - **Problem**: The `export_photo` signature takes separate parameters for raw path, NIMA score, and XMP rating, but this is a complex, disconnected data flow that ignores high-level abstractions.
  - **Remediation**: Pass a strongly-typed struct wrapping metadata, or encapsulate options inside a single structured input object.

## sfh findings

- **Finding**: Database Query Upfront Filters "Qualifying" Photos, Skipping XMP Ratings
  - **Finding ID**: 2e7073766916ec1a9dadf49d9960040f422e9df2
  - **Location**: [session-08.md:54](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L54)
  - **Severity**: CRITICAL
  - **Problem**: Upfront database query filtering by rating or NIMA score on the SQL side will miss photos whose database ratings are low but have high manual ratings in disk-based XMP sidecars. This leads to silent omission of photos that should have been exported.
  - **Remediation**: Fetch all active database rows, and evaluate the final rating (checking XMP first, then falling back to database NIMA score) inside the worker processing loop.

- **Finding**: Malformed/Empty Sidecar XML Causes Silent Fallback to NIMA
  - **Finding ID**: 0db7e4e52c82bb0f3c084ff3124f466f9fbc7fde
  - **Location**: [session-08.md:47](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L47)
  - **Severity**: HIGH
  - **Problem**: If an XMP sidecar is corrupt or empty, parser errors might be silently swallowed, leading to a silent fallback to NIMA catalog rating instead of reporting an error.
  - **Remediation**: If an XMP file exists but fails to parse, treat it as a per-photo export failure rather than a silent fallback.

- **Finding**: Silently Returning Exit Code 0 on Partial Export Failures
  - **Finding ID**: 229ed499477abe9a67431b34329fed90b79ddec3
  - **Location**: [session-08.md:62](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L62)
  - **Severity**: HIGH
  - **Problem**: When `--strict` is not enabled, if a subset of images fail to export (e.g., 5 out of 10), the command might return exit code 0, silently hiding issues in CI or automated scripts.
  - **Remediation**: If any export error occurs, output a clear stderr warning report, and ensure that if all photos fail to export, or if `--strict` is set, a non-zero exit code is returned.

## com findings

- **Finding**: Incomplete Watermark Position Specifications and Coordinate Equations in D2
  - **Finding ID**: c3616fa81d06d40d40e7cb13bbdbb7205314ba46
  - **Location**: [session-08.md:108](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L108)
  - **Severity**: HIGH
  - **Problem**: Section `D2` defines coordinate equations for `bottom-left` but leaves `top-right` completely unspecified. This makes the layout system incomplete and ambiguous.
  - **Remediation**: Document the exact mathematical coordinates for `top-right` positioning ($x_{pos} = W - D - W_{text}$, $y_{pos} = D$).

- **Finding**: Contradiction in Padding Calculation: Respective Edge vs. Long Edge
  - **Finding ID**: 2b2df2e2f60d3968245bff5682d69112a7617723
  - **Location**: [session-08.md:29](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L29)
  - **Severity**: HIGH
  - **Problem**: Line 29 claims padding is "1.5% of the respective edge", but other sections claim padding is calculated relative to the long edge. A short edge on panorama could make padding extremely small, breaking symmetry.
  - **Remediation**: Standardize padding as 1.5% of the scaled long edge, ensuring uniform spacing regardless of edge.

- **Finding**: Subpixel Text Offset Causes Blurring and Degrades Visual Quality
  - **Finding ID**: 282b6795d83df9c43206c3845bd2fbf8da0a5933
  - **Location**: [session-08.md:30](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L30)
  - **Severity**: MEDIUM
  - **Problem**: Applying a fractional drop-shadow offset like 1.5px triggers subpixel interpolation in 2D rasterization, causing text blurring and reducing watermark legibility.
  - **Remediation**: Mandate integer-aligned pixel offsets (`1px` or `2px`) to ensure crisp text.

## test findings

- **Finding**: Pipeline Data Flow Contradiction and Loss of `PhotoId`
  - **Finding ID**: 752245db6f17b2c4e382aacae1e6ee8020e1b18c
  - **Location**: [session-08.md:147](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L147)
  - **Severity**: HIGH
  - **Problem**: Converting the database rows to `HashMap<PathBuf, f64>` of NIMA scores discards `PhotoId`. This makes it impossible to query the catalog ID to resolve unique-suffix file naming collisions inside Rayon.
  - **Remediation**: Preserve `DevelopRow` (containing `photo_id`, `path`, `nima_score`, `rating`, etc.) and pass the vector of rows directly to Rayon iterator.

- **Finding**: Division-by-Zero Risk in Aspect Ratio Calculations
  - **Finding ID**: f6a74e40c42caf37d1b7f970999351b3341d2c94
  - **Location**: [session-08.md:20](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L20)
  - **Severity**: HIGH
  - **Problem**: Proportional scaling calculates target width/height using division by the original dimensions. A corrupted or malicious RAW file with width/height of 0 will trigger a division-by-zero crash.
  - **Remediation**: Add explicit validation of input dimensions, returning `ExportError::InvalidDimensions` if width or height is 0.

- **Finding**: Collision Behaviors for Existing Files in Target Directory
  - **Finding ID**: 87dd33799b9132a328d6856e42f5e45476ca926e
  - **Location**: [session-08.md:60](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L60)
  - **Severity**: HIGH
  - **Problem**: The plan does not specify how to handle cases where the output JPEG file already exists in the destination directory. Will it silently overwrite, skip, or fail?
  - **Remediation**: Specify a standard CLI parameter `--force` or similar, documenting that by default, existing files are skipped with a warning, unless `--force` is specified.

## simp findings

- **Finding**: Over-engineered Text Layout with `cosmic-text` and Font System Scanning
  - **Finding ID**: fdf06921e82db41cb260ca5811d294cc15acf8c1
  - **Location**: [session-08.md:27](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L27)
  - **Severity**: HIGH
  - **Problem**: Instantiating `cosmic-text`'s `FontSystem` without arguments scans the entire host system for fonts, causing extremely slow I/O bottlenecks. Since we bundle the font via `include_bytes!`, system scanning is redundant.
  - **Remediation**: Initialize `FontSystem::new_with_locale_and_db` with an empty font database and manually load our embedded font to bypass system scanning.

- **Finding**: Redundant CPU Division in Pixel Demultiplication
  - **Finding ID**: 7f0483f2819b4341b914b8afb4f342e3d7593502
  - **Location**: [session-08.md:34](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L34)
  - **Severity**: HIGH
  - **Problem**: Demultiplying color channels when alpha is 255 (the vast majority of pixels) is a waste of CPU cycles.
  - **Remediation**: Fast-path the demultiplication loop: if alpha is 255, skip the math and directly copy the color channels.

- **Finding**: Lack of Fast-Path Optimization for Original-Resolution, Non-Watermarked Exports
  - **Finding ID**: ca401889ee488ec0011973750a28779b8dd247ad
  - **Location**: [session-08.md:129](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L129)
  - **Severity**: HIGH
  - **Problem**: If there is no `--long-edge` specified and no watermark, converting raw pixels to tiny-skia `Pixmap` only to extract them is a redundant overhead.
  - **Remediation**: Bypass `tiny-skia` entirely for original-resolution, un-watermarked images, feeding decoded pixels directly to MozJPEG.

- **Finding**: Exit Hang Bottleneck in Progress Heartbeat Thread
  - **Finding ID**: 3871b39262f8101945203f71745f1df24e51f967
  - **Location**: [session-08.md:61](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L61)
  - **Severity**: HIGH
  - **Problem**: Sleeping for 10 seconds inside a heartbeat loop blocks thread termination, causing the CLI tool to hang on exit until the 10-second timer expires.
  - **Remediation**: Use channel receiver timeouts, `Condvar::wait_timeout`, or a pool of shorter sleep durations with cooperative exit checks.

- **Finding**: Orphaned Temporary Files on Panic or Strict Cancellation
  - **Finding ID**: 17f093438e0caf2442490a1b46072033006b8d8b
  - **Location**: [session-08.md:58](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L58)
  - **Severity**: HIGH
  - **Problem**: Writing to `.tmp` files and deleting them on explicit failures will miss panics or abrupt cancellation, leaving orphaned `.tmp` files littered across the workspace.
  - **Remediation**: Implement a RAII `TempFileGuard` wrapper that automatically cleans up/deletes the target file in its `Drop` implementation unless explicitly committed.

- **Finding**: Over-engineered Watermark Coordinate Overflow Handling
  - **Finding ID**: 27ade3449dfc534dfcc6b8504a85f7fe3a1230f2
  - **Location**: [session-08.md:109](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L109)
  - **Severity**: MEDIUM
  - **Problem**: Dynamically scaling down or omitting watermarks when coordinates don't fit is complex.
  - **Remediation**: Keep it simple: omit watermarks entirely or log a warning if the watermark dimensions exceed the image dimensions.

- **Finding**: Missing Error Mapping for `Pixmap::new`
  - **Finding ID**: 3a164b8ac1121ace12f8e2ad36f27b89ca82801c
  - **Location**: [session-08.md:130](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L130)
  - **Severity**: HIGH
  - **Problem**: `tiny_skia::Pixmap::new` returns `None` if the width or height is zero or if an out-of-memory error occurs. Unwrapping this value will panic.
  - **Remediation**: Map `None` from `Pixmap::new` to a dedicated error variant `ExportError::AllocationFailed` or `ExportError::InvalidDimensions`.

## verification

```yaml
verification:
  docs/plans/session-08.md:
    - finding_id: da9bf84ceab2d1e281e8bcaebd461f818f30a733
      file: docs/plans/session-08.md
      line: 116
      present: yes
      retain: yes
      reason: "Forbidden Unsafe Code in `photohelper-export` Crate"
    - finding_id: 8213d0aba70a0cabefb1edc098450d8c0d07a825
      file: docs/plans/session-08.md
      line: 34
      present: yes
      retain: yes
      reason: "Non-existent `.demultiply()` Method on `tiny_skia::PremultipliedColorU8`"
    - finding_id: d12c36b2101c5817106b900f3667440dc1979e44
      file: docs/plans/session-08.md
      line: 34
      present: yes
      retain: yes
      reason: "Unsafe FFI Memory Corruption in Direct `mozjpeg` C-Bindings"
    - finding_id: 2ed4d98a80ace26d8a4b273feb5b39d6c18fd956
      file: docs/plans/session-08.md
      line: 120
      present: yes
      retain: yes
      reason: "Unused Crate Dependency `photohelper-core` Triggers Compiler Error"
    - finding_id: 2e7073766916ec1a9dadf49d9960040f422e9df2
      file: docs/plans/session-08.md
      line: 54
      present: yes
      retain: yes
      reason: "Database Query Upfront Filters \"Qualifying\" Photos, Skipping XMP Ratings"
    - finding_id: 752245db6f17b2c4e382aacae1e6ee8020e1b18c
      file: docs/plans/session-08.md
      line: 147
      present: yes
      retain: yes
      reason: "Pipeline Data Flow Contradiction and Loss of `PhotoId`"
    - finding_id: 22b727b9b8816f2590a4c1e05750471153aa3b5f
      file: docs/plans/session-08.md
      line: 18
      present: yes
      retain: yes
      reason: "Data Flow Gap in Central `export_photo` Signature"
    - finding_id: 7a0cb7e674c683444286f3353a8e927e8c6f191e
      file: docs/plans/session-08.md
      line: 60
      present: yes
      retain: yes
      reason: "Concurrency Race Hazard in Filename Collision Resolution"
    - finding_id: e9882d45236073579b4b6b56c4d33cf05f87b601
      file: docs/plans/session-08.md
      line: 18
      present: yes
      retain: yes
      reason: "Mismatched Floating-Point Types for NIMA Score"
    - finding_id: b86dd0fb19c5bb0d5134ad4d4b5ecccf22a19fc0
      file: docs/plans/session-08.md
      line: 18
      present: yes
      retain: yes
      reason: "Loose Type Safety on Ratings in `export_photo` Signature"
    - finding_id: 3871b39262f8101945203f71745f1df24e51f967
      file: docs/plans/session-08.md
      line: 61
      present: yes
      retain: yes
      reason: "Exit Hang Bottleneck in Progress Heartbeat Thread"
    - finding_id: 17f093438e0caf2442490a1b46072033006b8d8b
      file: docs/plans/session-08.md
      line: 58
      present: yes
      retain: yes
      reason: "Orphaned Temporary Files on Panic or Strict Cancellation"
    - finding_id: fdf06921e82db41cb260ca5811d294cc15acf8c1
      file: docs/plans/session-08.md
      line: 27
      present: yes
      retain: yes
      reason: "Over-engineered Text Layout with `cosmic-text` and Font System Scanning"
    - finding_id: 7f0483f2819b4341b914b8afb4f342e3d7593502
      file: docs/plans/session-08.md
      line: 34
      present: yes
      retain: yes
      reason: "Redundant CPU Division in Pixel Demultiplication"
    - finding_id: ca401889ee488ec0011973750a28779b8dd247ad
      file: docs/plans/session-08.md
      line: 129
      present: yes
      retain: yes
      reason: "Lack of Fast-Path Optimization for Original-Resolution, Non-Watermarked Exports"
    - finding_id: 2b2df2e2f60d3968245bff5682d69112a7617723
      file: docs/plans/session-08.md
      line: 29
      present: yes
      retain: yes
      reason: "Contradiction in Padding Calculation: Respective Edge vs. Long Edge"
    - finding_id: 27ade3449dfc534dfcc6b8504a85f7fe3a1230f2
      file: docs/plans/session-08.md
      line: 109
      present: yes
      retain: yes
      reason: "Over-engineered Watermark Coordinate Overflow Handling"
    - finding_id: 282b6795d83df9c43206c3845bd2fbf8da0a5933
      file: docs/plans/session-08.md
      line: 30
      present: yes
      retain: yes
      reason: "Subpixel Text Offset Causes Blurring and Degrades Visual Quality"
    - finding_id: c3616fa81d06d40d40e7cb13bbdbb7205314ba46
      file: docs/plans/session-08.md
      line: 108
      present: yes
      retain: yes
      reason: "Incomplete Watermark Position Specifications and Coordinate Equations in D2"
    - finding_id: 0db7e4e52c82bb0f3c084ff3124f466f9fbc7fde
      file: docs/plans/session-08.md
      line: 47
      present: yes
      retain: yes
      reason: "Malformed/Empty Sidecar XML Causes Silent Fallback to NIMA"
    - finding_id: 9c6d138e7837f3e6df25afa5018a00e567ee013d
      file: docs/plans/session-08.md
      line: 37
      present: yes
      retain: yes
      reason: "Process Crashes from Uncaught C-Level MozJPEG Errors"
    - finding_id: 3a164b8ac1121ace12f8e2ad36f27b89ca82801c
      file: docs/plans/session-08.md
      line: 130
      present: yes
      retain: yes
      reason: "Missing Error Mapping for `Pixmap::new`"
    - finding_id: f6a74e40c42caf37d1b7f970999351b3341d2c94
      file: docs/plans/session-08.md
      line: 20
      present: yes
      retain: yes
      reason: "Division-by-Zero Risk in Aspect Ratio Calculations"
    - finding_id: 229ed499477abe9a67431b34329fed90b79ddec3
      file: docs/plans/session-08.md
      line: 62
      present: yes
      retain: yes
      reason: "Silently Returning Exit Code 0 on Partial Export Failures"
    - finding_id: 87dd33799b9132a328d6856e42f5e45476ca926e
      file: docs/plans/session-08.md
      line: 60
      present: yes
      retain: yes
      reason: "Collision Behaviors for Existing Files in Target Directory"
    - finding_id: 5ab2348fc351dccd99dc28442ccb061449e8bf2b
      file: docs/plans/session-08.md
      line: 33
      present: yes
      retain: yes
      reason: "Division-by-Zero Panic in Premultiplied Alpha Demultiplication"
    - finding_id: 75bcfb5da7d139f8a9c03faf7f131171538e79ea
      file: docs/plans/session-08.md
      line: 49
      present: yes
      retain: yes
      reason: "Missing Validation of XMP Rating Values Extracted From Disk"
    - finding_id: dc5ffdaaf29f50c919f3a011d9b889780cabdd29
      file: docs/plans/session-08.md
      line: 84
      present: yes
      retain: yes
      reason: "Unspecified S2 Stop-Gap Tech-Debt Ledger Entry"
```
