#!/usr/bin/env bash
# scripts/stress-test-session-15.sh
#
# Full-pipeline stress test for session 15:
#
#   1. Clean slate  — wipe catalog + XMPs
#   2. Ingest       — catalog all 370 Canon R8 CR3s
#   3. Cull         — NIMA aesthetic scoring (AI)
#   4. Cluster      — CLIP embedding + cosine-similarity dedup
#   5. Develop      — write Lightroom-compatible XMP sidecars (rating + keywords)
#   6. Export HD    — render ALL photos as high-quality JPEGs at 4000px long-edge
#                     (quality 95, min-rating 0 = every photo regardless of score)
#   7. Watermark HD — composite both PNG marks onto every exported JPEG
#                     at native export resolution (no resize)
#   8. Summary      — per-step timing, file counts, output sizes
#
# Usage:
#   ./scripts/stress-test-session-15.sh [--source <dir>] [--dry-run]
#
# Defaults:
#   --source  /Users/ph/Pictures/tests
#   Output    ~/Pictures/stress-test-<timestamp>/
#
# Marks: /Users/ph/Pictures/top-marcas/Marca-1.png (top-right)
#         /Users/ph/Pictures/top-marcas/Marca-2.png (bottom-left)
#
# Disk estimate: ~3–6 GB of JPEG output (export + watermark).

set -euo pipefail

# ── cli args ─────────────────────────────────────────────────────────────────
SOURCE_DIR="/Users/ph/Pictures/tests"
DRY_RUN=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --source) SOURCE_DIR="${2%/}"; shift 2;;
        --dry-run) DRY_RUN=1; shift;;
        *) echo "Unknown arg: $1" >&2; exit 64;;
    esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
BASE_OUT="$HOME/Pictures/stress-test-${TIMESTAMP}"

MARKS_DIR="/Users/ph/Pictures/top-marcas"
MARK1="$MARKS_DIR/Marca-1.png"
MARK2="$MARKS_DIR/Marca-2.png"

CATALOG_DB="$SOURCE_DIR/.photohelper/catalog.db"

# HD parameters
EXPORT_QUALITY=95
EXPORT_LONG_EDGE=4000   # Canon R8 native: 6022×4024; 4000px is near-full-res

EXPORT_OUT="$BASE_OUT/export-hd"
WATERMARK_OUT="$BASE_OUT/watermark-hd"

# ── timing helpers ────────────────────────────────────────────────────────────
T_BUILD=0; T_CLEAN=0; T_INGEST=0; T_CULL=0
T_CLUSTER=0; T_DEVELOP=0; T_EXPORT=0; T_WATERMARK=0

