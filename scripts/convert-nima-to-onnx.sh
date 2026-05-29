#!/usr/bin/env bash
# scripts/convert-nima-to-onnx.sh
#
# Resolves DN-026: converts idealo/image-quality-assessment MobileNet aesthetic
# weights (Apache-2.0) from Keras .hdf5 to ONNX format, verifies inference,
# and writes a SHA-256 manifest.
#
# Provenance chain:
#   Source:  https://github.com/idealo/image-quality-assessment (Apache-2.0)
#   Weights: models/MobileNet/weights_mobilenet_aesthetic_0.07.hdf5
#   Tool:    tf2onnx (Apache-2.0)
#   Output:  crates/photohelper-ai/models/nima_mobilenet_aesthetic.onnx (Apache-2.0)
#
# Usage:
#   ./scripts/convert-nima-to-onnx.sh
#
# Requirements: python3.12 (via Homebrew on macOS, or system on Linux)
# All Python deps are installed in a local .nima-convert-venv/ — nothing global.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODELS_DIR="$REPO_ROOT/crates/photohelper-ai/models"
ONNX_OUT="$MODELS_DIR/nima_mobilenet_aesthetic.onnx"
HDF5_OUT="$MODELS_DIR/weights_mobilenet_aesthetic_0.07.hdf5"
MANIFEST_OUT="$MODELS_DIR/manifest.toml"
VENV_DIR="$REPO_ROOT/scripts/.nima-convert-venv"

HDF5_URL="https://github.com/idealo/image-quality-assessment/raw/refs/heads/master/models/MobileNet/weights_mobilenet_aesthetic_0.07.hdf5"

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[nima-convert]${NC} $*"; }
warn()  { echo -e "${YELLOW}[nima-convert]${NC} $*"; }

# ── 0. Skip if ONNX already exists ────────────────────────────────────────────
if [[ -f "$ONNX_OUT" ]]; then
    info "ONNX model already exists at $ONNX_OUT"
    info "Delete it and re-run to force reconversion."
    exit 0
fi

# ── 1. Find Python 3.12 ───────────────────────────────────────────────────────
PY=""
for candidate in /opt/homebrew/bin/python3.12 /usr/bin/python3.12 python3.12 python3; do
    if command -v "$candidate" &>/dev/null; then
        ver=$("$candidate" -c "import sys; print(sys.version_info[:2])")
        if [[ "$ver" == "(3, 12)" || "$ver" == "(3, 11)" || "$ver" == "(3, 10)" ]]; then
            PY="$candidate"; break
        fi
    fi
done
[[ -z "$PY" ]] && { echo "ERROR: python3.10-3.12 required (3.14 is too new for tensorflow)"; exit 1; }
info "Using Python: $PY ($($PY --version))"

# ── 2. Create virtual environment ─────────────────────────────────────────────
if [[ ! -d "$VENV_DIR" ]]; then
    info "Creating venv at $VENV_DIR ..."
    "$PY" -m venv "$VENV_DIR"
fi
# shellcheck source=/dev/null
source "$VENV_DIR/bin/activate"
PYV="$VENV_DIR/bin/python"

# ── 3. Install dependencies ───────────────────────────────────────────────────
info "Installing Python dependencies (this may take a few minutes on first run) ..."
pip install --quiet --upgrade pip
# tensorflow: arm64-native wheels available since 2.13
pip install --quiet "tensorflow>=2.16" "tf2onnx>=1.16" "onnxruntime>=1.18" "onnx>=1.14" numpy Pillow
info "Dependencies installed."

# ── 4. Download .hdf5 weights ─────────────────────────────────────────────────
mkdir -p "$MODELS_DIR"
if [[ ! -f "$HDF5_OUT" ]]; then
    info "Downloading MobileNet NIMA aesthetic weights (~12 MB) ..."
    curl -fL --progress-bar "$HDF5_URL" -o "$HDF5_OUT"
    info "Download complete: $HDF5_OUT"
else
    info "Weights already downloaded: $HDF5_OUT"
fi

# Verify the hdf5 is not a LFS pointer (GitHub sometimes returns pointer text)
file_size=$(wc -c < "$HDF5_OUT")
if [[ $file_size -lt 1000000 ]]; then
    echo "ERROR: Downloaded file is only ${file_size} bytes — likely a Git LFS pointer, not the real weights."
    echo "Try downloading directly:"
    echo "  curl -fL '$HDF5_URL' -o '$HDF5_OUT'"
    rm -f "$HDF5_OUT"
    exit 1
fi
info "Weights file size: ${file_size} bytes — looks correct."

# ── 5. Convert .hdf5 → ONNX ──────────────────────────────────────────────────
info "Converting Keras model to ONNX (opset 13) ..."
"$PYV" - "$HDF5_OUT" "$ONNX_OUT" <<'PYEOF'
import sys, os
os.environ['TF_CPP_MIN_LOG_LEVEL'] = '2'  # suppress TF startup noise

