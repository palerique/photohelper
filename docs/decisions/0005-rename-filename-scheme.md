# Decision 0005 — `rename` filename scheme divergence from `export`

**Date**: 2026-06-02 (session 15)

## Context

The `rename` subcommand and the `export` subcommand both construct output filenames
that embed the NIMA cull score and dedup cluster id. They use different capitalisation
conventions that were established independently:

| Subcommand | Filename format |
|---|---|
| `export` | `cluster-007-cull-07.85-photo.jpg` (all lower-case) |
| `rename` | `Cluster-007_Cull-07.85-photo.CR3` (mixed-case, underscore separator, no ext change) |

The `rename` format matches the user's explicit specification (`Cluster-{X}_Cull-{Y}-…`).
The `export` format emerged earlier from plan session-01/02 conventions.

## Decision

Both formats are intentional and are **not** unified:

1. **`export` format stays lower-case** — changing it would rename all existing
   exported JPEGs and break users' Lightroom imports.
2. **`rename` format uses the user's spec** — the capitalised `Cluster-…_Cull-…`
   prefix is the agreed naming convention for the renamed RAW/sidecar copies.
3. **Shared formatter**: both subcommands route through the shared
   `format_nima_score_label` helper (`commands/util.rs`) for the `{:05.2}` zero-padded
   score formatting, ensuring consistent score precision across both outputs.
4. **`develop.rs` collision key** uses NFC + lowercase (`develop.rs:240-264`) while
   the shared `resolve_collisions` in `util.rs` is lowercase-only (`export.rs`-style).
   `develop` is out of scope; the divergence is acknowledged and intentional.

## Consequences

- Users who compare `export` and `rename` output directories will observe different
  capitalisation. This is expected and documented.
- A future unification session MUST not change either format without an explicit
  user-facing migration plan (TD or ADR).
