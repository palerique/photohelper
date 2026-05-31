# Session 13: Auto-Tone and Sortable Labels

## Goal
Add `--auto-tone` and `--lr-label-score` to `photohelper develop` to enable Lightroom auto-enhancement and allow users to sort photos natively by the exact aesthetic value (NIMA score).

## Deliverables
- `SidecarSettings` extended with `auto_tone: Option<bool>`.
- `reader.rs` updated to parse `crs:AutoTone="True" / "False"`.
- `writer.rs` updated to emit `crs:AutoTone="True" / "False"` if set.
- `DevelopArgs` extended with `--auto-tone` and `--lr-label-score`.
- `develop.rs` updated to route these flags into the `SidecarSettingsBuilder`.
- `scripts/photohelper-all.sh` updated to use `--auto-tone` and `--lr-label-score` automatically for maximum image improvement out of the box.

## Out of Scope
- Actually building our own image improvement logic for Lightroom (we are explicitly delegating to Lightroom's native `AutoTone` engine).

## Testing
- Unit tests for `SidecarSettingsBuilder` ensuring `auto_tone` builds correctly.
- Unit tests for `reader.rs` and `writer.rs` round-tripping `crs:AutoTone="True"`.
- Integration test for `photohelper develop --auto-tone --lr-label-score` ensuring the XMP sidecar contains `crs:AutoTone="True"` and `xmp:Label="...NIMA..."`.

## Checkpoints
- Hand off to `plan-review` for multi-agent validation.

## Discoveries & Assumptions
- **Discovery**: Lightroom natively supports sorting by "Label Text", which allows us to achieve exactly what the user wants (sort by NIMA score) by injecting the score text into `xmp:Label`.
- **Discovery**: Lightroom natively supports auto-improvement on import if the `crs:AutoTone="True"` metadata is injected into the sidecar.
- **Assumption**: We will make `photohelper-all.sh` apply `--auto-tone` and `--lr-label-score` by default, as the user explicitly asked for "image improvement" and "sorting by aesthetic value".
- **Assumption**: `lr_label_score` takes precedence over `lr_label` (color labels) if both happen to be set, but `photohelper-all.sh` will use `lr_label_score`.

## Synchronization Compliance
All references to `crs:AutoTone` and `xmp:Label` follow `docs/quality-assurance.md § State & Context Synchronization Discipline`.
