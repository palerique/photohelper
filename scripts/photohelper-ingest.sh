#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <photos_dir> [extra ingest args]" >&2
  echo "Example: $0 /path/to/photos --strict" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PHOTOS_DIR="$1"
shift

cargo run --release --manifest-path "$ROOT_DIR/Cargo.toml" -p photohelper-cli -- ingest "$PHOTOS_DIR" "$@"
