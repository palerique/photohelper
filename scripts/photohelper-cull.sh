#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PHOTOHELPER_MODEL_DIR="$ROOT_DIR/crates/photohelper-ai/models" \
  cargo run --release --manifest-path "$ROOT_DIR/Cargo.toml" -p photohelper-cli -- cull "$@"
