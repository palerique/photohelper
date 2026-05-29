#!/usr/bin/env bash
# scripts/verify-model-sha256.sh
#
# CI gate: verify nima_mobilenet_aesthetic.onnx SHA-256 matches manifest.toml.
# Provides fast-fail without compiling the full binary (which would run the
# same check via VerifiedModelBytes::from_manifest at runtime).
#
# Exit codes: 0 = PASS, 1 = FAIL.
set -euo pipefail

MODEL="crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx"
MANIFEST="crates/photohelper-ai/models/manifest.toml"

# ── Check manifest exists ─────────────────────────────────────────────────────
if [[ ! -f "$MANIFEST" ]]; then
    echo "FAIL: manifest not found at $MANIFEST" >&2
    exit 1
fi

# ── Check model file exists and is not a Git LFS pointer ─────────────────────
if [[ ! -f "$MODEL" ]]; then
    echo "FAIL: model file not found at $MODEL" >&2
    echo "  Ensure git-lfs is installed: git lfs install" >&2
    echo "  Then pull the model: git lfs pull" >&2
    exit 1
fi

model_size=$(wc -c < "$MODEL")
if [[ $model_size -lt 1000000 ]]; then
    echo "FAIL: model file is only ${model_size} bytes — likely a Git LFS pointer." >&2
    echo "  Run: git lfs pull" >&2
    exit 1
fi

# ── Extract expected SHA-256 from manifest.toml ───────────────────────────────
expected=$(grep -A 10 '^\[nima_mobilenet_aesthetic\]' "$MANIFEST" \
           | grep 'sha256' \
           | head -1 \
           | sed 's/.*= *"\([^"]*\)".*/\1/')

if [[ -z "$expected" ]]; then
    echo "FAIL: could not extract sha256 from $MANIFEST" >&2
    exit 1
fi

# ── Compute actual SHA-256 ────────────────────────────────────────────────────
actual=$(shasum -a 256 "$MODEL" | awk '{print $1}')

# ── Compare ───────────────────────────────────────────────────────────────────
if [[ "$actual" == "$expected" ]]; then
    echo "PASS: $MODEL SHA-256 verified (${expected:0:8}...)"
    exit 0
else
    echo "FAIL: SHA-256 mismatch for $MODEL" >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    exit 1
fi
