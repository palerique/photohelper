# Session 10 Plan — Pipeline Orchestration via the `run` Subcommand
**Branch**: `session-10/run-pipeline`
**Date**: 2026-05-31
**Status**: v1 — planning (Drafted following exploration & alignment on clarifying questions)

---

## Session goal

Implement the orchestrating pipeline subcommand `run` to sequentially chain and coordinate the `ingest → cull → develop → export` stages of `photohelper`. This allows executing the entire photo ingestion, scoring, sidecar generation, and JPEG rendering workflow in a single unified command, with absolute consistency in the catalog database resolution, robust parameter propagation, and strict error boundaries.

---

## What will exist by end of session

### 1. Unified `photohelper run` Subcommand CLI
- Define a new command args struct `RunArgs` in `crates/photohelper-cli/src/commands/run.rs`.
- `RunArgs` will parse the input directory `path` and require an export output directory `--output` / `-o`.
- It will also accept and propagate arguments for all pipeline stages:
  - **Ingest**: `--recursive` (bool, default true), `--strict` (bool, default false).
  - **Develop / Sidecar Settings**:
    - Exposure and color parameters: `--exposure`, `--temp`, `--tint`, `--contrast`, `--highlights`, `--shadows` (propagated exactly to develop stage).
    - Metadata flags: `--lr-rating` (bool, default false), `--lr-label` (bool, default false), `--lr-keywords` (bool, default false), `--all-lr` (bool, default false).
    - Custom localized label strings: `--lr-label-red` (String, default "Red"), `--lr-label-green` (String, default "Green").
  - **Export Options**:
    - `--long-edge` (Option<u32>).
    - `--quality` (u8, default 80).
    - `--watermark` (Option<String>).
    - `--watermark-position` (CliWatermarkPosition, default bottom-left).
    - `--min-rating` (u8, default 3).
    - `--force` (bool, default false) - propagated to both develop and export stages.

### 2. Automatic Catalog Sync & Verification
- If `cli.catalog` is `None` at start:
  - Resolves `<path>` to its absolute canonical path via `std::fs::canonicalize`.
  - Dynamically sets `cli.catalog = Some(canonical_path.join(".photohelper").join("catalog.db"))`.
  - This ensures that all four subcommands called under `run` operate on the exact same database.

### 3. Strict Sequential Orchestration Execution
- Sequential execution flow in `photohelper-cli`'s main controller:
  1. **Stage 1 (Ingest)**: Call `run_ingest` with `IngestArgs`.
  2. **Stage 2 (Cull)**: Query manifest NIMA model and call `run_cull` with `CullArgs`.
  3. **Stage 3 (Develop)**: Call `run_develop` with `DevelopArgs`.
  4. **Stage 4 (Export)**: Call `run_export` with `ExportArgs`.
- **Strict Halt Execution**: If any stage returns a non-zero exit code (or propagates an `Err`), execution terminates immediately, and that specific exit code or error is bubbled up to `main.rs` to prevent any silent failures on earlier steps.

---

## What is explicitly OUT OF SCOPE (deferred TDs)

All existing tech debts remain active but out of scope for this pipeline synchronization session:

| TD | Trigger | Rationale for deferral |
|---|---|---|
| TD-012 | When develop does custom demosaic | Standard slider-based pipeline is sufficient for metadata syncing and run subcommand. |
| TD-017 | Next session optimizing clustering | Union-find grouping is fast and stable for current library sizes. |
| TD-018 | Dedup float-to-int BLOB quantization | Storage for CLIP embeddings is not a bottleneck today. |

---

## Stop-gap declarations

No new stop-gaps are introduced.

---

## Verification & Testing Strategy

### 1. Integration Tests in `crates/photohelper-cli/tests/cli.rs`
- **Happy Path End-to-End Pipeline**:
  - Set up a temporary directory with a valid raw fixture (e.g. synthetic CR3) and a temp export output directory.
  - Run `photohelper run <input-dir> --output <output-dir> --all-lr`.
  - Verify that:
    1. Ingest catalogs the photo.
    2. Cull scores the photo.
    3. Develop generates an XMP sidecar containing star ratings and keywords.
    4. Export reads the sidecar rating, validates it against `min-rating`, and exports the JPEG output successfully.
- **Fail-Fast Pipeline Abort**:
  - Run `run` on an empty folder (which causes Ingest to return an error/warning).
  - Verify that subsequent stages are not called, and the proper exit code is returned.
- **Option Propagation Verification**:
  - Run `run` with custom settings (e.g., `--quality 95`, `--watermark "Photohelper"`, `--lr-label-red "Vermelho"`).
  - Verify the resulting XMP sidecar uses `"Vermelho"` for low-scored ratings, and the exported JPEG has correct quality and watermark settings.

### 2. Verification command
- Run `just ci` to guarantee all 248 existing tests plus new integration pipeline tests compile, format, and pass cleanly.

---

## Synchronization Compliance

All references, paths, variable types, and exit codes defined or used in this plan strictly adhere to `docs/quality-assurance.md § State & Context Synchronization Discipline`. High-density, precise file locations and module bindings are guaranteed.
