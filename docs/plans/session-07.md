# Session 07 Plan — Lightroom Namespace Compatibility
**Branch**: `session-07/lightroom-namespace-compatibility`
**Date**: 2026-05-30
**Status**: v5 — approved (remediated via Multi-Agent Plan Review Round 3 & Lightroom Compatibility Additions)

---

## Session goal

Resolve **DN-029** (Lightroom Classic custom namespace incompatibility) by mapping computed `photohelper` AI scores and duplicate cluster IDs to standard, natively indexable Lightroom fields (star ratings, color labels, flat keywords, and hierarchical keywords) inside `.xmp` sidecar files. This allows users to filter, group, and visualize duplicate groups and aesthetic culling selections natively inside Lightroom Classic without requiring third-party SDK plugins.

---

## What will exist by end of session

1. **Strongly-Typed `Rating` Enum**:
   ```rust
   #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
   #[repr(i32)]
   pub enum Rating {
       Rejected = -1,
       Unrated = 0,
       One = 1,
       Two = 2,
       Three = 3,
       Four = 4,
       Five = 5,
   }
   ```
   With a robust `TryFrom<i32>` implementation, ensuring ratings are strictly validated and unrepresentable as out-of-bounds integers.
   - **XMP Serialization Rule**: To guarantee perfect compatibility with Lightroom Classic, `Rating::Unrated` is serialized by **omitting** the `xmp:Rating` attribute entirely on write (avoiding non-standard `xmp:Rating="0"` noise), while `Rating::Rejected` is written as `xmp:Rating="-1"`.

2. **Extended `SidecarSettings` & Builder**:
   - `rating`: `Option<Rating>` representing the star rating state.
   - `label`: `Option<String>` representing the Lightroom color label. To support clearing/decaying an existing label, `Some(String::new())` (or `Some("")`) represents "explicitly clear label" (which the XML writer translates to `xmp:Label=""` explicitly, as Lightroom Classic requires the empty attribute to clear its catalog value), whereas `None` represents "unspecified (inherit existing during merge)".
   - `keywords`: `BTreeSet<String>` representing flat keywords in Dublin Core (`dc:subject`).
   - `hierarchical_keywords`: `BTreeSet<String>` representing hierarchical keywords in Lightroom (`lr:hierarchicalSubject`).
   - Map negative `dedup_cluster_id` database values gracefully to `None` during catalog extraction rather than triggering hard validation failures in the builder.

3. **Upgraded `photohelper-sidecar` Reader/Writer**:
   - **Namespace & Prefix Independence**: The reader matches local names (`Rating`, `Label`, `MetadataDate`, `subject`, `hierarchicalSubject`) with prefix flexibility (prefix-agnostic) to prevent silent read misses.
   - **Decimal Formatting Compatibility**: Numeric parsing helpers (e.g. `parse_i32`, `parse_i64`) parse input strings as `f64` first and round them to integers, ensuring compatibility with tools writing decimal-formatted integers (such as `xmp:Rating="3.0"`).
   - **XMP Reader**:
     - Parsing support for nested `dc:subject` and `lr:hierarchicalSubject` list structures.
     - **Flat state machine**: Maintain a single, flat outer parsing loop over `reader.read_event()` with a prefix-tag stack (`Vec<String>` of open element tags) to track the exact current path (e.g. `["dc:subject", "rdf:Bag", "rdf:li"]`). This eliminates nested sub-loops that consume events from the same reader cursor, eliminating CPU-spinning infinite loops and desynchronization on premature EOFs or missing end tags.
     - Ignores self-closing empty container elements (`Event::Empty`) from setting persistent state flags.
     - Safe, non-panicking text accumulator that logs warnings on unescaping failures and skips invalid segments gracefully rather than panicking.
     - Nested element format overrides attribute format for rating and labels if both are present in a sidecar.
     - **Clamping Extreme Sliders**: Clamps extreme but valid parsed Temperature (`[2000, 50000]`) and Tint (`[-150, 150]`) values written by Lightroom Classic to our valid bounds rather than silently discarding them.
   - **XMP Writer**:
     - **Control Character Sanitization**: Prior to writing, all user-defined strings (labels, keywords) are filtered to remove XML 1.0 illegal control characters (range `0x00`–`0x1F` except `0x09`, `0x0A`, `0x0D`).
     - **XML Escaping**: All sanitized user strings are escaped using `quick_xml::escape::escape` on writing.
     - Declares namespaces `xmlns:dc` and `xmlns:lr` on the `<rdf:Description>` element conditionally only when their respective fields/collections are non-empty.
   - **Append-only Merging**:
     - Preserves existing standard fields (ratings, labels, keywords) inside `merge_and_write` even when individual update flags are not passed (no default clobbering).
     - **First-Run Merging**: Modify the `(Some(_), None)` branch of `conflict.rs` (representing pre-existing sidecars without a `ph:LastProcessedAt` timestamp) to perform a safe, non-destructive merge and write the merged results back, returning `WriteOutcome::Overwritten` (or `WriteOutcome::Merged`). Only bypass and return `ConflictPreserved` if we have already processed the file (`Some(lp)`) and the external edits are strictly newer than our last run (`md > lp`).
     - **Stale Keyword Decay**: During keyword merging, strip previous photohelper flat and hierarchical keywords case-insensitively. This includes stripping the bare `"nima:"` and `"cluster:"` prefixed flat keywords along with standard `"photohelper"` prefixes to prevent flat keyword accumulation and pollution across subsequent develop runs.
     - Hierarchical leaf nodes in `<dc:subject>` and `<lr:hierarchicalSubject>` are cleanly aligned to avoid root-level Keyword List pollution in Lightroom Classic.

