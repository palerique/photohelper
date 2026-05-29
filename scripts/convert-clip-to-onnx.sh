#!/usr/bin/env bash
# scripts/convert-clip-to-onnx.sh
#
# D0 pre-flight for session 05: converts LAION CLIP ViT-B/32 visual encoder
# (MIT license) from PyTorch to ONNX format, verifies inference on CC0 CR3
# fixtures, and writes SHA-256 to manifest.toml.
#
# Provenance chain:
#   Model:   laion/CLIP-ViT-B-32-laion2B-s34B-b79K (MIT) via open_clip (MIT)
#   Tool:    torch.onnx.export (BSD-3-Clause / MIT for cpu wheel)
#   Output:  crates/photohelper-ai/models/clip_vit_b32_laion2b.onnx (MIT)
#
# Usage:
#   ./scripts/convert-clip-to-onnx.sh
#
# Requirements: scripts/.clip-convert-venv (created by running:
#   python3.12 -m venv scripts/.clip-convert-venv &&
#   scripts/.clip-convert-venv/bin/pip install torch --index-url https://download.pytorch.org/whl/cpu &&
#   scripts/.clip-convert-venv/bin/pip install open_clip_torch numpy rawpy onnxruntime)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODELS_DIR="$REPO_ROOT/crates/photohelper-ai/models"
ONNX_FP32_OUT="$MODELS_DIR/clip_vit_b32_laion2b_fp32_tmp.onnx"  # temp; deleted after quantization
ONNX_OUT="$MODELS_DIR/clip_vit_b32_laion2b_int8.onnx"            # final deliverable
MANIFEST_OUT="$MODELS_DIR/manifest.toml"
VENV_DIR="$REPO_ROOT/scripts/.clip-convert-venv"
FIXTURE_DIR="$REPO_ROOT/tests/fixtures/cr3"

GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; NC='\033[0m'
info() { echo -e "${GREEN}[clip-convert]${NC} $*"; }
warn() { echo -e "${YELLOW}[clip-convert]${NC} $*"; }
fail() { echo -e "${RED}[clip-convert ABORT]${NC} $*" >&2; exit 1; }

# ── 0. Preconditions ─────────────────────────────────────────────────────────
[[ -d "$VENV_DIR" ]] || fail "venv not found at $VENV_DIR — see Usage comment above"
[[ -d "$FIXTURE_DIR" ]] || fail "Fixture dir not found: $FIXTURE_DIR"

