#!/usr/bin/env bash
# scripts/photohelper-rename.sh
#
# Copy RAW+XMP into --output under catalog-driven prefixed names.
#
# Usage:
#   ./scripts/photohelper-rename.sh \
#       --source <DIR> \
#       --output <DIR> \
#       [--catalog <path>] \
#       [--force] [--strict]
#
# All arguments are forwarded to `cargo run --release ... rename`.
set -euo pipefail

exec cargo run --release -p photohelper-cli -- rename "$@"