4. **Database & Query Layer Upgrades**:
   - Extended `DevelopRow` in `crates/photohelper-catalog/src/row.rs` to include `dedup_cluster_id: Option<i64>`.
   - Updated `Catalog::all_photos_with_cull_scores` to take both `aesthetic_model_slug: &str` and `dedup_model_slug: &str` as parameters, completely decoupling the storage layer from AI domain constants.

5. **CLI `develop` Subcommand Upgrades**:
   - CLI flags added: `--lr-rating`, `--lr-label`, `--lr-keywords`.
   - Propagates `dedup_cluster_id` from `DevelopRow` to `SidecarSettingsBuilder`.
   - Handles `is_finite()` validation on float scores early; NaN or Infinite scores raise a `tracing::warn!` and map rating/label to `None` rather than falling through to standard ratings.
   - Emits a clear console warning if any requested Lightroom compatibility flags are active but their corresponding database source data in the catalog is empty.

6. **Parallelism via Rayon**:
   - **Write Race Hazard Prevention**: Dedup targeted sidecar paths before starting parallel execution to avoid concurrent write race hazards on duplicate files or virtual copies. Use thread-unique temporary file suffixes (e.g. `<path>.phdev.<thread_id>.tmp`) to ensure isolation.
   - **SystemTime Overhead Reduction**: Call `unix_now_as_datetime()` exactly once before parallel iteration begins to prevent high-frequency, concurrent `SystemTime::now()` system call overhead.
   - **Parallel Error Accumulation**: Map the parallel iteration to `Result<PathBuf, SidecarProcessError>` for each file, accumulate success and error listings, emit `tracing::warn!` with the exact file path and reason on any failure, print an aggregate final user-facing summary to stderr, and exit with correct strict/non-strict status codes.

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
| TD-021 | First session extending RAW Exif variant handling | `RawExifCause::UnsupportedFormat` variant is dead code; defer until RAW format parsing is expanded |
| TD-022 | First session adding non-crs: namespace fields we don't fully model (XMP round-trip fidelity) | We still use a pure-Rust template-based rendering for sidecars. |

- **TD-023 (time-msrv)**: Pin the `time` crate dependency strictly to `=0.3.47` in `Cargo.toml` to guarantee compiling stability under workspace Rust `1.88` MSRV, avoiding breaking patch-bumps from upstream's aggressive MSRV policy.

---

## Stop-gap declarations

All active stop-gaps:

| # | Stop-gap | TD | Introducing commit | Location | Binding trigger |
|---|---|---|---|---|---|
| S1 | XMP write uses `quick-xml` manual template rather than Adobe XMP Toolkit SDK | TD-022 | `photohelper-sidecar/src/writer.rs` | First session adding non-crs: namespace fields we don't fully model; or before v1.0 if XMP round-trip fidelity is required |

