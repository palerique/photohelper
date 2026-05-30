# BUG-001 — Lightroom Classic Metadata Synchronization Gaps

> **Status**: PROPOSED
> **Severity**: HIGH (Blocks primary end-user workflow)
> **Date**: 2026-05-30
> **Authors**: Antigravity ( Session 08 )
> **Related**: `docs/discovery-notes.md § DN-029`, `docs/adr/0003-lightroom-compatibility-xmp-architecture.md`

---

## 1. Bug Description

The user reports that after running `photohelper`'s ingestion, culling, sorting, and metadata writing pipeline, opening Adobe Lightroom Classic and syncing metadata does not display the expected changes, classifications, sorts, or improvements. If metadata changes (ratings, labels, keywords) cannot be synced reliably into Lightroom Classic, the utility of the tool is severely limited for their cataloging workflow.

---

## 2. Technical Root-Cause Analysis

Our analysis of the codebase, Lightroom Classic's design quirks, and the XMP specifications has surfaced **three primary vectors** that can cause this issue:

### Vector A: Lightroom's Passive Metadata Syncing Model (Pull, Not Push)
Unlike some raw editors that watch the filesystem dynamically, **Adobe Lightroom Classic does not automatically scan and import `.xmp` sidecar changes for photos already in its active catalog.**
- **The Behavior**: If a user runs `photohelper develop` on RAW files that have already been imported into their Lightroom catalog, the new ratings, color labels, and keywords will be written perfectly to disk in `<photo>.xmp`, but Lightroom will remain unaware.
- **The Symptom**: The photo's metadata will look unchanged in Lightroom, or a subtle "Metadata has been changed on disk" exclamation-mark icon will appear on the thumbnail.
- **The Remedy**: The user must explicitly select the affected photos in Lightroom Classic, right-click, and select **Metadata -> Read Metadata from File**.

### Vector B: Missing Lightroom CLI Flags
In `photohelper-cli`'s `develop` subcommand, the flags to map AI classifications to standard Lightroom-readable metadata fields are **strictly opt-in** and default to `false`:
- `--lr-rating`: Map NIMA score to standard `xmp:Rating` stars (1–5).
- `--lr-label`: Map NIMA ranges to standard `xmp:Label` color bands (Red/Green/None).
- `--lr-keywords`: Map duplicate cluster IDs and quality tiers to standard `dc:subject` and `lr:hierarchicalSubject` keywords.

If the user simply executes:
```bash
photohelper develop
# or
just develop
```
...the tool will write basic camera adjustments (like exposure, temperature, and tint if specified), but **will write absolutely zero star ratings, color labels, or keywords**. The user might assume that these are written automatically after a culling run.

### Vector C: Quiet Conflict Resolution Exclusions
In `crates/photohelper-sidecar/src/conflict.rs`, we implemented a robust safety shield to prevent `photohelper` from destroying existing, high-value edits made in Lightroom:
- If a photo has been edited in Lightroom *after* it was ingested/processed by `photohelper` (detected because `xmp:MetadataDate > ph:LastProcessedAt`), `photohelper` resolves this as `WriteOutcome::ConflictPreserved`.
- **The Behavior**: The CLI silently preserves their Lightroom edit and **refuses to write any new ratings or keywords** to that file, returning a skipped count in the final summary.
- **The Gap**: The current terminal summary displays the total number of files in this state (e.g., `conflict-preserved: 15`), but does not print *which* specific files were skipped, or why. The user is left in the dark about why some files did not sync, and they may not know they can bypass this protection using the `--force` flag.

### Vector D: Localized Color Label Set Mismatches
Lightroom Classic's color label system (`xmp:Label`) is **string-literal based** and matches whatever set is active in the user's Lightroom Catalog under **Metadata -> Color Label Set**.
- In an English locale, writing `xmp:Label="Green"` highlights the photo's border in green.
- If the user runs Lightroom Classic in another language (e.g., Portuguese, Spanish, or French) where the green color bar is bound to a translated string (like `"Verde"` or `"Vert"`), writing `"Green"` results in Lightroom displaying a white border with the text `"Green"`, rather than the actual green color highlight.

---

## 3. Proposed Session 09 Action Plan

To ensure that `photohelper`'s metadata is 100% visible, intuitive, and reliable in Lightroom Classic, we propose dedicating the upcoming **Session 09** to the following targeted improvements:

### Action 1: Standardize CLI UX with Smart Warnings
- If `photohelper develop` is run without any active Lightroom flags (`--lr-rating`, `--lr-label`, or `--lr-keywords`), the CLI should output a friendly, highly visible terminal warning pointing out that no classification metadata is being written to the sidecars.
- Propose adding a convenient `--all-lr` shorthand or changing the defaults so that running `photohelper develop` automatically writes all classifications unless explicitly disabled.

### Action 2: Add Granular Conflict Feedback
- Update the parallel Rayon loop in `develop.rs` to print a detailed `info` or `warn` log line for every file skipped due to conflict preservation, explaining that the file has newer edits in Lightroom and telling the user they can bypass this check using `photohelper develop --force`.

### Action 3: Write a Comprehensive Lightroom Syncing Guide
- Author a permanent troubleshooting section in the workspace's `README.md` or a dedicated `docs/user-guide/lightroom-sync.md`.
- Detail the exact step-by-step instructions for the user:
  1. How to select photos in Lightroom and trigger **Metadata -> Read Metadata from File**.
  2. How to ensure their Lightroom Color Label Set is configured to match the strings written by `photohelper` (or document how they can customize the label strings using an environment variable or config option).

---

## 4. Verification & Testing Strategy

1. **Integration Tests**:
   - Add a test verifying that calling `develop` without flags warns the user.
   - Verify that localized custom labels can be written if configured.
2. **User Acceptance Verification**:
   - Run the updated pipeline on a test set of raw photos, import them into Lightroom, verify immediate auto-read, and verify pull-to-sync for pre-existing catalog items.