import tensorflow as tf
import tf2onnx
import onnx
import numpy as np

hdf5_path = sys.argv[1]
onnx_path = sys.argv[2]

# Try loading as a complete SavedModel/Keras model first.
# If that fails (weights-only file), reconstruct the architecture.
print(f"Loading weights from: {hdf5_path}")
try:
    model = tf.keras.models.load_model(hdf5_path, compile=False)
    print("Loaded as full Keras model (architecture included in hdf5).")
except Exception as e:
    print(f"Full-model load failed ({e}); reconstructing architecture ...")
    # Architecture matches idealo/image-quality-assessment src/handlers/model_builder.py
    # Base: MobileNet, input (224,224,3), include_top=False, pooling='avg'
    # Head: Dense(10, softmax)  [dropout_rate=0 → no-op, omitted for ONNX cleanliness]
    from tensorflow.keras.applications.mobilenet import MobileNet
    from tensorflow.keras.models import Model
    from tensorflow.keras.layers import Dense

    base = MobileNet(input_shape=(224, 224, 3), weights=None, include_top=False, pooling='avg')
    out  = Dense(units=10, activation='softmax')(base.output)
    model = Model(base.inputs, out)
    model.load_weights(hdf5_path)
    print("Architecture reconstructed and weights loaded.")

print(f"Model input:  {model.input_shape}")
print(f"Model output: {model.output_shape}")

# Convert to ONNX
input_sig = [tf.TensorSpec(shape=(None, 224, 224, 3), dtype=tf.float32, name='input')]
model_proto, _ = tf2onnx.convert.from_keras(model, input_signature=input_sig, opset=13)
onnx.save(model_proto, onnx_path)
print(f"ONNX model saved to: {onnx_path}")

# Smoke test: run inference on a random image
import onnxruntime as ort
sess = ort.InferenceSession(onnx_path, providers=['CPUExecutionProvider'])
dummy = np.random.rand(1, 224, 224, 3).astype(np.float32)
# Note: production callers must apply mobilenet preprocess_input (scales [0,255]→[-1,1])
# before passing images. This smoke test uses raw random values.
outputs = sess.run(None, {'input': dummy})
scores = outputs[0][0]   # shape: (10,) — probability per rating 1-10
mean_score = sum((i + 1) * s for i, s in enumerate(scores))
print(f"Smoke test: scores sum = {scores.sum():.6f} (expected ~1.0)")
print(f"Smoke test: mean aesthetic score = {mean_score:.4f} (range [1,10])")
assert abs(scores.sum() - 1.0) < 1e-4, f"Score distribution does not sum to 1: {scores.sum()}"
assert 1.0 <= mean_score <= 10.0, f"Mean score out of range: {mean_score}"
print("Smoke test PASSED.")
PYEOF

info "Conversion complete."

# ── 6. Compute SHA-256 ────────────────────────────────────────────────────────
SHA256=$(shasum -a 256 "$ONNX_OUT" | awk '{print $1}')
info "SHA-256: $SHA256"

# ── 7. Write manifest.toml ───────────────────────────────────────────────────
cat > "$MANIFEST_OUT" << TOML
# crates/photohelper-ai/models/manifest.toml
# Generated by scripts/convert-nima-to-onnx.sh
# Provenance: idealo/image-quality-assessment (Apache-2.0) via tf2onnx (Apache-2.0)

[nima_mobilenet_aesthetic]
filename     = "nima_mobilenet_aesthetic.onnx"
source_repo  = "https://github.com/idealo/image-quality-assessment"
source_license = "Apache-2.0"
source_weights = "models/MobileNet/weights_mobilenet_aesthetic_0.07.hdf5"
architecture = "MobileNet (include_top=false, pooling=avg) + Dense(10, softmax)"
input_shape  = [1, 224, 224, 3]   # NHWC; preprocessing: mobilenet_preprocess_input (→ [-1,1])
output_shape = [1, 10]             # softmax distribution over ratings 1-10
opset        = 13
sha256       = "$SHA256"
converted_by = "tf2onnx (Apache-2.0)"
license      = "Apache-2.0"        # derivative of Apache-2.0 source
TOML

info "Manifest written to $MANIFEST_OUT"

# ── 8. Summary ────────────────────────────────────────────────────────────────
echo ""
echo "══════════════════════════════════════════════════════"
echo "  NIMA ONNX conversion complete — DN-026 RESOLVED"
echo "══════════════════════════════════════════════════════"
echo "  ONNX model : $ONNX_OUT"
echo "  SHA-256    : $SHA256"
echo "  Manifest   : $MANIFEST_OUT"
echo "  License    : Apache-2.0 (derivative of idealo/iqa)"
echo ""
echo "  Next: start session 04 and run D0 re-verification."
echo "══════════════════════════════════════════════════════"
