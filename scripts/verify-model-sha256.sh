#!/usr/bin/env bash
# scripts/verify-model-sha256.sh
#
# CI gate: verify all ONNX models in manifest.toml have correct SHA-256.
# Iterates over every [section] in manifest.toml, reads filename + sha256,
# and verifies each model file. Fails fast on first mismatch.
#
# Exit codes: 0 = all PASS, 1 = any FAIL.
set -euo pipefail

MODELS_DIR="crates/photohelper-ai/models"
MANIFEST="$MODELS_DIR/manifest.toml"

GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
pass() { echo -e "${GREEN}PASS${NC}: $*"; }
fail() { echo -e "${RED}FAIL${NC}: $*" >&2; exit 1; }

# ── Check manifest exists ─────────────────────────────────────────────────────
[[ -f "$MANIFEST" ]] || fail "manifest not found at $MANIFEST"

# ── Parse all [section] headers from manifest.toml ────────────────────────────
SECTIONS=()
while IFS= read -r line; do
    if [[ "$line" =~ ^\[([a-zA-Z0-9_]+)\] ]]; then
        SECTIONS+=("${BASH_REMATCH[1]}")
    fi
done < "$MANIFEST"

[[ ${#SECTIONS[@]} -gt 0 ]] || fail "no [sections] found in $MANIFEST"

echo "Verifying ${#SECTIONS[@]} model(s) from $MANIFEST ..."

# ── Verify each model ─────────────────────────────────────────────────────────
for section in "${SECTIONS[@]}"; do
    # Extract filename field; fall back to {section}.onnx for backward compat.
    raw_filename=$(grep -A 20 "^\[$section\]" "$MANIFEST" \
        | grep -m1 'filename' \
        | sed 's/.*= *"\([^"]*\)".*/\1/' || true)
    filename="${raw_filename:-${section}.onnx}"

    model="$MODELS_DIR/$filename"

    # Extract expected SHA-256
    expected=$(grep -A 20 "^\[$section\]" "$MANIFEST" \
        | grep -m1 'sha256' \
        | sed 's/.*= *"\([^"]*\)".*/\1/')

    [[ -n "$expected" ]] || fail "[$section] missing sha256 in $MANIFEST"

    # Check file exists (not just a Git LFS pointer)
    if [[ ! -f "$model" ]]; then
        fail "model file not found: $model  (run: git lfs pull)"
    fi

    model_size=$(wc -c < "$model")
    if [[ $model_size -lt 1000000 ]]; then
        fail "[$section] file is only ${model_size} bytes — likely a Git LFS pointer. Run: git lfs pull"
    fi

    # Compute and compare SHA-256
    actual=$(shasum -a 256 "$model" | awk '{print $1}')
    if [[ "$actual" == "$expected" ]]; then
        pass "[$section] $filename SHA-256 verified (${expected:0:8}...)"
    else
        fail "SHA-256 mismatch for [$section] $filename
  expected: $expected
  actual:   $actual"
    fi
done

echo "All ${#SECTIONS[@]} model SHA-256 checks passed."
