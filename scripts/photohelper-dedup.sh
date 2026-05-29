#!/usr/bin/env bash
# scripts/photohelper-dedup.sh
#
# Convenience wrapper: run `photohelper dedup` with PHOTOHELPER_MODEL_DIR
# pointing at crates/photohelper-ai/models/ so the CLIP model is found
# without needing to set the env var manually.
#
# Usage:
#   ./scripts/photohelper-dedup.sh [--catalog <path>] [--similarity-threshold <f>] \
#                                   [--strict] [OTHER FLAGS]
#
# All arguments are forwarded to `cargo run --release ... dedup`.
# Requires the CLIP ONNX model to be present (git lfs pull).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export PHOTOHELPER_MODEL_DIR="$ROOT_DIR/crates/photohelper-ai/models"

exec cargo run --release -p photohelper-cli -- dedup "$@"
