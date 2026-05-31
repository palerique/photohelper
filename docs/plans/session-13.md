# Session 13: Auto-Tone and Sortable Labels

## Goal
Add `--auto-tone` and `--lr-label-score` to `photohelper develop` to enable Lightroom auto-enhancement and allow users to sort photos natively by the exact aesthetic value (NIMA score).

## Deliverables

### 1. `crates/photohelper-sidecar/src/settings.rs`
- Extend `SidecarSettings` with `auto_tone: Option<bool>`.
- Extend `ParsedFields` with `auto_tone: Option<bool>`.
- Update `SidecarSettings::from_parsed` to accept `auto_tone`.
- Update `SidecarSettings::has_crs_fields()` to return `true` if `auto_tone` is set (otherwise Lightroom ignores the directive).
- Update `SidecarSettings::is_empty()` to return `false` if `auto_tone` is set.
- Update `SidecarSettings::merge()` to merge `incoming.auto_tone.or(self.auto_tone)`.
- Extract `merge_keywords` helper function to eliminate the 15-line code duplication for `keywords` and `hierarchical_keywords`.
- Refactor manual clamping for `temperature` and `tint` to use `v.clamp(MIN, MAX)`.

### 2. `crates/photohelper-sidecar/src/reader.rs`
- Update `parse_description_attrs` to parse `crs:AutoTone="True" / "False"`.
- If an invalid string is provided for `crs:AutoTone`, explicitly `tracing::warn!` the error and the malformed value instead of swallowing it.
- Refactor the 13 duplicated `match e.unescape()` blocks by hoisting the unescape call above the inner match.

### 3. `crates/photohelper-sidecar/src/writer.rs`
- Update `render_xmp` to emit `crs:AutoTone="True"` or `crs:AutoTone="False"` if `auto_tone` is set.

### 4. `crates/photohelper-cli/src/commands/develop.rs`
- Extend `DevelopArgs` struct with `--auto-tone` (bool) and `--lr-label-score` (bool).
- Use `clap`'s `conflicts_with = "lr_label"` on `--lr-label-score` to enforce mutual exclusivity at the type/parser edge, since both write to `xmp:Label`.
- Add explicit docstrings explaining that `lr_label_score` injects the score into `xmp:Label` to enable "Label Text" sorting in Lightroom.
- Consolidate all NIMA score logic (`lr_rating`, `lr_label`, `lr_label_score`, tier keywords) into a single `if let Some(score) = valid_nima` block.
- **CRITICAL**: Format the score with zero-padding (e.g., `format!("{:05.2}", score)`) before passing it to `builder.label()` to ensure Lightroom's lexicographical sort matches numeric values (`09.50` vs `10.00`).

### 5. `crates/photohelper-cli/src/commands/run.rs`
- Extend `RunArgs` with `--auto-tone` and `--lr-label-score`.
- Update the explicit `DevelopArgs` struct literal instantiation at `run.rs:181` to pass these new fields through.

### 6. Ecosystem & Documentation
- Update `scripts/photohelper-develop.sh` and `scripts/photohelper-all.sh` to use `--auto-tone` and `--lr-label-score` automatically for maximum image improvement out of the box.
- Update `docs/user-guide/lightroom-sync.md` to document the new `--auto-tone` behavior and explain how to configure and utilize `--lr-label-score` for native sorting.

## Out of Scope
- Building our own image improvement logic for Lightroom (delegated entirely to Lightroom's internal `AutoTone` engine). This distinction will be explicitly noted in the `SidecarSettings::auto_tone` docstring.

## Testing
- Unit tests for `SidecarSettingsBuilder` ensuring `auto_tone` builds correctly and propagates to `has_crs_fields`.
- Unit tests for `SidecarSettingsBuilder` verifying `lr_label_score` formats the NIMA score zero-padded (e.g., `09.50`).
- Unit tests for `reader.rs` parsing `crs:AutoTone="True"`, `crs:AutoTone="False"`, and failing loudly on invalid strings.
- Integration test for CLI `conflicts_with` behavior between `--lr-label` and `--lr-label-score`.
- Integration test for `photohelper develop --auto-tone --lr-label-score` ensuring the XMP sidecar contains `crs:AutoTone="True"` and `xmp:Label="...NIMA..."`.

## Checkpoints
- `plan-review-loop` for multi-agent validation.
- `session-end` double-review upon completion of the implementation.

## Discoveries & Assumptions
- **Discovery**: Lightroom natively supports sorting by "Label Text" lexicographically. To achieve true numeric sorting, the score must be zero-padded.
- **Discovery**: Lightroom requires `crs:HasSettings="True"` (triggered by `has_crs_fields()`), otherwise it ignores `crs:AutoTone`.
- **Assumption**: We will make `photohelper-all.sh` apply `--auto-tone` and `--lr-label-score` by default, as the user explicitly asked for "image improvement" and "sorting by aesthetic value".

## Synchronization Compliance
All references to `crs:AutoTone`, `xmp:Label`, `DevelopArgs`, and `ParsedFields` follow `docs/quality-assurance.md § State & Context Synchronization Discipline`.