---

## Design decisions locked by this plan

### D1 — NIMA Aesthetic Score Bins
When `--lr-rating` is active, NIMA scores are mapped to `Rating` using these open-ended boundary conditions:
* Score $< 4.0 \rightarrow \mathbf{Rating::One}$ (Severe technical flaws / disfavored aesthetic)
* $[4.0, 5.5) \rightarrow \mathbf{Rating::Two}$ (Below average / discard candidate)
* $[5.5, 7.0) \rightarrow \mathbf{Rating::Three}$ (Average / baseline keeper)
* $[7.0, 8.5) \rightarrow \mathbf{Rating::Four}$ (Good quality / keeper)
* Score $\ge 8.5 \rightarrow \mathbf{Rating::Five}$ (Exceptional quality / highly favored)

These bins also determine the keyword `<tier>` simple adjective string used by `--lr-keywords`:
* `discard` (score $< 4.0$)
* `poor` ($[4.0, 5.5)$)
* `fair` ($[5.5, 7.0)$)
* `good` ($[7.0, 8.5)$)
* `excellent` ($\ge 8.5$)

### D1b — NIMA Aesthetic Score Bins to Color Labels
When `--lr-label` is active, NIMA scores are mapped to Lightroom's standard color labels. Standard metadata consumers (such as Adobe Lightroom Classic) use `xmp:Label` to color-code photos in the library view:
* Score $< 4.0 \rightarrow \mathbf{"Red"}$ (Color-coded Red to denote immediate discard candidates / technical failures)
* Score $\ge 7.0 \rightarrow \mathbf{"Green"}$ (Color-coded Green to denote high-quality keepers / favorites)
* Score $[4.0, 7.0) \rightarrow \mathbf{""}$ (An empty string represents explicitly clearing any existing color label during deep merge)

### D2 — Hierarchical vs Flat Keywords
Lightroom Classic natively indexes flat keywords in Dublin Core `<dc:subject>` and hierarchical structures in `<lr:hierarchicalSubject>`.
* Hierarchical leaf nodes in `<dc:subject>` must exactly match the leaf nodes of the hierarchy to avoid root-level Keyword List pollution.
* For a photo with NIMA score `7.3` (tier `good`) and duplicate cluster ID `3`, we write:
  - Flat tags in `<dc:subject>`:
    - `photohelper`
    - `cluster:3`
    - `nima:good`
  - Hierarchical tags in `<lr:hierarchicalSubject>`:
    - `photohelper`
    - `photohelper|cluster:3`
    - `photohelper|nima:good`

### D3 — Append-Merging to Preserve Existing Keywords (No Clobbering)
* When writing XMP updates:
  1. Parse the existing XMP file (if any) and collect existing rating, label, flat, and hierarchical keywords.
  2. Perform a deep merge: inherit existing ratings, labels, and Camera Raw sliders if the incoming parameters are `None`.
  3. Strip previous `photohelper` flat, hierarchical, and bare prefixed tags case-insensitively to prevent stale keyword accumulation: match exact `"photohelper"`, and any starting with `"photohelper:"`, `"photohelper|"`, `"cluster:"`, or `"nima:"`. This ensures that flat keywords like `"cluster:3"` and `"nima:good"` do not accumulate stale entries across multiple runs.
  4. Perform a union: `all_keywords = user_keywords ∪ generated_keywords`.
  5. Trim whitespace, filter out empty elements, sort alphabetically, and deduplicate lists.
  6. Write the merged lists back into the child elements of `<rdf:Description>`.