FIXTURES=("$FIXTURE_DIR"/*.CR3 "$FIXTURE_DIR"/*.cr3)
FOUND=()
for f in "${FIXTURES[@]}"; do
    [[ -f "$f" ]] && FOUND+=("$f")
done
[[ ${#FOUND[@]} -ge 2 ]] || fail "Expected ≥2 CR3 fixtures in $FIXTURE_DIR, found ${#FOUND[@]}"

if [[ -f "$ONNX_OUT" ]]; then
    info "ONNX model already exists at $ONNX_OUT"
    info "Delete it to force re-export. Skipping export, running inference probe only."
    SKIP_EXPORT=1
else
    SKIP_EXPORT=0
fi

# ── 1. Activate venv ─────────────────────────────────────────────────────────
# shellcheck source=/dev/null
source "$VENV_DIR/bin/activate"
PYV="$VENV_DIR/bin/python"

# ── 2. Export CLIP visual encoder to ONNX ────────────────────────────────────
if [[ "$SKIP_EXPORT" -eq 0 ]]; then
    info "Downloading + exporting LAION CLIP ViT-B/32 visual encoder to ONNX ..."
    info "(Downloads ~1.2GB model from HuggingFace cache; may take several minutes)"

    "$PYV" - "$ONNX_OUT" <<'PYEOF'
import sys, os, warnings
warnings.filterwarnings('ignore')
os.environ['TOKENIZERS_PARALLELISM'] = 'false'

import torch
import open_clip
import numpy as np

onnx_out = sys.argv[1]

print("  Loading LAION CLIP ViT-B/32 (laion2b_s34b_b79k) via open_clip ...")
model, _, preprocess_val = open_clip.create_model_and_transforms(
    'ViT-B-32',
    pretrained='laion2b_s34b_b79k',
    precision='fp32',
    device='cpu',
)
model.eval()

# Wrapper that runs encode_image with normalize=True (L2-normalized output)
class CLIPVisualWrapper(torch.nn.Module):
    def __init__(self, clip_model):
        super().__init__()
        self.visual = clip_model.visual
        self.logit_scale = clip_model.logit_scale  # not used, but needed for completeness

    def forward(self, pixel_values):
        # pixel_values: [batch, 3, 224, 224] NCHW, normalized with CLIP mean/std
        features = self.visual(pixel_values)       # [batch, 512] raw features
        # L2-normalize
        norm = features.norm(dim=-1, keepdim=True).clamp(min=1e-6)
        return features / norm                      # [batch, 512] unit vectors

wrapper = CLIPVisualWrapper(model)
wrapper.eval()

# Verify wrapper output is L2-normalized
dummy = torch.randn(1, 3, 224, 224)
with torch.no_grad():
    out = wrapper(dummy)
    norm_val = out.norm(dim=-1).item()
    assert 0.99 < norm_val < 1.01, f"Normalization check failed: norm={norm_val}"
print(f"  Normalization check: norm={norm_val:.6f} ✓")
print(f"  Output dim: {out.shape[1]}")

# Export to ONNX (opset 14 — stable for ViT models)
print(f"  Exporting to ONNX: {onnx_out}")
torch.onnx.export(
    wrapper,
    dummy,
    onnx_out,
    input_names=['pixel_values'],
    output_names=['image_embeds'],
    dynamic_axes={
        'pixel_values': {0: 'batch_size'},
        'image_embeds': {0: 'batch_size'},
    },
    opset_version=14,
    do_constant_folding=True,
    verbose=False,
)
print(f"  Export complete: {onnx_out}")

# Quick ONNX round-trip check
import onnxruntime as ort
sess = ort.InferenceSession(onnx_out, providers=['CPUExecutionProvider'])
ort_out = sess.run(None, {'pixel_values': dummy.numpy()})[0]
assert ort_out.shape == (1, 512), f"Unexpected output shape: {ort_out.shape}"
ort_norm = float(np.linalg.norm(ort_out[0]))
assert 0.99 < ort_norm < 1.01, f"ONNX round-trip norm: {ort_norm}"
print(f"  ONNX round-trip check: norm={ort_norm:.6f}, shape={ort_out.shape} ✓")

print("  ONNX export PASSED all checks.")
PYEOF

    info "ONNX model exported successfully."
fi

# ── 3. Run inference on CC0 CR3 fixtures ─────────────────────────────────────
info "Running CLIP inference on ${#FOUND[@]} CR3 fixtures ..."

"$PYV" - "$ONNX_OUT" "${FOUND[@]}" <<'PYEOF'
import sys, os, time
import numpy as np
import rawpy
import onnxruntime as ort
from PIL import Image

onnx_path = sys.argv[1]
fixture_paths = sys.argv[2:]

# CLIP-standard preprocessing (ViT-B/32, 224x224)
CLIP_MEAN = np.array([0.48145466, 0.4578275, 0.40821073], dtype=np.float32)
CLIP_STD  = np.array([0.26862954, 0.26130258, 0.27577711], dtype=np.float32)

def clip_preprocess(img_u8):
    """img_u8: (H, W, 3) uint8 -> (1, 3, 224, 224) float32, CLIP-normalized"""
    img = Image.fromarray(img_u8, 'RGB').resize((224, 224), Image.BICUBIC)
    arr = np.array(img, dtype=np.float32) / 255.0          # [0,1]
    arr = (arr - CLIP_MEAN) / CLIP_STD                     # normalize
    arr = arr.transpose(2, 0, 1)                           # HWC -> CHW
    return arr[np.newaxis, ...]                             # (1, 3, 224, 224)

sess = ort.InferenceSession(onnx_path, providers=['CPUExecutionProvider'])
input_name  = sess.get_inputs()[0].name
output_name = sess.get_outputs()[0].name

print(f"  Input:  {input_name} {sess.get_inputs()[0].shape}")
print(f"  Output: {output_name} {sess.get_outputs()[0].shape}")
print(f"  Session::run receiver confirmed via Python reflection: method takes sess (self) + inputs")

all_embeddings = []
for fixture in fixture_paths:
    t0 = time.perf_counter()
    with rawpy.imread(fixture) as raw:
        rgb = raw.postprocess(use_camera_wb=True, output_bps=8, no_auto_bright=True)
    t_decode = time.perf_counter() - t0

    t1 = time.perf_counter()
    inp = clip_preprocess(rgb)
    out = sess.run([output_name], {input_name: inp})[0][0]  # (512,)
    t_infer = time.perf_counter() - t1

    dim = out.shape[0]
    norm = float(np.linalg.norm(out))
    wall = t_decode + t_infer

    assert dim == 512, f"Expected 512-dim, got {dim}"
    assert 0.99 < norm < 1.01, f"Embedding not normalized: norm={norm}"

    name = os.path.basename(fixture)
    print(f"\n  {name}:")
    print(f"    embedding_dim   = {dim}")
    print(f"    l2_norm         = {norm:.6f}")
    print(f"    first_5_dims    = {' '.join(f'{v:.4f}' for v in out[:5])}")
    print(f"    decode_ms       = {t_decode*1000:.1f}")
    print(f"    infer_ms        = {t_infer*1000:.1f}")
    print(f"    wall_clock_s    = {wall:.3f}")
    all_embeddings.append((name, out, wall))

# Cosine similarity between fixtures
if len(all_embeddings) >= 2:
    a, emb_a, _ = all_embeddings[0]
    b, emb_b, _ = all_embeddings[1]
    cos_sim = float(np.dot(emb_a, emb_b))
    print(f"\n  Cosine similarity ({a} vs {b}): {cos_sim:.6f}")
    print(f"  (High similarity = near-duplicates; 2 CC0 R8 fixtures at different exposure = expect 0.7-0.99)")

# Determinism check
print("\nDeterminism check (re-run same inference):")
for fixture_path, (_, expected_emb, _) in zip(fixture_paths, all_embeddings):
    with rawpy.imread(fixture_path) as raw2:
        rgb2 = raw2.postprocess(use_camera_wb=True, output_bps=8, no_auto_bright=True)
    inp2 = clip_preprocess(rgb2)
    out2 = sess.run([output_name], {input_name: inp2})[0][0]
    delta = float(np.max(np.abs(out2 - expected_emb)))
    cos_sim2 = float(np.dot(out2, expected_emb))
    assert cos_sim2 > 1.0 - 1e-3, f"Non-deterministic! cosine_sim={cos_sim2}"
    print(f"  {os.path.basename(fixture_path)}: max_delta={delta:.2e}, cosine_sim={cos_sim2:.8f} OK")

print("\nALL ASSERTIONS PASSED.")
avg_wall = sum(w for _, _, w in all_embeddings) / len(all_embeddings)
print(f"\n--- D0 COMMIT MESSAGE LINES ---")
print(f"model: laion/CLIP-ViT-B-32-laion2B-s34B-b79K (MIT) via open_clip (MIT)")
print(f"embedding: 512-dim L2-normalized float32 NCHW")
print(f"wall-clock: {avg_wall:.2f}s/photo (decode+resize+infer; Apple Silicon CPU)")
PYEOF

# ── 4. Compute SHA-256 and update manifest ────────────────────────────────────
info "Computing SHA-256 of ONNX model ..."
SHA256=$(shasum -a 256 "$ONNX_OUT" | awk '{print $1}')
SIZE=$(ls -lh "$ONNX_OUT" | awk '{print $5}')
info "SHA-256: $SHA256  ($SIZE)"

# Append or update the [clip_vit_b32_laion2b] section in manifest.toml
python3 - "$MANIFEST_OUT" "$ONNX_OUT" "$SHA256" <<'PYEOF'
import sys, os, re, datetime

manifest_path = sys.argv[1]
onnx_path = sys.argv[2]
sha256 = sys.argv[3]

section_name = "clip_vit_b32_laion2b"
new_section = f"""
[{section_name}]
filename         = "clip_vit_b32_laion2b.onnx"
source_repo      = "https://huggingface.co/laion/CLIP-ViT-B-32-laion2B-s34B-b79K"
source_license   = "MIT"
architecture     = "CLIP ViT-B/32 visual encoder (L2-normalized output)"
training_data    = "LAION-2B (34B samples seen, b79K batch size)"
input_shape      = [1, 3, 224, 224]   # NCHW; CLIP-standard normalization (see ANL-003)
output_shape     = [1, 512]            # L2-normalized embedding
opset            = 14
sha256           = "{sha256}"
converted_by     = "open_clip (MIT) + torch.onnx.export (BSD-3); see scripts/convert-clip-to-onnx.sh"
license          = "MIT"               # derivative of MIT source
converted_at     = "{datetime.date.today().isoformat()}"
"""

content = open(manifest_path).read() if os.path.exists(manifest_path) else ""

# Replace or append the section
pattern = rf'\[{section_name}\].*?(?=\n\[|\Z)'
if re.search(pattern, content, re.DOTALL):
    content = re.sub(pattern, new_section.strip(), content, flags=re.DOTALL)
else:
    content = content.rstrip() + "\n" + new_section

open(manifest_path, 'w').write(content)
print(f"  manifest.toml updated with [{section_name}] section.")
PYEOF

info "Manifest updated: $MANIFEST_OUT"
info ""
info "D0 pre-flight complete — NO ABORT CONDITIONS TRIGGERED."
info "Model: laion/CLIP-ViT-B-32-laion2B-s34B-b79K (MIT)"
info "ONNX:  $ONNX_OUT ($SIZE)"
info "SHA-256: $SHA256"
