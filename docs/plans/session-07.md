# Session 07 Plan — Lightroom Namespace Compatibility
**Branch**: `session-07/lightroom-namespace-compatibility`
**Date**: 2026-05-30
**Status**: v1 — draft

---

## Session goal

Resolve **DN-029** (Lightroom Classic custom namespace incompatibility) by mapping computed `photohelper` AI scores and duplicate cluster IDs to standard, natively indexroom-supported fields (star ratings, color labels, flat keywords, and hierarchical keywords) inside `.xmp` sidecar files. This allows users to filter, group, and visualize duplicate groups and aesthetic culling selections natively inside Lightroom Classic without requiring third-party SDK plugins.

---

## What will exist by end of session

1. **Extended `SidecarSettings`**:
   - `rating`: `Option<i32>` representing the native star rating (range `[1, 5]`).
   - `label`: `Option<String>` representing the Lightroom color label (e.g., `"Green"`, `"Red"`).
   - `keywords`: `Vec<String>` representing flat keywords in the Dublin Core (`dc:subject`) bag.
   - `hierarchical_keywords`: `Vec<String>` representing hierarchical keywords in the Lightroom (`lr:hierarchicalSubject`) bag.
   - Robust builder validation guaranteeing valid star ratings `[1, 5]`.

2. **Upgraded `photohelper-sidecar` Reader/Writer**:
   - **XMP Reader**: Parsing support for nested `dc:subject` and `lr:hierarchicalSubject` child tags (specifically parsing `<rdf:Bag>` and `<rdf:li>` elements) and simple attributes `xmp:Rating` and `xmp:Label` on `<rdf:Description>`.
   - **XMP Writer**: Native rendering of nested `dc:subject` and `lr:hierarchicalSubject` XML structures, plus simple `xmp:Rating` and `xmp:Label` attributes, with accurate namespace declarations (`xmlns:dc="http://purl.org/dc/elements/1.1/"` and `xmlns:lr="http://ns.adobe.com/lightroom/1.0/"`).
   - **Append-only Merging**: Update the conflict/merging layer to perform a **union** of keywords instead of clobbering. If a sidecar already contains user-defined keywords, `photohelper` appends its generated keywords and dedupes them, ensuring zero data loss of user edits.

3. **Lightroom Mapping Layer in CLI `develop` Subcommand**:
   - Added CLI flags to `photohelper develop`:
     - `--lr-rating`: Maps NIMA scores `[1.0, 10.0]` to standard star ratings `[1, 5]` using a defined mapping bin.
     - `--lr-label`: Maps high-quality keepers (NIMA $\ge 7.0$) to `"Green"` and low-quality discards (NIMA $< 4.0$) to `"Red"`.
     - `--lr-keywords`: Automatically writes `photohelper` keywords:
       - Flat (`dc:subject`): `photohelper`, `photohelper:cluster:<id>`, `photohelper:nima:<tier>`.
       - Hierarchical (`lr:hierarchicalSubject`): `photohelper`, `photohelper|cluster|<id>`, `photohelper|nima|<tier>`.
   - Full backward compatibility: if these flags are absent, only the custom `ph:` namespace properties are written (preserving existing clean metadata isolation).

4. **Rigorous Tests**:
   - **Sidecar Unit Tests**: Asserting reading, writing, and append-merging of ratings, labels, and flat/hierarchical keywords.
   - **CLI Integration Tests**: Verifying that `photohelper develop --lr-rating --lr-label --lr-keywords` outputs compliant `.xmp` sidecar structures containing both standard attributes and nested RDF collections.

---

## What is explicitly OUT OF SCOPE (deferred TDs with non-fired triggers)

| TD | Trigger (not yet fired) | Rationale for deferral |
|---|---|---|
| TD-002 | MSRV bump needed (1.88→1.92+) before rusqlite 0.40 | MSRV bump is its own ADR process; no CVE pressure |
| TD-006 | Fires when develop does pixel processing | v0.1 develop = XMP sidecars only; no pixel decode |
| TD-007 | Fires when `photohelper-raw/src/decode.rs` extended | Develop doesn't extend raw decode API in v0.1 |
| TD-012 | Fires when develop does AHD demosaic for processed output | Export session; not needed for XMP-only develop |
| TD-013 | User-report trigger ("I ran cull twice…") | Not fired |
| TD-015 | User-request trigger (custom NIMA model) | Not fired |
| TD-017 | n > 10K photo corpus trigger | Not fired |
| TD-018 | User storage-size complaint trigger | Not fired |
| TD-019 | User-report trigger (dedup audit trail) | Not fired |
| TD-022 | First session adding non-crs: namespace fields we don't fully model (XMP round-trip fidelity) | We still use a pure-Rust template-based rendering for sidecars, which matches our stop-gap S1 (quick-xml manual template). |

