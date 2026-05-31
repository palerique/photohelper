# Session 10 Plan — Pipeline Orchestration via the `run` Subcommand
**Branch**: `session-10/run-pipeline`
**Date**: 2026-05-31
**Status**: v3 — ready (R2 remediation complete)

---

## Session goal

Implement the orchestrating pipeline subcommand `run` to sequentially chain and coordinate the `ingest → cull → dedup → develop → export` stages of `photohelper`. This allows executing the entire photo ingestion, scoring, clustering, sidecar generation, and JPEG rendering workflow in a single unified command, with absolute consistency in the catalog database resolution, robust parameter propagation, and strict error boundaries.

---

## What will exist by end of session

### 1. Unified `photohelper run` Subcommand CLI
- Define a new command args struct `RunArgs` in `crates/photohelper-cli/src/commands/run.rs`.
- `RunArgs` will parse the input directory `<path>` and require an export output directory `--output` / `-o`.
- **Explicit Parameter Mapping**: `RunArgs` will explicitly define and hold the shared global flags (e.g. `--strict`, `--force`). Instead of `#[command(flatten)]` (which causes duplicate argument panics), `RunArgs` will instantiate `IngestArgs`, `CullArgs`, `DevelopArgs`, and `ExportArgs` manually, propagating the shared flags and context transparently to each.
- Ensure all custom localized label strings retain their `env` macro attributes.

### 2. Automatic Catalog Sync & Verification
- If `cli.catalog` is `None` at start, resolves `<path>` to its absolute canonical path, safely stripping Windows UNC paths via `dunce::canonicalize`.
- Constructs a strongly-typed `ValidatedIO` boundary asserting that `canonicalize(output) != canonicalize(input)` and `canonicalize(output)` does not start with `canonicalize(input)`.
- Instantiates a strongly-typed `PipelineContext` holding the verified SQLite catalog connection (with `PRAGMA journal_mode=WAL` to reduce lock contention) and `Arc<Nima>`.

### 3. Horizontal Pipeline Orchestration
- Execution will proceed via a horizontal bulk processing pipeline to preserve machine learning batching and SQLite transaction efficiency. The order is:
  1. `run_ingest`
  2. `run_cull`
  3. `run_dedup` (Required for cluster ID mapping in `develop` keywords)
  4. `run_develop` (In-memory output metadata passed to export)
  5. `run_export` (Consumes in-memory metadata rather than re-reading XMP from disk)
- **Fail-Fast & Idempotency Contract**:
  - All stages mandate idempotency: `INSERT ... ON CONFLICT (id) DO NOTHING` to ensure real DB faults aren't swallowed by broad `OR IGNORE` clauses.
  - Non-strict mode (`--strict false`): Errors in pipeline stages are caught, logged with trace-friendly IDs, accumulated, and execution proceeds where possible. The command returns a non-zero exit code at the very end if any errors occurred.
  - Strict mode (`--strict true`): Uses `try_for_each` or short-circuit evaluation to fail the pipeline immediately upon the first `Err`.

---

## What is explicitly OUT OF SCOPE (deferred TDs)

All existing tech debts remain active but out of scope for this pipeline synchronization session:

| TD | Trigger | Rationale for deferral |
|---|---|---|
| TD-012 | When develop does custom demosaic | Standard slider-based pipeline is sufficient for metadata syncing and run subcommand. |
| TD-017 | n > 10K photos or user request for faster/lower-memory clustering | Union-find grouping is fast and stable for current library sizes. |
| TD-018 | First user request for int8/f16 quantization or storage-size complaint | Storage for CLIP embeddings is not a bottleneck today. |

---

## Stop-gap declarations

No new stop-gaps are introduced.

---

## Verification & Testing Strategy

### 1. Integration Tests in `crates/photohelper-cli/tests/cli.rs`
- **Happy Path End-to-End Pipeline**:
  - Set up a temporary directory with a valid raw fixture (e.g. synthetic CR3).
  - Verify auto-creation of a missing `--output` directory.
  - Run `photohelper run <input-dir> --output <output-dir> --all-lr`.
  - Verify proper execution of ingest, cull, dedup, develop, and export.
- **Strict-Mode Mid-Pipeline Abort & Idempotency**:
  - Pre-sort inputs or construct deterministic execution passes with a mix of valid and corrupt fixtures.
  - Run `run` with `--strict`.
  - Verify that the pipeline fails cleanly upon hitting the corrupt fixture mid-pipeline without leaving torn state.
- **Input/Output Collision Boundary**:
  - Verify that providing an output path inside the input path throws an explicit validation error.
  - Verify that providing an output path exactly equal to the input path throws an explicit validation error.
- **Negative Behavioral Testing (`--min-rating`)**:
  - Verify that a raw fixture evaluated to a rating below `--min-rating` is correctly skipped and NOT exported.
- **Option Propagation Verification**:
  - Run `run` with custom settings (e.g., `--quality 95`, `--watermark "Photohelper"`, `--lr-label-red "Vermelho"`).
  - Verify the resulting XMP sidecar uses `"Vermelho"` for low-scored ratings, and the exported JPEG has correct quality and watermark settings.

### 2. Verification command
- Run `just ci` to guarantee all existing tests plus new integration pipeline tests compile, format, and pass cleanly.

---

## Synchronization Compliance

All references, paths, variable types, and exit codes defined or used in this plan strictly adhere to `docs/quality-assurance.md § State & Context Synchronization Discipline`. High-density, precise file locations and module bindings are guaranteed.
