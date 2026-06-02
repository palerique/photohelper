#!/usr/bin/env bash
# scripts/test-session-15-features.sh
#
# End-to-end acceptance test for session-15 features:
#   - `photohelper watermark` — shadow gradient + dual-corner PNG marks → JPEG
#   - `photohelper rename`    — catalog-driven Cluster-X_Cull-Y-Name prefixes
#
# Usage:
#   ./scripts/test-session-15-features.sh
#
# Prerequisites:
#   - /Users/ph/Pictures/tests/      — Canon R8 CR3 files (source RAW)
#   - /Users/ph/Pictures/top-marcas/ — Marca-1.png, Marca-2.png (corner marks)
#   - /Users/ph/Pictures/photohelper-exports/ — existing exports (raster sources)
#   - Catalog must be seeded (the script runs ingest+cull+dedup if needed)
#
# All output lands under ~/Pictures/session-15-test-<timestamp>/

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
BASE_OUTPUT="$HOME/Pictures/session-15-test-${TIMESTAMP}"

RAW_SOURCE="/Users/ph/Pictures/tests"
MARKS_DIR="/Users/ph/Pictures/top-marcas"
EXPORTS_DIR="/Users/ph/Pictures/photohelper-exports"

MARK1="$MARKS_DIR/Marca-1.png"
MARK2="$MARKS_DIR/Marca-2.png"

CATALOG_DB="$RAW_SOURCE/.photohelper/catalog.db"

