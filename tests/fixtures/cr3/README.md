# Canon CR3 test fixtures

> **License**: every CR3 in this directory is **CC0 / public domain**.
> Sourced from [raw.pixls.us](https://raw.pixls.us/) which mandates CC0
> for new submissions. Pixls confirms: *"I declare that I own full rights
> to this file and I hereby release it under the [CC0] license into the
> public domain."*

These fixtures are the **integration-test inputs** for the photohelper
RAW pipeline. The cargo test suite reads them through
`photohelper-raw::exif::read_cr3` and `photohelper-raw::decode::read_raw`
to verify the LibRaw FFI behaves correctly against real Canon EOS R8
sensor data (not synthetic stubs).

## Per-fixture provenance

| Fixture | Source | Original size | License | Notes |
|---------|--------|---------------|---------|-------|
| `CRAW_FULL_FRAME.CR3` | [raw.pixls.us/data/Canon/EOS R8/](https://raw.pixls.us/data/Canon/EOS%20R8/) | 14 MB | CC0 | Canon's compressed RAW format (CRAW). Full-frame R8 sensor. |
| `RAW_FULL_FRAME.CR3` | [raw.pixls.us/data/Canon/EOS R8/](https://raw.pixls.us/data/Canon/EOS%20R8/) | 27 MB | CC0 | Canon's full-precision RAW. Full-frame R8 sensor. |

Downloaded fresh during the Deliverable 3 commit on 2026-05-28.

## EXIF sanitization

Both fixtures were re-processed through `exiftool` before commit to
strip personally-identifiable metadata. The sanitization command:

```sh
exiftool -all= -tagsfromfile @ \
  -Make -Model -Orientation -DateTimeOriginal \
  -ExifImageWidth -ExifImageHeight -Software \
  -overwrite_original <fixture.cr3>
```

This wipes every EXIF / XMP / IPTC tag, then re-inserts only the seven
named tags from the original file. The `MakerNotes` block (Canon's
proprietary metadata) is preserved by ExifTool because the CR3 ISO-BMFF
container's MakerNotes references cannot be safely stripped without
corrupting the file — ExifTool emits a `[minor]` warning about this
limitation, which is expected and accepted (`MakerNotes` does not carry
PII for the R8 firmware revisions shipped publicly).

After sanitization, LibRaw still extracts the six photohelper-required
fields (Make, Model, Orientation, CaptureTime, Width, Height) cleanly.

## Sanitization check

`scripts/sanitize-check.sh` runs from `just ci` and verifies every
`*.CR3` in this directory contains only the asserted-survivor EXIF tag
set. Any unexpected PII tag (GPS / LensSerialNumber / Artist / IPTC
credits / etc.) fails CI.

## Adding new fixtures

If you add a fixture:

1. Source it from a CC0 publisher (raw.pixls.us, public-domain
   collections); verify the license claim.
2. Run the sanitization command above against it.
3. Add a row to the per-fixture provenance table.
4. Confirm `just ci` (which runs `scripts/sanitize-check.sh` and the
   integration tests) stays green.
5. Update `crates/photohelper-raw/tests/common/mod.rs::ALL_FIXTURES`
   if the new fixture should participate in the existing tests.

Fixtures are stored in Git LFS (`*.CR3` is matched by `.gitattributes`).
Contributors need `git lfs install` before first checkout — see the
top-level `README.md § Quickstart`.
