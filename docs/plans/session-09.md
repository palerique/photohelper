# Session 09 Plan — Lightroom Classic Metadata Synchronization Improvements
**Branch**: `session-09/lightroom-sync-fixes`
**Date**: 2026-05-30
**Status**: v3 — planning (remediated and updated after Multi-Agent Round 2)

---

## Session goal

Address Lightroom Classic metadata synchronization gaps identified in **`BUG-001`** to ensure that `photohelper`'s ratings, color labels, and keywords sync reliably, clean, and safely into Adobe Lightroom Classic. We will provide smart CLI warnings to alert the user about empty metadata writes, improve parallel performance by avoiding log contention, provide single-source advice on bypass options, introduce robust type-safe localized color labels, implement an `mtime`-based external modification conflict shield, and author a comprehensive syncing guide.

---

## What will exist by end of session

### 1. Smart CLI Metadata Warnings & Shorthand
- Individual classification flags (`--lr-rating`, `--lr-label`, `--lr-keywords`) remain `false` (opt-in) by default to maintain backward compatibility and clean sidecar isolation.
- Introduce an `--all-lr` convenient shorthand flag in `photohelper develop` that activates all three metadata fields.
- Resolve resolved variables at startup:
  ```rust
  let lr_rating = args.all_lr || args.lr_rating;
  let lr_label = args.all_lr || args.lr_label;
  let lr_keywords = args.all_lr || args.lr_keywords;
  ```
  These resolved variables must be used *everywhere* (startup warnings, metadata builders, and NIMA score mappings).
- If all three resolved variables are `false`, output a highly visible startup warning to `stderr` explaining that no classification metadata is currently being written to sidecars, and suggest using `--all-lr` or individual flags.

### 2. High-Performance Granular Conflict Feedback
- Update the parallel Rayon pipeline in `develop` subcommand to log individual sidecar skips at `tracing::info!` or `tracing::debug!` inside the loop, rather than `tracing::warn!`. This avoids Terminal I/O locking and thread contention.
- At the end of `run_develop`, if `stats.conflict_preserved > 0`, print a single highly visible explanation to `stderr` explaining once that `X` files were skipped to protect Lightroom edits, and suggest using the `--force` flag if they want to override them.

### 3. Type-Safe Localized Color Label Customization
- Replace custom key-value string mapping with two separate type-safe CLI arguments in Clap:
  ```rust
  /// Custom Lightroom color label for 'Red' (NIMA < 4.0)
  #[arg(long, env = "PHOTOHELPER_LR_LABEL_RED", default_value = "Red")]
  lr_label_red: String,

  /// Custom Lightroom color label for 'Green' (NIMA >= 7.0)
  #[arg(long, env = "PHOTOHELPER_LR_LABEL_GREEN", default_value = "Green")]
  lr_label_green: String,
  ```
- Perform upfront validation inside `run_develop`: when color labels are enabled, trim the label strings and verify that they are non-empty (`!val.trim().is_empty()`), distinct, and contain only valid XML characters. If validation fails, exit immediately with a Clap error before loading catalogs or spawning threads.

### 4. mtime-Based Fallback Conflict Shield & Precision Alignment
- In `crates/photohelper-sidecar/src/conflict.rs`'s `merge_and_write` function, retrieve the XMP sidecar's actual filesystem modification time (`mtime`).
- If `mtime` retrieval fails (e.g. on custom mounts, virtual/sandboxed filesystems), catch the error, log a warning with `tracing::warn!`, and gracefully fall back to the existing database-timestamp checks.
- If `mtime > our_last_processed_at + 2.0` (with a 2-second safety margin), treat it as an external modification conflict and preserve the sidecar with `WriteOutcome::ConflictPreserved` to protect manual edits that didn't update `xmp:MetadataDate`.
- To completely eliminate write-buffer delay, system scheduling drift, and network mount (NAS/SMB) clock skews:
  1. Retrieve the UTC timestamp *per-photo* inside the parallel loop immediately before calling the sidecar writer.
  2. Use the `filetime` crate inside `crates/photohelper-sidecar/src/writer.rs` after a successful write to explicitly set the physical file's `mtime` to match exactly `ph:LastProcessedAt`. This guarantees perfect 1-to-1 alignment between the filesystem timestamp and the internal metadata timestamp, eliminating any race conditions.

### 5. XML Parser Resiliency & Swallowed Errors
- Ensure all XML parsing, attribute decoding, and unescaping errors in `photohelper-sidecar/src/reader.rs` are logged with `tracing::warn!` (including the sidecar file path context where available) and handled with safe, graceful fallbacks instead of being silently swallowed.

### 6. Syncing Guide & README Enhancements
- Author a beautiful, comprehensive user-facing guide at `docs/user-guide/lightroom-sync.md`. Explain Lightroom's passive metadata sync, reload instructions, case sensitivity rules, and custom color label mappings.
- Update the root `README.md` to mark all shipped subcommands (`cull`, `develop`, `export`, `dedup`) as completed/shipped and direct users to our new Lightroom sync guide.

---

## What is explicitly OUT OF SCOPE (deferred TDs)

| TD | Trigger | Rationale for deferral |
|---|---|---|
| TD-012 | Fires when develop does demosaic | Custom slider-based demosaic is not needed for metadata syncing. |
| TD-017 | Next session optimizing clustering | $O(N^2)$ clustering is stable for small-medium folders. |

---

## Stop-gap declarations

No new stop-gaps are introduced. We are building on top of existing safe models.

---

## Verification & Testing Strategy

### 1. Integration Tests in `crates/photohelper-cli/tests/cli.rs`
- **Clean Isolation Test**: Verify that executing `develop` without any flags writes no Lightroom classification metadata and prints the stderr warning.
- **Shorthand Flag Test**: Verify that passing `--all-lr` successfully writes rating, label, and keywords to sidecars.
- **Custom Translation Test**: Set `PHOTOHELPER_LR_LABEL_RED="Vermelho"` and `PHOTOHELPER_LR_LABEL_GREEN="Verde"`. Verify they are correctly written to the sidecar XML.
- **Upfront Validation Test**: Assert that invalid, colliding, or empty custom labels fail fast with Clap validation errors.
- **mtime Conflict Protection Test**: Create an XMP, set `ph:LastProcessedAt` matching our DB, touch the file's modification time forward (simulating a Lightroom edit), run `develop`, and assert it is preserved as `ConflictPreserved`.
- **Consolidated Warning Test**: Assert that the helpful `--force` bypass advice is printed exactly once at the end if files were skipped.

### 2. Unit Tests in `crates/photohelper-sidecar/tests/`
- Add `filetime = { workspace = true }` under `[dev-dependencies]` and `[dependencies]` in `crates/photohelper-sidecar/Cargo.toml`.
- Use `filetime::set_file_mtime` inside sidecar conflict tests (e.g. `conflict_overwrite_older_lightroom_edit`) to backdate the physical test files to match simulated past timestamps, avoiding false conflict triggers on OS-assigned current `mtime`.

### 3. Verification Command
- Run `just ci` to ensure all 236+ tests compile and pass.
