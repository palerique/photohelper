#!/usr/bin/env bash
# scripts/verify-nima-d0-prime.sh
#
# D0' verification: run actual NIMA inference on the CC0 R8 CR3 fixtures and
# record per-fixture aesthetic scores + wall-clock time.
#
# Resolves the empirical half of ANL-002 §Inference end-to-end and
# §Per-photo wall-clock that was blocked by the D0 ABORT in session 03.
#
# Usage:
#   ./scripts/verify-nima-d0-prime.sh
#
# Requires: the .nima-convert-venv created by convert-nima-to-onnx.sh.
# All heavy deps are already in that venv (tensorflow, onnxruntime, numpy).
# rawpy is installed on first run for CR3 decode.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENV_DIR="$REPO_ROOT/scripts/.nima-convert-venv"
ONNX_MODEL="$REPO_ROOT/crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx"
FIXTURE_DIR="$REPO_ROOT/tests/fixtures/cr3"
MANIFEST="$REPO_ROOT/crates/photohelper-ai/models/manifest.toml"

GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
info() { echo -e "${GREEN}[d0-prime]${NC} $*"; }
fail() { echo -e "${RED}[d0-prime ABORT]${NC} $*" >&2; exit 1; }

# ── 0. Preconditions ─────────────────────────────────────────────────────────
[[ -f "$ONNX_MODEL" ]] || fail "ONNX model not found at $ONNX_MODEL — run convert-nima-to-onnx.sh first"
[[ -d "$VENV_DIR" ]]   || fail "venv not found at $VENV_DIR — run convert-nima-to-onnx.sh first"
[[ -d "$FIXTURE_DIR" ]] || fail "Fixture dir not found: $FIXTURE_DIR"

FIXTURES=("$FIXTURE_DIR"/*.CR3 "$FIXTURE_DIR"/*.cr3)
FOUND=()
for f in "${FIXTURES[@]}"; do
    [[ -f "$f" ]] && FOUND+=("$f")
done
[[ ${#FOUND[@]} -ge 2 ]] || fail "Expected ≥2 CR3 fixtures in $FIXTURE_DIR, found ${#FOUND[@]}"

# ── 1. Activate venv and install rawpy if needed ──────────────────────────────
# shellcheck source=/dev/null
source "$VENV_DIR/bin/activate"
PYV="$VENV_DIR/bin/python"

if ! "$PYV" -c "import rawpy" 2>/dev/null; then
    info "Installing rawpy into venv ..."
    pip install --quiet rawpy
    info "rawpy installed."
fi

# ── 2. Run inference on each fixture ─────────────────────────────────────────
info "Running NIMA inference on ${#FOUND[@]} CR3 fixtures ..."

"$PYV" - "$ONNX_MODEL" "${FOUND[@]}" <<'PYEOF'
import sys, time, os
os.environ['TF_CPP_MIN_LOG_LEVEL'] = '3'

import rawpy
import numpy as np
import onnxruntime as ort

model_path = sys.argv[1]
fixture_paths = sys.argv[2:]

# MobileNet preprocess_input: maps [0,255] -> [-1,1] per channel (A3 amendment).
def mobilenet_preprocess(img_u8):
    """img_u8: (H, W, 3) uint8 -> (1, 224, 224, 3) float32 in [-1, 1]"""
    from PIL import Image
    img = Image.fromarray(img_u8, 'RGB').resize((224, 224), Image.BILINEAR)
    arr = np.array(img, dtype=np.float32)
    arr = arr / 127.5 - 1.0          # MobileNet preprocess_input
    return arr[np.newaxis, ...]       # (1, 224, 224, 3)

sess = ort.InferenceSession(model_path, providers=['CPUExecutionProvider'])
input_name = sess.get_inputs()[0].name

all_scores = []
for fixture in fixture_paths:
    t0 = time.perf_counter()
    with rawpy.imread(fixture) as raw:
        rgb = raw.postprocess(
            use_camera_wb=True,
            output_bps=8,
            no_auto_bright=True,
        )                             # returns (H, W, 3) uint8
    t_decode = time.perf_counter() - t0

    t1 = time.perf_counter()
    inp = mobilenet_preprocess(rgb)
    outputs = sess.run(None, {input_name: inp})
    t_infer = time.perf_counter() - t1

    probs = outputs[0][0]             # (10,) softmax
    mean_score = sum((i + 1) * float(p) for i, p in enumerate(probs))
    wall_clock = t_decode + t_infer

    assert abs(probs.sum() - 1.0) < 1e-4, f"score distribution sums to {probs.sum()}"
    assert 1.0 <= mean_score <= 10.0, f"score out of range: {mean_score}"

    name = os.path.basename(fixture)
    print(f"  {name}:")
    print(f"    aesthetic_score = {mean_score:.6f}")
    print(f"    softmax_10      = {' '.join(f'{p:.4f}' for p in probs)}")
    print(f"    decode_ms       = {t_decode*1000:.1f}")
    print(f"    infer_ms        = {t_infer*1000:.1f}")
    print(f"    wall_clock_s    = {wall_clock:.3f}")
    all_scores.append((name, mean_score, probs, wall_clock))

# Determinism check: re-run with identical parameters and compare
print("\nDeterminism check (re-run same inference):")
for fixture_path, (_, expected_score, expected_probs, _) in zip(fixture_paths, all_scores):
    with rawpy.imread(fixture_path) as raw2:
        rgb2 = raw2.postprocess(
            use_camera_wb=True,
            output_bps=8,
            no_auto_bright=True,
        )
    inp2 = mobilenet_preprocess(rgb2)
    out2 = sess.run(None, {input_name: inp2})[0][0]
    mean2 = sum((i + 1) * float(p) for i, p in enumerate(out2))
    delta = abs(mean2 - expected_score)
    assert delta < 1e-3, f"Non-deterministic! delta={delta}"
    print(f"  {os.path.basename(fixture_path)}: delta={delta:.2e} OK")

print("\nALL ASSERTIONS PASSED.")
print("\n--- COMMIT MESSAGE LINES ---")
scores_str = ', '.join(f'{s:.4f}' for _, s, _, _ in all_scores)
avg_wall = sum(w for _, _, _, w in all_scores) / len(all_scores)
print(f"inference: {len(all_scores)}/{len(all_scores)} fixtures, scores [{scores_str}]")
print(f"wall-clock: {avg_wall:.2f}s/photo (decode + resize + infer on Apple Silicon)")
sha = open(sys.argv[1].replace('nima_mobilenet_aesthetic.onnx', 'manifest.toml')).read()
import re
sha256 = re.search(r'sha256\s*=\s*"([^"]+)"', sha).group(1)[:8]
print(f"dn-026: closed (path-a, sha256={sha256}...)")
PYEOF

# ── 3. Report ─────────────────────────────────────────────────────────────────
echo ""
info "D0' verification complete — no ABORT conditions triggered."
echo "  Use the commit message lines above in the D0' commit."
