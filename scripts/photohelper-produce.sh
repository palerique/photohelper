#!/usr/bin/env bash
# scripts/photohelper-produce.sh
#
# All-in-one production script: ingest → cull → cluster → develop → export+watermark (single-pass).
# Output is a single folder of final watermarked JPEGs, resized and AI-selected.
#
# Usage:
#   ./scripts/photohelper-produce.sh \
#       --source   <dir>       Source directory (CR3 RAW and/or JPEG/PNG)
#       --mark1    <png>       Top-right corner mark (PNG)
#       --mark2    <png>       Bottom-left corner mark (PNG)
#       --max-long-edge <N>    Long-edge limit in pixels (≥16)
#       [--output  <dir>]      Output directory (default: ~/Pictures/produce-<timestamp>)
#       [--min-rating <0-5>]   Minimum AI star rating to export (default: 3)
#       [--quality <1-100>]    JPEG quality for exports (default: 90)
#       [--force]              Overwrite existing output files
#
# For RAW (CR3) sources the full pipeline runs:
#   Ingest → Cull (AI) → Cluster (dedup) → Develop (XMP) → Export+Watermark (single-pass)
#   (co-located JPEG/PNG files are watermarked separately via the watermark subcommand)
#
# For raster-only sources (JPEG/PNG only) the catalog steps are skipped:
#   Watermark directly at the requested long-edge

set -euo pipefail

cleanup() { [[ -n "${RASTER_TEMP:-}" ]] && rm -rf "$RASTER_TEMP"; }
trap cleanup EXIT

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
export PHOTOHELPER_MODEL_DIR="$ROOT_DIR/crates/photohelper-ai/models"
BINARY="$ROOT_DIR/target/release/photohelper"

# ── defaults ──────────────────────────────────────────────────────────────────
SOURCE_DIR=""
MARK1=""
MARK2=""
MAX_LONG_EDGE=""
OUTPUT_DIR="$HOME/Pictures/produce-${TIMESTAMP}"
MIN_RATING=3
QUALITY=90
FORCE=""

# ── parse args ────────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --source)        SOURCE_DIR="${2%/}"; shift 2;;
        --mark1)         MARK1="$2";          shift 2;;
        --mark2)         MARK2="$2";          shift 2;;
        --max-long-edge) MAX_LONG_EDGE="$2";  shift 2;;
        --output)        OUTPUT_DIR="${2%/}";  shift 2;;
        --min-rating)    MIN_RATING="$2";     shift 2;;
        --quality)       QUALITY="$2";        shift 2;;
        --force)         FORCE="--force";     shift;;
        -h|--help)
            grep '^#' "$0" | sed 's/^# \?//'
            exit 0;;
        *) echo "Unknown argument: $1" >&2; exit 64;;
    esac
done

# ── validate required args ────────────────────────────────────────────────────
MISSING=0
for arg in SOURCE_DIR MARK1 MARK2 MAX_LONG_EDGE; do
    [[ -n "${!arg}" ]] || { echo "Missing required: --${arg//_/-}" | tr '[:upper:]' '[:lower:]' | sed 's/_/-/g' >&2; MISSING=1; }
done
[[ $MISSING -eq 0 ]] || { echo "Run with --help for usage." >&2; exit 64; }

[[ -d "$SOURCE_DIR" ]] || { echo "Source directory not found: $SOURCE_DIR" >&2; exit 1; }
[[ -f "$MARK1" ]]      || { echo "Mark1 not found: $MARK1" >&2; exit 1; }
[[ -f "$MARK2" ]]      || { echo "Mark2 not found: $MARK2" >&2; exit 1; }