### D4 — Industry Metadata Compatibility (Aftershoot, Lightroom, etc.)
To allow users to compare the "before and after" states of raw adjustments and metadata seamlessly inside Adobe Lightroom Classic, Aftershoot, and other industry-standard tools, `photohelper` writes standard properties under standard namespaces directly to the `.xmp` sidecar files:
* **Star Ratings (`xmp:Rating`)**: Placed in the `http://ns.adobe.com/xap/1.0/` namespace. Star ratings range from `1` to `5`, and `-1` represents a "Rejected" state. To align with Adobe Lightroom Classic's exact expectations, "Unrated" (`0`) is written by omitting the `xmp:Rating` attribute entirely on write (preventing non-standard `xmp:Rating="0"` clutter), while rejected files carry an explicit `xmp:Rating="-1"`.
* **Color Labels (`xmp:Label`)**: Placed in the `http://ns.adobe.com/xap/1.0/` namespace. Emits `"Red"` for poor images ($< 4.0$), `"Green"` for excellent keepers ($\ge 7.0$), and an explicit empty string `""` to clear any pre-existing color label for average images.
* **Flat Keywords (`dc:subject`)**: Placed under the Dublin Core namespace `http://purl.org/dc/elements/1.1/` inside an `<rdf:Bag>`. This allows Lightroom Classic to natively index flat metadata keywords (`photohelper`, `cluster:{id}`, `nima:{tier}`).
* **Hierarchical Keywords (`lr:hierarchicalSubject`)**: Placed under the Lightroom namespace `http://ns.adobe.com/lightroom/1.0/` inside an `<rdf:Bag>` (`photohelper`, `photohelper|cluster:{id}`, `photohelper|nima:{tier}`). By writing to `lr:hierarchicalSubject` and nesting elements beneath a root `"photohelper"`, we allow Lightroom Classic's hierarchical keyword list to remain perfectly clean, keeping our generated keywords nested beneath a single, unified group.
* **Development Sliders (`crs:`)**: Placed in the `http://ns.adobe.com/camera-raw-settings/1.0/` namespace. Support standard Lightroom parameters (`crs:ProcessVersion`, `crs:Temperature`, `crs:Tint`, `crs:Exposure2012`, `crs:Contrast2012`, `crs:Highlights2012`, `crs:Shadows2012`). By matching and merging these fields, we allow users to compare their AI/user-enhanced RAW renderings natively.
* **Non-Destructive Blending (Non-Clobbering)**: During deep merge, photohelper acts in an append-only mode. If a user has already made edits (e.g., custom exposure, crop points, camera profiles, or radial gradients) in Lightroom Classic, those unmodeled properties are preserved exactly as-is. Our writer merges our targeted fields without discarding any other metadata or development sliders inside the sidecar.

### D5 — Rust Metadata Library Evaluation
We evaluated several Rust libraries to determine the best approach for XMP parsing, merging, and writing. The evaluation criteria centered on: performance, pure-Rust compilation safety (avoiding complex external C/C++ FFI toolchain constraints), and non-destructive merging of existing XMP files:
1. **`xmp-writer`**:
   * *Strengths*: Pure Rust, lightweight, and very clean API for creating XMP sidecars.
   * *Weaknesses*: Strictly **write-only / creation-only**. It has no parsing or querying capabilities. If we used `xmp-writer`, we would be unable to parse existing XMP sidecars to perform a deep merge. This would lead to catastrophic metadata loss, completely clobbering existing user adjustments, crop points, or custom metadata when writing photohelper updates.
2. **`rexiv2`**:
   * *Strengths*: Highly capable, wrapper around the standard C++ `Exiv2` library, supporting full read/write/merge round-tripping.
   * *Weaknesses*: Introduces a heavy, brittle **C/C++ FFI build dependency** on the system's `libexiv2` library. This violates the repository's strict architectural mandate to build portable, single-binary executables with zero system-level C/C++ runtimes or compilation-phase FFI pkg-config requirements.
3. **`quick-xml` (Our Choice)**:
    * *Approach*: By leveraging `quick-xml`'s high-performance streaming parser (`Reader`) for parsing and its robust escaping module (`quick_xml::escape::escape`) combined with a custom lightweight templated formatter, we parse incoming XMP files event-by-event, extract the active fields (ratings, labels, keywords) into a safe in-memory data structure, perform a deep non-destructive merge, and format/write the merged results back atomically. This preserves 100% of unmodeled Lightroom metadata, retains full round-trip fidelity, and keeps compiling 100% safe and portable with no FFI overhead.

---

## Verification plan

### Automated Tests

