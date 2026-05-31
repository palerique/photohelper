# Session 12 - Round 1 Code Review

The eight-agent review suite has executed its Round 1 analysis on the Session 12 implementation (Standalone ISP, Image Watermarks, Native Export Rendering).

## Critical Findings
1. **Unconditional Out-of-Bounds Panic on Monochrome RAWs (Fixed)**: The pixel conversion loop assumed `channels >= 3` unconditionally, leading to out-of-bounds panics on monochrome RAWs. Remediation applied: explicit validation check added to `export_photo`.
2. **Missing RAII Guard Allows C-Heap Leaks on Panic (Fixed)**: `libraw_dcraw_make_mem_image` allocation lacked an RAII drop guard for `libraw_dcraw_clear_mem`, causing memory leaks if unwinding occurred. Remediation applied: `ProcessedImageGuard` created and implemented in `ffi.rs`.
3. **Fail-open condition in watermark application**: Out-of-bounds watermarks issue a warning and continue, resulting in unwatermarked images. Must fail explicitly with `ExportError::WatermarkOmitted`.
4. **Encapsulation Breach in ImageBuffer**: Fields are `pub`, bypassing spatial invariants. Needs fallible constructor.
5. **False safety guarantee regarding pointer alignment**: Unaligned pointer cast for `copy_nonoverlapping`. Needs byte-wise cast.

## High Findings
1. **Silent swallow of export failures**: `run_export` returns `Ok(0)` even if `total_failures > 0` under non-strict mode. Needs to return `EX_PARTIAL_FAIL` instead.
2. **Missing Test Coverage for Edge Cases, Watermarks, and Luma Statistics**: Needs unit tests for `BadgeLoadFailed` and `DuplicateWatermarkPosition`, plus integration tests for luma averages and pixel comparisons.
3. **Missing Documentation and Workspace Ledger Updates**: `SESSION-STATE.md`, `README.md`, `TECH-DEBT.md` need updates.
4. **$O(N)$ Redundant Disk I/O & Decoding Bottleneck**: Badge PNG decoded per photo instead of preloaded and shared.

## Medium/Low Findings
1. **Loss of critical execution context via broad panic catch**: Catch block on `compress_jpeg` drops panic payloads.
2. **Weak Invariant Expression (Primitive Obsession)**: CLI boundaries on `quality` and `long_edge` not represented in type system.
3. **Unchecked Arithmetic Overflow Risk**: Multiplication in `expected_size` calculation for `libraw` buffer could overflow `usize`.
4. **Platform-specific conditional compilation cluttering control flow**: Refactor target key generation.

## Next Steps
Several of the critical fixes (Bounds checking and RAII Guards) have already been preemptively applied to ensure `cargo test` stability and prevent memory leaks. The remaining findings should be resolved in the next session loop.