---

## Stop-gap declarations

All active stop-gaps:

| # | Stop-gap | TD | Introducing commit | Location | Binding trigger |
|---|---|---|---|---|---|
| S1 | XMP write uses `quick-xml` manual template rather than Adobe XMP Toolkit SDK | TD-022 | `photohelper-sidecar/src/sidecar.rs` | First session adding non-crs: namespace fields we don't fully model (e.g. `crs:GradientBasedCorrections`); or before v1.0 if XMP round-trip fidelity is required |

---

## Design decisions locked by this plan

### D1 — NIMA Aesthetic Score Bins
When `--lr-rating` is active, NIMA scores are mapped to `xmp:Rating` (star ratings `[1, 5]`) using these boundary conditions:
* $[1.0, 4.0) \rightarrow \mathbf{1}$ **star** (Severe technical flaws / disfavored aesthetic)
* $[4.0, 5.5) \rightarrow \mathbf{2}$ **stars** (Below average / discard candidate)
* $[5.5, 7.0) \rightarrow \mathbf{3}$ **stars** (Average / baseline keeper)
* $[7.0, 8.5) \rightarrow \mathbf{4}$ **stars** (Good quality / keeper)
* $[8.5, 10.0] \rightarrow \mathbf{5}$ **stars** (Exceptional quality / highly favored)

These bins also determine the keyword `<tier>` string used by `--lr-keywords`:
* `nima:discard` (score $< 4.0$)
* `nima:poor` ($[4.0, 5.5)$)
* `nima:fair` ($[5.5, 7.0)$)
* `nima:good` ($[7.0, 8.5)$)
* `nima:excellent` ($\ge 8.5$)

### D2 — Hierarchical vs Flat Keywords
Lightroom Classic natively indexes flat keywords in Dublin Core `<dc:subject>` and hierarchical structures (nested via pipes or tabs) in `<lr:hierarchicalSubject>`.
* To guarantee perfect compatibility, `--lr-keywords` writes to **both** structures.
* For a photo with NIMA score `7.3` and duplicate cluster ID `3`, we write:
  * Flat tags in `<dc:subject>`:
    * `photohelper`
    * `photohelper:cluster:3`
    * `photohelper:nima:good`
  * Hierarchical tags in `<lr:hierarchicalSubject>`:
    * `photohelper`
    * `photohelper|cluster|3`
    * `photohelper|nima|good`

### D3 — Append-Merging to Preserve Existing Keywords (No Clobbering)
XMP sidecar files represent the single source of truth for user edits. Clobbering existing keywords is a **CRITICAL severity bug**.
* When writing XMP updates:
  1. Parse the existing XMP file (if any) and collect all existing values under `<dc:subject>` and `<lr:hierarchicalSubject>`.
  2. Perform a union: `all_keywords = existing_keywords ∪ generated_keywords`.
  3. Sort and deduplicate the list to ensure output remains clean, deterministic, and free of duplicates.
  4. Write the merged lists back into the child elements of the `<rdf:Description>`.

---

## Verification plan

### Automated Tests
1. **Unit tests in `crates/photohelper-sidecar`**:
   - `test_read_dc_subject_flat_keywords`: Asserts reader successfully parses simple `<dc:subject>` tags.
   - `test_read_lr_hierarchical_keywords`: Asserts reader parses `<lr:hierarchicalSubject>` lists.
   - `test_write_keywords_nested_bag`: Asserts that writing flat and hierarchical keywords produces perfectly indented, compliant `<rdf:Bag>` tags with correct namespaces.
   - `test_merge_preserves_existing_keywords`: Validates that existing, unrelated user keywords are not lost or duplicated when merging new `photohelper` tags.
   - `test_read_write_rating_label`: Asserts that `xmp:Rating` (integer) and `xmp:Label` (string) round-trip accurately through parser and formatter.
2. **Integration tests in `crates/photohelper-cli` (`tests/cli.rs`)**:
   - `test_develop_with_lightroom_compatibility_flags`: Asserts that executing `photohelper develop --lr-rating --lr-label --lr-keywords` parses the database, calculates rating/labels/keywords, and outputs fully-formed compatible `.xmp` sidecar files containing expected elements.

---

## Checkpoints & Cadence

This session operates under the **A-tier (high-frequency / strict)** protocol cadence:
* **Plan-review**: Run `plan-review` skill (two rounds of double review) immediately before implementation code is written.
* **Session-end**: Run `session-end` skill (two rounds of double review) to verify gating, compile, verify state, and ship the branch.