1. **Unit tests in `crates/photohelper-sidecar`**:
   - `temperature_out_of_range_rejected`, `exposure_out_of_range_rejected`, `tint_out_of_range_rejected`, `int_crs_field_boundary_rejected`, and `valid_settings_build_succeeds`: Validate SidecarSettings builder and limits.
   - `sidecar_path_for_cr3_replaces_extension`: Validates sidecar path extension conversion.
   - `write_and_read_roundtrip_all_fields`: Asserts a complete write/read roundtrip works for all fields.
   - `write_with_only_ph_namespace` and `write_with_only_crs_namespace`: Asserts conditional/scoped writing of only specific namespaces when others are absent.
   - `lightroom_compatible_output`: Checks Lightroom standard namespace declarations are correctly output.
   - `read_unknown_fields_ignored`, `read_malformed_temperature_warns_and_returns_none`, `read_malformed_xml_returns_parse_error`, and `read_minimal_xmp`: Verify parser robustness.
   - `conflict_preserve_newer_lightroom_edit`, `conflict_overwrite_older_lightroom_edit`, `conflict_missing_metadata_date_preserves`, `conflict_missing_last_processed_merges`, and `conflict_force_overwrite`: Verify conflict resolution matrix outcomes.
   - `write_xmp_to_readonly_dir_returns_io_error` and `write_xmp_atomic_no_partial_on_io_error`: Validate atomic write temp file creation and cleanup on failure.
   - `test_slider_clamping_on_parse`: Asserts raw slider values from XML are clamped to valid ranges.
   - `test_merge_and_write_empty_color_label_retention`: Validates that empty/cleared labels or keywords are retained or omitted as per instructions.
   - `test_precise_keyword_stripping_on_merge` and `test_precise_hierarchical_keyword_stripping_on_merge`: Asserts precise merging/removal of keywords.
   - `test_rating_try_from`: Asserts Rating enum conversions.
   - `test_lenient_parsing_via_xmp`: Asserts lenient XML parser behavior.
   - `test_xml_illegal_control_character_sanitization`: Asserts that invalid XML characters are sanitized.
   - `test_write_no_keywords_omits_elements`: Verifies that empty keyword collections emit no XML container tags.
   - `test_parse_prefix_agnostic_attributes`: Asserts alternative namespace prefix compatibility.
   - `test_read_non_bag_rdf_containers`: Asserts non-bag standard RDF keyword container compatibility.
   - `test_parse_crs_elements_detects_crs_attr`: Asserts nested element `<crs:Temperature>` compatibility.

2. **Unit tests in `crates/photohelper-catalog`**:
   - `test_develop_row_retrieves_cluster_id`: Asserts that `Catalog::all_photos_with_cull_scores` correctly LEFT JOINs `dup_clusters` and populates `dedup_cluster_id` filtering on active CLIP model slug.

3. **Integration tests in `crates/photohelper-cli` (`tests/cli.rs` or unit tests in commands)**:
   - `test_nima_score_mapping_boundaries`: Asserts the float boundary mappings for star ratings, color labels, and keyword tiers, ensuring robust handling of `NaN`, `Infinity`, and out-of-bounds values.
   - `test_develop_with_lightroom_compatibility_flags`: Asserts that executing `photohelper develop --lr-rating --lr-label --lr-keywords` parses the database, calculates rating/labels/keywords, and outputs fully-formed compatible `.xmp` sidecar files.
   - `test_develop_clean_isolation_by_default`: Runs `develop` without any `--lr-*` flags and asserts that the resulting XMP contains ONLY `ph:` namespace properties, with standard tags and collections completely absent.
   - `test_develop_individual_lr_flags`: Verifies that passing only `--lr-rating` writes the rating but does not write labels or keywords to standard tags.
   - `test_develop_handles_nan_and_infinite_scores`: Verifies NaN score error mapping and early validation.
   - `test_develop_rayon_partial_failures`: Verifies Rayon-based parallel execution robustly aggregates error outcomes and logs failures safely.
   - `test_develop_missing_scores_warning`: Asserts that a console warning is successfully emitted if any Lightroom compatibility flags are active but the catalog contains zero scores.

---

## Checkpoints & Cadence

This session operates under the **A-tier (high-frequency / strict)** protocol cadence:
* **Plan-review**: Run `plan-review` skill (two rounds of double review) immediately before implementation code is written.
* **Session-end**: Run `session-end` skill (two rounds of double review) to verify gating, compile, verify state, and ship the branch.
