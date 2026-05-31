#!/usr/bin/env bash
# photohelper-all.sh — run the full photohelper pipeline from scratch
#
# Usage:
#   scripts/photohelper-all.sh <target_dir>
#
# Pipeline steps:
#   1. Clean catalog
#   2. Remove existing XMP sidecars
#   3. Ingest photos
#   4. Cull (AI scoring)
#   5. Develop (Write Lightroom XMP sidecars)
#   6. Export (Render JPEGs to <target_dir>/exports-<timestamp>)

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <target_dir>" >&2
    echo "Example: $0 /Users/ph/Pictures/tests/" >&2
    exit 64
fi

TARGET_DIR="${1%/}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Verify target directory exists
if [[ ! -d "$TARGET_DIR" ]]; then
    echo "Error: Target directory '$TARGET_DIR' does not exist." >&2
    exit 1
fi

echo "=== Starting Full Pipeline for '$TARGET_DIR' ==="

# 1. Clean Catalog
echo ""
echo ">>> Step 1: Cleaning catalog..."
"$ROOT_DIR/scripts/photohelper-clean-catalog.sh" "$TARGET_DIR" --yes

# 2. Remove Sidecar Metadata
echo ""
echo ">>> Step 2: Removing existing XMP sidecars..."
find "$TARGET_DIR" -maxdepth 1 -name "*.xmp" -type f -delete
echo "Removed .xmp files in $TARGET_DIR"

CATALOG_DB="$TARGET_DIR/.photohelper/catalog.db"

# 3. Ingest
echo ""
echo ">>> Step 3: Ingesting photos..."
"$ROOT_DIR/scripts/photohelper-ingest.sh" "$TARGET_DIR"

# 4. Cull
echo ""
echo ">>> Step 4: Culling (AI scoring)..."
"$ROOT_DIR/scripts/photohelper-cull.sh" --catalog "$CATALOG_DB"

# 4.5. Dedup (Clustering)
echo ""
echo ">>> Step 4.5: Deduplicating (Clustering)..."
"$ROOT_DIR/scripts/photohelper-dedup.sh" --catalog "$CATALOG_DB"

# 5. Develop
echo ""
echo ">>> Step 5: Developing (Writing XMP sidecars)..."
"$ROOT_DIR/scripts/photohelper-develop.sh" --catalog "$CATALOG_DB" --lr-rating --lr-keywords --force

# 6. Export
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
EXPORT_DIR="$TARGET_DIR/exports-${TIMESTAMP}"
echo ""
echo ">>> Step 6: Exporting to $EXPORT_DIR..."
mkdir -p "$EXPORT_DIR"
"$ROOT_DIR/scripts/photohelper-export.sh" --catalog "$CATALOG_DB" --output "$EXPORT_DIR" --long-edge 1920 --watermark "PHOTOHELPER" --force

echo ""
echo "=== Full Pipeline Completed Successfully ==="