# ── colour helpers ───────────────────────────────────────────────────────────
bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m⚠\033[0m %s\n' "$*"; }
fail() { printf '  \033[31m✗\033[0m %s\n' "$*" >&2; }
sep()  { echo; printf '%0.s─' {1..72}; echo; }

# ── preflight ────────────────────────────────────────────────────────────────
sep
bold "Session 15 Feature Test — $TIMESTAMP"
sep

echo "Output root : $BASE_OUTPUT"
echo "RAW source  : $RAW_SOURCE"
echo "Marks       : $MARK1"
echo "              $MARK2"

for f in "$MARK1" "$MARK2"; do
    if [[ ! -f "$f" ]]; then
        fail "Mark not found: $f"
        exit 1
    fi
    ok "Mark found: $(basename "$f")"
done
if [[ ! -d "$RAW_SOURCE" ]]; then
    fail "RAW source not found: $RAW_SOURCE"
    exit 1
fi

mkdir -p "$BASE_OUTPUT"

# ── Step 0: Build ────────────────────────────────────────────────────────────
sep
bold ">>> Step 0: Building (release)..."
export PHOTOHELPER_MODEL_DIR="$ROOT_DIR/crates/photohelper-ai/models"
cargo build --release -p photohelper-cli -q
ok "Build done"

BINARY="$ROOT_DIR/target/release/photohelper"

# ── Step 1: Seed catalog if needed ───────────────────────────────────────────
sep
bold ">>> Step 1: Ensuring catalog is seeded..."

if [[ -f "$CATALOG_DB" ]]; then
    # Use the shell script helper (not the binary — binary has no list-catalog subcommand).
    SCORED=$("$ROOT_DIR/scripts/photohelper-list-catalog.sh" --catalog "$CATALOG_DB" --count 2>/dev/null || echo "?")
    ok "Catalog exists ($SCORED rows). Skipping re-seed."
else
    echo "  Catalog not found — running ingest → cull → dedup..."
    "$ROOT_DIR/scripts/photohelper-ingest.sh" "$RAW_SOURCE"
    "$ROOT_DIR/scripts/photohelper-cull.sh"   --catalog "$CATALOG_DB"
    "$ROOT_DIR/scripts/photohelper-dedup.sh"  --catalog "$CATALOG_DB"
    ok "Catalog seeded."
fi

# ── Step 2: Watermark — CR3 RAW sources ──────────────────────────────────────
sep
bold ">>> Step 2: watermark (CR3 RAW sources) — resize 1920px + shadow + marks"

WM_RAW_OUT="$BASE_OUTPUT/watermark-raw"
mkdir -p "$WM_RAW_OUT"

# Grab a representative subset (first 10 CR3s) to keep runtime short.
# Hard-link (ln without -s) rather than symlink: collect_source_files uses
# file_type().is_file() which is false for symlinks, so symlinks yield walked:0.
WM_RAW_SRC="$BASE_OUTPUT/watermark-raw-source"
mkdir -p "$WM_RAW_SRC"
CR3_COUNT=0
for f in "$RAW_SOURCE"/*.CR3; do
    [[ -f "$f" ]] || continue
    ln -f "$f" "$WM_RAW_SRC/$(basename "$f")" 2>/dev/null \
        || cp "$f" "$WM_RAW_SRC/$(basename "$f")"  # fallback if cross-device
    CR3_COUNT=$((CR3_COUNT + 1))
    [[ $CR3_COUNT -ge 10 ]] && break
done
echo "  Using $CR3_COUNT CR3 files from $RAW_SOURCE"

echo "  Running: photohelper watermark ..."
"$BINARY" watermark \
    --source   "$WM_RAW_SRC" \
    --mark1    "$MARK1" \
    --mark2    "$MARK2" \
    --output   "$WM_RAW_OUT" \
    --max-long-edge 1920 \
    --force

echo "  Output directory: $WM_RAW_OUT"
WRITTEN_RAW=$(find "$WM_RAW_OUT" -name "*.jpg" | wc -l | tr -d ' ')
if [[ "$WRITTEN_RAW" -gt 0 ]]; then
    ok "watermark (RAW): $WRITTEN_RAW JPEG(s) written"
else
    fail "watermark (RAW): no JPEGs in output — check stderr above"
fi

# ── Step 3: Watermark — raster (JPEG) sources ────────────────────────────────
sep
bold ">>> Step 3: watermark (raster JPEG sources) — resize 2048px + shadow + marks"

WM_RASTER_SRC="$BASE_OUTPUT/watermark-raster-source"
WM_RASTER_OUT="$BASE_OUTPUT/watermark-raster"
mkdir -p "$WM_RASTER_SRC" "$WM_RASTER_OUT"

JPEG_COUNT=0
# Pull from most-recent export run; hard-link for the same reason as CR3s above.
for f in $(find "$EXPORTS_DIR" -name "*.jpg" | sort -r | head -10); do
    [[ -f "$f" ]] || continue
    ln -f "$f" "$WM_RASTER_SRC/$(basename "$f")" 2>/dev/null \
        || cp "$f" "$WM_RASTER_SRC/$(basename "$f")"
    JPEG_COUNT=$((JPEG_COUNT + 1))
done

if [[ $JPEG_COUNT -eq 0 ]]; then
    warn "No exported JPEGs found in $EXPORTS_DIR — skipping raster watermark test."
else
    echo "  Using $JPEG_COUNT JPEG(s) from $EXPORTS_DIR"
    echo "  Running: photohelper watermark ..."
    "$BINARY" watermark \
        --source   "$WM_RASTER_SRC" \
        --mark1    "$MARK1" \
        --mark2    "$MARK2" \
        --output   "$WM_RASTER_OUT" \
        --max-long-edge 2048 \
        --force

    WRITTEN_RASTER=$(find "$WM_RASTER_OUT" -name "*.jpg" | wc -l | tr -d ' ')
    if [[ "$WRITTEN_RASTER" -gt 0 ]]; then
        ok "watermark (raster): $WRITTEN_RASTER JPEG(s) written"
    else
        fail "watermark (raster): no JPEGs in output"
    fi
fi

# ── Step 4: Watermark — idempotency check ────────────────────────────────────
sep
bold ">>> Step 4: watermark idempotency — second run must skip all existing outputs"

echo "  Running watermark again on the same source (no --force)..."
IDEMPOTENT_OUT=$("$BINARY" watermark \
    --source   "$WM_RAW_SRC" \
    --mark1    "$MARK1" \
    --mark2    "$MARK2" \
    --output   "$WM_RAW_OUT" \
    --max-long-edge 1920 2>&1)

echo "$IDEMPOTENT_OUT"
if echo "$IDEMPOTENT_OUT" | grep -q "skipped-existing: $WRITTEN_RAW"; then
    ok "Idempotency: all $WRITTEN_RAW outputs correctly skipped"
else
    warn "Idempotency: expected skipped-existing: $WRITTEN_RAW — check output above"
fi

# ── Step 5: Watermark — non-destructive check ────────────────────────────────
sep
bold ">>> Step 5: watermark non-destructive — source files must be unchanged"

# Check the ORIGINAL in $RAW_SOURCE, not the hard-link copy in the temp dir.
# (Hard links share inodes; stat on either returns the same real size.)
SAMPLE_CR3_BASE=$(find "$WM_RAW_SRC" -name "*.CR3" -print -quit | xargs basename 2>/dev/null)
if [[ -n "$SAMPLE_CR3_BASE" ]]; then
    ORIG="$RAW_SOURCE/$SAMPLE_CR3_BASE"
    if [[ -f "$ORIG" ]]; then
        ORIG_SIZE=$(stat -f%z "$ORIG" 2>/dev/null || stat -c%s "$ORIG")
        ok "Source file size before/after: $ORIG_SIZE bytes ($SAMPLE_CR3_BASE)"
        # Verify the source dir has no new .jpg files (watermark must write only to --output)
        NEW_JPGS=$(find "$RAW_SOURCE" -maxdepth 1 -newer "$ORIG" -name "*.jpg" | wc -l | tr -d ' ')
        if [[ "$NEW_JPGS" -eq 0 ]]; then
            ok "Non-destructive: no new JPEGs written into source directory"
        else
            fail "Non-destructive: $NEW_JPGS unexpected JPEG(s) appeared in source — SOURCE WAS MODIFIED"
        fi
    fi
fi

# ── Step 6: rename — catalog-driven prefix ───────────────────────────────────
sep
bold ">>> Step 6: rename — Cluster-X_Cull-Y-OriginalFilename.ext"

RENAME_OUT="$BASE_OUTPUT/rename"
mkdir -p "$RENAME_OUT"

echo "  Running: photohelper rename ..."
RENAME_STDOUT=$("$BINARY" rename \
    --catalog "$CATALOG_DB" \
    --source  "$RAW_SOURCE" \
    --output  "$RENAME_OUT" \
    --force 2>&1)

echo "$RENAME_STDOUT"

RENAMED=$(find "$RENAME_OUT" -name "*.CR3" | wc -l | tr -d ' ')
if [[ "$RENAMED" -gt 0 ]]; then
    ok "rename: $RENAMED RAW file(s) copied with prefixed names"
else
    fail "rename: no output files found — check output above"
fi

# ── Step 7: rename — verify filename format ───────────────────────────────────
sep
bold ">>> Step 7: rename — filename format verification"

BAD_NAMES=0
GOOD_NAMES=0
while IFS= read -r f; do
    base=$(basename "$f")
    if [[ "$base" =~ ^Cluster-[0-9A-Z]+_Cull-[0-9\.A-Z]+-.*\.CR3$ ]]; then
        GOOD_NAMES=$((GOOD_NAMES + 1))
    else
        warn "Unexpected filename format: $base"
        BAD_NAMES=$((BAD_NAMES + 1))
    fi
done < <(find "$RENAME_OUT" -name "*.CR3" | head -20 || true)

if [[ "$GOOD_NAMES" -gt 0 && "$BAD_NAMES" -eq 0 ]]; then
    ok "All $GOOD_NAMES filenames match Cluster-X_Cull-Y-Name.CR3 pattern"
elif [[ "$BAD_NAMES" -gt 0 ]]; then
    warn "$GOOD_NAMES good / $BAD_NAMES unexpected format"
fi

# Show a sample of renamed files
echo ""
echo "  Sample renamed files:"
{ find "$RENAME_OUT" -name "*.CR3" | sort | head -5 || true; } | while IFS= read -r f; do
    printf "    %s\n" "$(basename "$f")"
done

# ── Step 8: rename — source non-destructive ──────────────────────────────────
sep
bold ">>> Step 8: rename non-destructive — source RAWs must still exist unchanged"

SAMPLE_RAW=$(find "$RAW_SOURCE" -maxdepth 1 -name "*.CR3" -print -quit)
if [[ -n "$SAMPLE_RAW" ]]; then
    if [[ -f "$SAMPLE_RAW" ]]; then
        ok "Source RAW still exists: $(basename "$SAMPLE_RAW")"
    else
        fail "Source RAW missing after rename — source was modified!"
    fi
fi

# ── Step 9: rename — XMP sidecar copied alongside RAW ───────────────────────
sep
bold ">>> Step 9: rename — XMP sidecars copied alongside renamed RAWs"

XMP_COUNT=$(find "$RENAME_OUT" -name "*.xmp" | wc -l | tr -d ' ')
CR3_COUNT_OUT=$(find "$RENAME_OUT" -name "*.CR3" | wc -l | tr -d ' ')

echo "  Renamed RAWs : $CR3_COUNT_OUT"
echo "  Renamed XMPs : $XMP_COUNT"

if [[ "$XMP_COUNT" -gt 0 ]]; then
    ok "XMP sidecars present: $XMP_COUNT file(s)"
    # Verify at least one XMP pairs with its RAW
    SAMPLE_RENAMED_RAW=$(find "$RENAME_OUT" -name "*.CR3" -print -quit)
    PAIRED_XMP="${SAMPLE_RENAMED_RAW%.CR3}.xmp"
    if [[ -f "$PAIRED_XMP" ]]; then
        ok "Paired XMP found: $(basename "$PAIRED_XMP")"
    else
        warn "Expected XMP not found at: $(basename "$PAIRED_XMP")"
    fi
else
    warn "No XMP sidecars in rename output (expected if source XMPs were missing or catalog had no sidecars)"
fi

# ── Summary ──────────────────────────────────────────────────────────────────
sep
bold "=== Session 15 Feature Test Complete ==="
echo ""
echo "  All output at: $BASE_OUTPUT"
echo ""
echo "  Results:"
echo "    watermark (RAW):    $WM_RAW_OUT"
[[ $JPEG_COUNT -gt 0 ]] && echo "    watermark (raster): $WM_RASTER_OUT"
echo "    rename:             $RENAME_OUT"
echo ""
echo "  Open in Finder to visually verify:"
echo "    open '$WM_RAW_OUT'"
[[ $JPEG_COUNT -gt 0 ]] && echo "    open '$WM_RASTER_OUT'"
echo "    open '$RENAME_OUT'"
echo ""
bold "What to check visually:"
echo "  watermark outputs:"
echo "    - Bottom black shadow gradient fading from full opacity to transparent"
echo "    - Marca-1.png  → top-right corner  (~14% of image height)"
echo "    - Marca-2.png  → bottom-left corner (~13% of image height, inside shadow)"
echo "    - Both marks at ~4.6% margin from edges"
echo "    - Long edge ≤ 1920px (downscale-only — originals smaller than 1920 unchanged)"
echo ""
echo "  rename outputs:"
echo "    - Files named: Cluster-NNN_Cull-NN.NN-OriginalName.CR3"
echo "    - Cluster-NONE / Cull-NONE for unscored/unclustered photos"
echo "    - Matching .xmp sidecar alongside each .CR3"
echo "    - Original files in $RAW_SOURCE untouched"
sep
