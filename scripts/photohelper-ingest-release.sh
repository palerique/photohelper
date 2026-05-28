#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <photos_dir> [extra ingest args]" >&2
  echo "Example: $0 /path/to/photos --strict" >&2
  exit 64
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT_DIR/target/release/photohelper"
PHOTOS_DIR="$1"
shift

if [[ ! -x "$BIN" ]]; then
  echo "Release binary not found at $BIN; building it now..." >&2
  cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release --all-features --workspace
fi

"$BIN" ingest "$PHOTOS_DIR" "$@"
