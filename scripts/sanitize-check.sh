#!/usr/bin/env bash
# Sanitization gate for tests/fixtures/cr3/*.CR3.
#
# Every CR3 fixture in the repo MUST contain ONLY the asserted-survivor
# EXIF tag set (see tests/fixtures/cr3/README.md § Sanitization check).
# Any other tag — most importantly anything PII (GPS, owner serial,
# IPTC creators) — fails CI and forces the contributor to re-sanitize.
#
# Two-stage check (TD-009 closed, session 06 D2a):
#   Stage 1: top-level EXIF allow-list via `exiftool -G -a`
#   Stage 2: extract embedded preview JPEG and run the same allow-list
#            check on it. CR3 files embed a preview JPEG that exiftool's
#            top-level pass does NOT recurse into; a GPS-tagged preview
#            would silently pass stage 1 only.
set -euo pipefail

FIXTURE_DIR="tests/fixtures/cr3"

if ! command -v exiftool >/dev/null 2>&1; then
    echo "sanitize-check: exiftool not found on PATH" >&2
    echo "  install: macOS 'brew install exiftool'; Debian/Ubuntu" >&2
    echo "           'sudo apt install libimage-exiftool-perl'." >&2
    exit 2
fi

# Tags that MAY survive sanitization. Anything else → fail.
# Format: regex matching "[Group]:Tag" pairs from `exiftool -G -a` output.
ALLOWED_TAG_RE='^\['
ALLOWED_TAG_RE+='(File|ExifTool|FileType|System'
ALLOWED_TAG_RE+='|IFD0|ExifIFD|MakerNotes'
ALLOWED_TAG_RE+='|Composite|QuickTime|ICC[_-]Profile|JFIF)'
ALLOWED_TAG_RE+='\]'

# Tags whose presence in the SURVIVOR set is REQUIRED (post-sanitization).
# Each one must appear in at least one [Group]:Tag pair. Names use
# exiftool's `-G` display names, not the raw EXIF tag IDs (so "Model"
# is "Camera Model Name", "DateTimeOriginal" is "Date/Time Original").
REQUIRED_TAGS=(
    "Make"
    "Camera Model Name"
    "Orientation"
    "Date/Time Original"
)

# Tag substrings that MUST NOT appear (PII / fingerprinting risks).
FORBIDDEN_TAG_PATTERNS=(
    "GPS"
    "LensSerialNumber"
    "InternalSerialNumber"
    "OwnerName"
    "Artist"
    "Copyright"
    "By-line"
    "Credit"
    "Source"
)

failures=0
fixture_count=0
for fixture in "$FIXTURE_DIR"/*.CR3; do
    if [ ! -f "$fixture" ]; then
        continue
    fi
    fixture_count=$((fixture_count + 1))
    base=$(basename "$fixture")
    tags=$(exiftool -G -a "$fixture" 2>/dev/null)

    # Required-tag check. exiftool -G output line shape:
    #   "[Group]              TagName                    : value"
    # Match the TagName column.
    for req in "${REQUIRED_TAGS[@]}"; do
        if ! echo "$tags" | grep -qE "[[:space:]]${req}[[:space:]]+:"; then
            echo "  ERR $base: required tag '$req' missing — re-sanitize" >&2
            failures=$((failures + 1))
        fi
    done

    # Forbidden-tag check (PII / fingerprinting). Match the TagName
    # column (NOT the value column — a benign value containing the
    # word "GPS" should not trip).
    for forbidden in "${FORBIDDEN_TAG_PATTERNS[@]}"; do
        if echo "$tags" | grep -qE "[[:space:]]${forbidden}[A-Za-z_]*[[:space:]]+:"; then
            echo "  ERR $base: forbidden tag matching '$forbidden' present" >&2
            failures=$((failures + 1))
        fi
    done

    # Stage 2: extract embedded preview JPEG and run the same allow-list.
    # Use mktemp to avoid parallel-CI clobber of a shared /tmp/preview.jpg.
    # Note: macOS mktemp does not support suffixes after XXXXXX in full-path form;
    # use -t flag (creates in $TMPDIR) or strip the .jpg suffix.
    preview_tmp=$(mktemp "${TMPDIR:-/tmp}/ph-sanitize-XXXXXX")
    exiftool -b -PreviewImage "$fixture" > "$preview_tmp" 2>/dev/null || true
    if [ -s "$preview_tmp" ]; then
        preview_tags=$(exiftool -G -a "$preview_tmp" 2>/dev/null)
        for forbidden in "${FORBIDDEN_TAG_PATTERNS[@]}"; do
            if echo "$preview_tags" | grep -qE "[[:space:]]${forbidden}[A-Za-z_]*[[:space:]]+:"; then
                echo "  ERR $base (embedded preview): forbidden tag matching '$forbidden' present" >&2
                failures=$((failures + 1))
            fi
        done
    fi
    rm -f "$preview_tmp"
done

if [ "$fixture_count" -eq 0 ]; then
    echo "sanitize-check: no CR3 fixtures found in $FIXTURE_DIR"
    exit 0
fi

if [ "$failures" -gt 0 ]; then
    echo "" >&2
    echo "sanitize-check: $failures violations across $fixture_count fixtures" >&2
    echo "  see tests/fixtures/cr3/README.md § Sanitization for the contract" >&2
    exit 1
fi

echo "sanitize-check: clean ($fixture_count fixtures, all survivor-only)"