# ── helpers ───────────────────────────────────────────────────────────────────
ts()      { date +%s; }
elapsed() { echo $(( $(ts) - $1 )); }
hms()     { local s=$1; printf "%02d:%02d:%02d" $(( s/3600 )) $(( (s%3600)/60 )) $(( s%60 )); }
bold()    { printf '\033[1m%s\033[0m\n' "$*"; }
ok()      { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn()    { printf '  \033[33m⚠\033[0m %s\n' "$*"; }
step()    { echo; printf '\033[1;36m>>> %s\033[0m\n' "$*"; }
sep()     { echo; printf '%0.s─' {1..72}; echo; }
info()    { printf '  %-28s %s\n' "$1" "$2"; }

# ── timing accumulators ───────────────────────────────────────────────────────
TOTAL_START=$(ts)
T_BUILD=0 T_INGEST=0 T_CULL=0 T_CLUSTER=0 T_DEVELOP=0 T_EXPORT=0 T_RASTER=0

# ── detect source type ────────────────────────────────────────────────────────
CR3_COUNT=$(find "$SOURCE_DIR" -maxdepth 1 -name "*.CR3" | wc -l | tr -d ' ')
RAW_PIPELINE=0
[[ "$CR3_COUNT" -gt 0 ]] && RAW_PIPELINE=1

# ── banner ────────────────────────────────────────────────────────────────────
sep
bold "Photohelper Produce — $TIMESTAMP"
sep
info "Source:"         "$SOURCE_DIR"
info "Mark1 (top-right):" "$(basename "$MARK1")"
info "Mark2 (bot-left):"  "$(basename "$MARK2")"
info "Max long-edge:"  "${MAX_LONG_EDGE}px"
info "Quality:"        "$QUALITY / 100"
info "Output:"         "$OUTPUT_DIR"
RASTER_BANNER_COUNT=$(find "$SOURCE_DIR" -maxdepth 1 \
    \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" \) | wc -l | tr -d ' ')
# Warn about unsupported formats (JXL, HEIC, TIFF, etc.)
UNSUPPORTED_COUNT=$(find "$SOURCE_DIR" -maxdepth 1 \
    \( -iname "*.jxl" -o -iname "*.heic" -o -iname "*.heif" -o -iname "*.tif" -o -iname "*.tiff" -o -iname "*.webp" \) | wc -l | tr -d ' ')
if [[ "$UNSUPPORTED_COUNT" -gt 0 ]]; then
    warn "Found $UNSUPPORTED_COUNT file(s) in unsupported formats (JXL/HEIC/TIFF/WebP)."
    warn "Only JPEG and PNG are supported for direct watermarking."
    warn "Export them from Lightroom as JPEG first."
    find "$SOURCE_DIR" -maxdepth 1 \
        \( -iname "*.jxl" -o -iname "*.heic" -o -iname "*.heif" -o -iname "*.tif" -o -iname "*.tiff" -o -iname "*.webp" \) \
        -exec basename {} \; | head -5 | while read -r f; do printf "    ⚠️  %s\n" "$f"; done
fi
if [[ $RAW_PIPELINE -eq 1 ]]; then
    if [[ "$RASTER_BANNER_COUNT" -gt 0 ]]; then
        info "Pipeline:"   "RAW + raster — $CR3_COUNT CR3s (catalog) + $RASTER_BANNER_COUNT JPEG/PNG (direct)"
    else
        info "Pipeline:"   "RAW — ingest→cull→cluster→develop→export+watermark single-pass ($CR3_COUNT CR3s)"
    fi
    info "Min rating:" "$MIN_RATING / 5 (AI curation threshold)"
else
    info "Pipeline:"   "raster-only — watermark directly (no catalog)"
fi

mkdir -p "$OUTPUT_DIR"

# ── build ─────────────────────────────────────────────────────────────────────
step "Build (release)"
T=$(ts)
cargo build --release -p photohelper-cli -q
T_BUILD=$(elapsed $T)
ok "Done in $(hms $T_BUILD)"

# ═══════════════════════════════════════════════════════════════════════════════
# RAW PIPELINE
# ═══════════════════════════════════════════════════════════════════════════════
if [[ $RAW_PIPELINE -eq 1 ]]; then

    CATALOG_DB="$SOURCE_DIR/.photohelper/catalog.db"
    EXPORT_DIR="$OUTPUT_DIR/export"
    WATERMARK_DIR="$OUTPUT_DIR/watermarked"
    mkdir -p "$EXPORT_DIR" "$WATERMARK_DIR"

    # ── ingest ────────────────────────────────────────────────────────────────
    step "Ingest ($CR3_COUNT CR3 files → catalog)"
    T=$(ts)
    "$ROOT_DIR/scripts/photohelper-ingest.sh" "$SOURCE_DIR"
    T_INGEST=$(elapsed $T)
    ok "Done in $(hms $T_INGEST)"

    # ── cull ──────────────────────────────────────────────────────────────────
    step "Cull (NIMA aesthetic scoring)"
    T=$(ts)
    "$ROOT_DIR/scripts/photohelper-cull.sh" --catalog "$CATALOG_DB"
    T_CULL=$(elapsed $T)
    ok "Done in $(hms $T_CULL)"

    # ── cluster ───────────────────────────────────────────────────────────────
    step "Cluster (CLIP dedup — cosine-similarity)"
    T=$(ts)
    "$ROOT_DIR/scripts/photohelper-dedup.sh" --catalog "$CATALOG_DB"
    T_CLUSTER=$(elapsed $T)
    ok "Done in $(hms $T_CLUSTER)"

    # ── develop ───────────────────────────────────────────────────────────────
    step "Develop (write Lightroom XMP sidecars)"
    T=$(ts)
    "$ROOT_DIR/scripts/photohelper-develop.sh" \
        --catalog "$CATALOG_DB" --lr-rating --lr-keywords --force
    T_DEVELOP=$(elapsed $T)
    ok "Done in $(hms $T_DEVELOP)"

    # ── export + marks in one filmic-ISP pass (saves ~30% wall-clock) ─────────
    # --mark1-png/--mark2-png apply shadow+marks inside the same encode step,
    # eliminating a second JPEG decode/encode cycle versus separate watermark.
    step "Export+Watermark → JPEG (q=$QUALITY, ${MAX_LONG_EDGE}px, min-rating≥$MIN_RATING, single-pass)"
    T=$(ts)
    # Allow exit 2 (partial failure / mark-doesnt-fit for narrow images); abort on other codes.
    EX_EXIT=0
    "$BINARY" export \
        --catalog    "$CATALOG_DB" \
        --output     "$WATERMARK_DIR" \
        --quality    "$QUALITY" \
        --long-edge  "$MAX_LONG_EDGE" \
        --min-rating "$MIN_RATING" \
        --mark1-png  "$MARK1" \
        --mark2-png  "$MARK2" \
        --with-shadow \
        ${FORCE} || EX_EXIT=$?
    if [[ $EX_EXIT -ne 0 && $EX_EXIT -ne 2 ]]; then
        echo "Export failed (exit $EX_EXIT). Aborting." >&2
        exit $EX_EXIT
    fi
    if [[ $EX_EXIT -eq 2 ]]; then
        warn "Some images could not be watermarked during export (mark-doesnt-fit for narrow images)."
    fi
    WATERMARKED=$(find "$WATERMARK_DIR" -name "*.jpg" | wc -l | tr -d ' ')
    WM_SIZE=$(du -sh "$WATERMARK_DIR" 2>/dev/null | awk '{print $1}')
    if [[ "$WATERMARKED" -eq 0 ]]; then
        warn "Export produced 0 JPEGs. Check that the source directory contains CR3 files rated ≥$MIN_RATING."
    fi
    T_EXPORT=$(elapsed $T)
    ok "Done in $(hms $T_EXPORT) — $WATERMARKED JPEG(s) exported+watermarked ($WM_SIZE)"

    # ── watermark raster files from source (JPEG/PNG alongside the CR3s) ─────────
    RASTER_SOURCE_COUNT=$(find "$SOURCE_DIR" -maxdepth 1 \
        \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" \) | wc -l | tr -d ' ')
    if [[ "$RASTER_SOURCE_COUNT" -gt 0 ]]; then
        # Isolate raster files in a temp dir so watermark doesn't re-process CR3s.
        RASTER_TEMP="$OUTPUT_DIR/.raster-sources"
        mkdir -p "$RASTER_TEMP"
        find "$SOURCE_DIR" -maxdepth 1 \
            \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" \) | while IFS= read -r f; do
            ln -f "$f" "$RASTER_TEMP/$(basename "$f")" 2>/dev/null \
                || cp "$f" "$RASTER_TEMP/$(basename "$f")"
        done
        step "Watermark raster sources ($RASTER_SOURCE_COUNT JPEG/PNG from source — ${MAX_LONG_EDGE}px)"
        T=$(ts)
        # Allow exit 2 (partial failure / mark-doesnt-fit); abort on other non-zero codes.
        WM_EXIT=0
        "$BINARY" watermark \
            --source "$RASTER_TEMP" \
            --mark1  "$MARK1" \
            --mark2  "$MARK2" \
            --output "$WATERMARK_DIR" \
            --max-long-edge "$MAX_LONG_EDGE" \
            ${FORCE} || WM_EXIT=$?
        rm -rf "$RASTER_TEMP"
        if [[ $WM_EXIT -ne 0 && $WM_EXIT -ne 2 ]]; then
            echo "Raster watermark failed (exit $WM_EXIT). Aborting." >&2
            exit $WM_EXIT
        fi
        WATERMARKED=$(find "$WATERMARK_DIR" -name "*.jpg" | wc -l | tr -d ' ')
        WM_SIZE=$(du -sh "$WATERMARK_DIR" 2>/dev/null | awk '{print $1}')
        if [[ $WM_EXIT -eq 2 ]]; then
            warn "Some raster files could not be watermarked (mark-doesnt-fit for narrow images)."
            warn "Those images are too narrow for the current mark sizes. See warnings above."
        fi
        T_RASTER=$(elapsed $T)
        ok "Done in $(hms $T_RASTER) — $WATERMARKED total JPEG(s) in output ($WM_SIZE)"
    fi

    FINAL_DIR="$WATERMARK_DIR"
    FINAL_COUNT=$(find "$WATERMARK_DIR" -name "*.jpg" | wc -l | tr -d ' ')

# ═══════════════════════════════════════════════════════════════════════════════
# RASTER-ONLY PIPELINE
# ═══════════════════════════════════════════════════════════════════════════════
else

    WATERMARK_DIR="$OUTPUT_DIR/watermarked"
    mkdir -p "$WATERMARK_DIR"

    RASTER_COUNT=$(find "$SOURCE_DIR" -maxdepth 2 \( -iname "*.jpg" -o -iname "*.jpeg" -o -iname "*.png" \) | wc -l | tr -d ' ')
    step "Watermark ($RASTER_COUNT raster files — shadow + marks at ${MAX_LONG_EDGE}px)"
    T=$(ts)
    # Allow exit 2 (partial failure / mark-doesnt-fit); abort on other non-zero codes.
    WM_EXIT=0
    "$BINARY" watermark \
        --source "$SOURCE_DIR" \
        --mark1  "$MARK1" \
        --mark2  "$MARK2" \
        --output "$WATERMARK_DIR" \
        --max-long-edge "$MAX_LONG_EDGE" \
        ${FORCE} || WM_EXIT=$?
    if [[ $WM_EXIT -ne 0 && $WM_EXIT -ne 2 ]]; then
        echo "Watermark failed (exit $WM_EXIT). Aborting." >&2
        exit $WM_EXIT
    fi
    if [[ $WM_EXIT -eq 2 ]]; then
        warn "Some files could not be watermarked (mark-doesnt-fit for narrow images)."
        warn "Those images are too narrow for the current mark sizes. See warnings above."
    fi
    WATERMARKED=$(find "$WATERMARK_DIR" -name "*.jpg" | wc -l | tr -d ' ')
    WM_SIZE=$(du -sh "$WATERMARK_DIR" 2>/dev/null | awk '{print $1}')
    if [[ "$WATERMARKED" -eq 0 ]]; then
        warn "No files were watermarked. Check that the source directory contains JPEG/PNG images."
    fi
    T_RASTER=$(elapsed $T)
    ok "Done in $(hms $T_RASTER) — $WATERMARKED JPEG(s) watermarked ($WM_SIZE)"

    FINAL_DIR="$WATERMARK_DIR"
    FINAL_COUNT="$WATERMARKED"

fi

# ── timing summary ────────────────────────────────────────────────────────────
T_TOTAL=$(elapsed $TOTAL_START)

timing_row() {
    local label="$1" secs="$2"
    [[ "$secs" -eq 0 ]] && return
    printf '  %-26s %s\n' "$label" "$(hms $secs)"
}

sep
bold "⏱  Timing summary"
echo ""
timing_row "Build"              $T_BUILD
timing_row "Ingest"             $T_INGEST
timing_row "Cull"               $T_CULL
timing_row "Cluster"            $T_CLUSTER
timing_row "Develop"            $T_DEVELOP
timing_row "Export+Watermark"   $T_EXPORT
timing_row "Raster watermark"   $T_RASTER
echo "  ──────────────────────────────────"
printf '  %-26s %s\n' "Total" "$(hms $T_TOTAL)"
echo ""

# ── benchmark log (tab-separated, appendable for cross-run comparison) ────────
BENCH_LOG="$OUTPUT_DIR/benchmark.log"
{
    printf "timestamp\ttotal\tbuild\tingest\tcull\tcluster\tdevelop\texport\traster\tfiles\tsource\n"
    printf "%s\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%s\t%s\n" \
        "$TIMESTAMP" "$T_TOTAL" "$T_BUILD" "$T_INGEST" "$T_CULL" \
        "$T_CLUSTER" "$T_DEVELOP" "$T_EXPORT" "$T_RASTER" \
        "$FINAL_COUNT" "$SOURCE_DIR"
} > "$BENCH_LOG"
info "Benchmark log:" "$BENCH_LOG"

# ── final output summary ──────────────────────────────────────────────────────
sep
bold "=== Done ==="
echo ""
info "Final output:" "$FINAL_DIR"
info "Files:"        "$FINAL_COUNT watermarked JPEG(s)"
echo ""
echo "  open '$FINAL_DIR'"
sep
