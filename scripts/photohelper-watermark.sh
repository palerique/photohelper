#!/usr/bin/env bash
# scripts/photohelper-watermark.sh
#
# Apply shadow gradient + dual corner marks to a directory of images → JPEG.
#
# Usage:
#   ./scripts/photohelper-watermark.sh \
#       --source <DIR> \
#       --mark1 <PNG> \
#       --mark2 <PNG> \
#       --output <DIR> \
#       [--max-long-edge <N>] \
#       [--force] [--strict] [--allow-untested-raw]
#
# All arguments are forwarded to `cargo run --release ... watermark`.
set -euo pipefail

exec cargo run --release -p photohelper-cli -- watermark "$@"
