#!/usr/bin/env bash
# photohelper-develop — write Lightroom-compatible XMP sidecars for ingested photos.
#
# Usage:
#   scripts/photohelper-develop.sh [OPTIONS]
#
# Examples:
#   # Write XMP sidecars for all photos in the default catalog:
#   scripts/photohelper-develop.sh
#
#   # With a specific catalog and develop settings:
#   scripts/photohelper-develop.sh \
#       --catalog ~/Pictures/.photohelper/catalog.db \
#       --temp 5500 --exposure 0.3
#
#   # Force-overwrite existing sidecars (skip conflict check):
#   scripts/photohelper-develop.sh --catalog /path/to/catalog.db --force
#
# Writes <photo>.xmp (Lightroom-compatible: extension replaced, not appended)
# alongside each ingested RAW file. Conflict resolution preserves existing
# Lightroom edits if the sidecar was modified after our last write.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PHOTOHELPER_MODEL_DIR="${PHOTOHELPER_MODEL_DIR:-$ROOT_DIR/crates/photohelper-ai/models}"
export PHOTOHELPER_MODEL_DIR

cargo run --release --manifest-path "$ROOT_DIR/Cargo.toml" -p photohelper-cli -- develop --auto-tone --lr-label-score "$@"