ts() { date +%s; }
elapsed() { echo $(( $(ts) - $1 )); }
hms() {
    local s=$1
    printf "%02d:%02d:%02d" $(( s/3600 )) $(( (s%3600)/60 )) $(( s%60 ))
}

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
ok()    { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn()  { printf '  \033[33m⚠\033[0m %s\n' "$*"; }
fail()  { printf '  \033[31m✗\033[0m %s\n' "$*" >&2; }
step()  { echo; printf '\033[1;36m>>> %s\033[0m\n' "$*"; }
sep()   { echo; printf '%0.s─' {1..72}; echo; }
info()  { printf '  %-30s %s\n' "$1" "$2"; }

run()   {
    if [[ $DRY_RUN -eq 1 ]]; then
        echo "  [dry-run] $*"
    else
        "$@"
    fi
}

# ── preflight ─────────────────────────────────────────────────────────────────
sep
bold "Photohelper Stress Test — Session 15 — $TIMESTAMP"
[[ $DRY_RUN -eq 1 ]] && warn "DRY-RUN mode: commands will be printed, not executed"
sep

info "Source RAW dir:"   "$SOURCE_DIR"
info "Output root:"      "$BASE_OUT"
info "Mark1 (top-right):" "$MARK1"
info "Mark2 (bot-left):" "$MARK2"
info "Export quality:"   "$EXPORT_QUALITY / 100"
info "Export long-edge:" "${EXPORT_LONG_EDGE}px (downscale-only from ~6022px native)"

# Verify prerequisites
for f in "$MARK1" "$MARK2"; do
    [[ -f "$f" ]] || { fail "Mark not found: $f"; exit 1; }
done
CR3_TOTAL=$(find "$SOURCE_DIR" -maxdepth 1 -name "*.CR3" | wc -l | tr -d ' ')
[[ "$CR3_TOTAL" -gt 0 ]] || { fail "No CR3 files in $SOURCE_DIR"; exit 1; }
ok "$CR3_TOTAL CR3 files ready in $SOURCE_DIR"

export PHOTOHELPER_MODEL_DIR="$ROOT_DIR/crates/photohelper-ai/models"
mkdir -p "$EXPORT_OUT" "$WATERMARK_OUT"

# ── Step 0: Build ─────────────────────────────────────────────────────────────
step "Step 0 — Release build"
T0=$(ts)
run cargo build --release -p photohelper-cli -q
BINARY="$ROOT_DIR/target/release/photohelper"
T_BUILD=$(elapsed $T0)
ok "Build done in $(hms $T_BUILD)"

# ── Step 1: Clean slate ───────────────────────────────────────────────────────
step "Step 1 — Clean slate (catalog + XMPs)"
T1=$(ts)

run "$ROOT_DIR/scripts/photohelper-clean-catalog.sh" "$SOURCE_DIR" --yes

XMP_COUNT=$(find "$SOURCE_DIR" -maxdepth 1 -name "*.xmp" | wc -l | tr -d ' ')
if [[ "$XMP_COUNT" -gt 0 ]]; then
    echo "  Removing $XMP_COUNT XMP sidecars..."
    if [[ $DRY_RUN -eq 0 ]]; then
        find "$SOURCE_DIR" -maxdepth 1 -name "*.xmp" -delete
    else
        echo "  [dry-run] find $SOURCE_DIR -maxdepth 1 -name '*.xmp' -delete"
    fi
fi
T_CLEAN=$(elapsed $T1)
ok "Clean done in $(hms $T_CLEAN)"

# ── Step 2: Ingest ────────────────────────────────────────────────────────────
step "Step 2 — Ingest ($CR3_TOTAL CR3 files)"
T2=$(ts)
run "$ROOT_DIR/scripts/photohelper-ingest.sh" "$SOURCE_DIR"
T_INGEST=$(elapsed $T2)
ok "Ingest done in $(hms $T_INGEST)"

# ── Step 3: Cull ──────────────────────────────────────────────────────────────
step "Step 3 — Cull (NIMA aesthetic scoring — AI)"
T3=$(ts)
run "$ROOT_DIR/scripts/photohelper-cull.sh" --catalog "$CATALOG_DB"
T_CULL=$(elapsed $T3)
ok "Cull done in $(hms $T_CULL)"

# ── Step 4: Cluster ───────────────────────────────────────────────────────────
step "Step 4 — Cluster (CLIP embedding + dedup)"
T4=$(ts)
run "$ROOT_DIR/scripts/photohelper-dedup.sh" --catalog "$CATALOG_DB"
T_CLUSTER=$(elapsed $T4)
ok "Cluster done in $(hms $T_CLUSTER)"

# ── Step 5: Develop ───────────────────────────────────────────────────────────
step "Step 5 — Develop (write Lightroom XMP sidecars)"
T5=$(ts)
run "$ROOT_DIR/scripts/photohelper-develop.sh" \
    --catalog "$CATALOG_DB" \
    --lr-rating --lr-keywords --force
T_DEVELOP=$(elapsed $T5)
ok "Develop done in $(hms $T_DEVELOP)"

# ── Step 6: Export HD ─────────────────────────────────────────────────────────
step "Step 6 — Export HD (quality $EXPORT_QUALITY, long-edge ${EXPORT_LONG_EDGE}px, ALL photos)"
echo "  Output: $EXPORT_OUT"
echo "  Note: --min-rating 0 exports ALL scored photos (stress test — no curation filter)"
T6=$(ts)
run "$BINARY" export \
    --catalog    "$CATALOG_DB" \
    --output     "$EXPORT_OUT" \
    --quality    "$EXPORT_QUALITY" \
    --long-edge  "$EXPORT_LONG_EDGE" \
    --min-rating 0 \
    --force
T_EXPORT=$(elapsed $T6)

EXPORTED=$(find "$EXPORT_OUT" -name "*.jpg" | wc -l | tr -d ' ' || echo 0)
EXPORT_SIZE=$(du -sh "$EXPORT_OUT" 2>/dev/null | awk '{print $1}' || echo "?")
ok "Export done in $(hms $T_EXPORT) — $EXPORTED JPEG(s) written, total size: $EXPORT_SIZE"

# ── Step 7: Watermark HD ──────────────────────────────────────────────────────
step "Step 7 — Watermark HD (shadow + marks on all $EXPORTED exported JPEGs)"
echo "  Source: $EXPORT_OUT (HD JPEGs at ${EXPORT_LONG_EDGE}px)"
echo "  Output: $WATERMARK_OUT"
echo "  No --max-long-edge: watermarked outputs keep full export resolution"
echo "  Mark1 (top-right  ~14% height): $(basename "$MARK1")"
echo "  Mark2 (bottom-left ~13% height): $(basename "$MARK2")"
T7=$(ts)
run "$BINARY" watermark \
    --source "$EXPORT_OUT" \
    --mark1  "$MARK1" \
    --mark2  "$MARK2" \
    --output "$WATERMARK_OUT" \
    --force
T_WATERMARK=$(elapsed $T7)

WATERMARKED=$(find "$WATERMARK_OUT" -name "*.jpg" | wc -l | tr -d ' ' || echo 0)
WM_SIZE=$(du -sh "$WATERMARK_OUT" 2>/dev/null | awk '{print $1}' || echo "?")
ok "Watermark done in $(hms $T_WATERMARK) — $WATERMARKED JPEG(s) written, total size: $WM_SIZE"

# ── Summary ───────────────────────────────────────────────────────────────────
sep
bold "=== Stress Test Complete — $TIMESTAMP ==="
echo ""
bold "Timing:"
info "  Build:"     "$(hms $T_BUILD)"
info "  Clean:"     "$(hms $T_CLEAN)"
info "  Ingest:"    "$(hms $T_INGEST)   ($CR3_TOTAL files)"
info "  Cull:"      "$(hms $T_CULL)   (NIMA AI scoring)"
info "  Cluster:"   "$(hms $T_CLUSTER)   (CLIP embedding + dedup)"
info "  Develop:"   "$(hms $T_DEVELOP)   (XMP sidecars)"
info "  Export HD:" "$(hms $T_EXPORT)   ($EXPORTED JPEGs at q=$EXPORT_QUALITY, ${EXPORT_LONG_EDGE}px)"
info "  Watermark:" "$(hms $T_WATERMARK)   ($WATERMARKED watermarked JPEGs)"

TOTAL=$(( T_BUILD + T_CLEAN + T_INGEST + T_CULL + T_CLUSTER + T_DEVELOP + T_EXPORT + T_WATERMARK ))
echo ""
info "  TOTAL wall-clock:" "$(hms $TOTAL)"
echo ""

bold "Output:"
info "  Export HD JPEGs:"      "$EXPORT_OUT ($EXPORT_SIZE)"
info "  Watermarked JPEGs:"    "$WATERMARK_OUT ($WM_SIZE)"
echo ""
echo "  Open in Finder:"
echo "    open '$EXPORT_OUT'"
echo "    open '$WATERMARK_OUT'"
echo ""
bold "Visual checks:"
echo "  Export: correct aspect ratio; filmic look; long-edge ≤ ${EXPORT_LONG_EDGE}px"
echo "  Watermark:"
echo "    · Bottom shadow gradient (100%→0% over bottom 30% of height)"
echo "    · Marca-1 → top-right  corner (~14% height, ~4.6% margin)"
echo "    · Marca-2 → bottom-left corner (~13% height, inside shadow band)"
echo "    · Full ${EXPORT_LONG_EDGE}px resolution preserved (no resize)"
sep
