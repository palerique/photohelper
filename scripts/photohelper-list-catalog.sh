#!/usr/bin/env bash
# photohelper-list-catalog.sh — list rows from a photohelper catalog.
#
# Read-only by design — no destructive operations. Pretty-prints
# active rows by default; supports per-camera grouping, raw-count,
# and paths-only modes for piping into other tools.
#
# Usage:
#   scripts/photohelper-list-catalog.sh <ingest-dir>          [list flags]
#   scripts/photohelper-list-catalog.sh --catalog <db-path>   [list flags]
#
# Modes (mutually exclusive; default = list):
#   --list           List rows with key metadata (default).
#   --count          Print the active row count.
#   --by-camera      Aggregate by camera_slug.
#   --paths-only     Print just source_path, one per line.
#
# List-mode flags:
#   --all            Include superseded rows (default: active only).
#   --limit N        Cap rows (default 50; use 0 for no limit).
#   --sort FIELD     One of: capture (default), path, ingested.
#
# Common:
#   --help, -h
#
# Exit codes:
#   0   success
#   1   no catalog found
#   2   refused (target doesn't look like a photohelper catalog)
#   64  CLI misuse
#   65  sqlite3 not installed

set -euo pipefail

print_usage() {
    cat >&2 <<'EOF'
Usage: photohelper-list-catalog.sh <ingest-dir>          [list flags]
       photohelper-list-catalog.sh --catalog <db-path>   [list flags]

Modes (mutually exclusive; default = --list):
  --list           List rows with key metadata.
  --count          Print the active row count.
  --by-camera      Aggregate by camera_slug.
  --paths-only     Print just source_path, one per line.

List-mode flags:
  --all            Include superseded rows (default: active only).
  --limit N        Cap rows shown (default 50; use 0 for no limit).
  --sort FIELD     capture (default) | path | ingested | score

Examples:
  photohelper-list-catalog.sh "$HOME/Pictures/tests"
  photohelper-list-catalog.sh "$HOME/Pictures/tests" --by-camera
  photohelper-list-catalog.sh "$HOME/Pictures/tests" --count
  photohelper-list-catalog.sh "$HOME/Pictures/tests" --paths-only --limit 0
  photohelper-list-catalog.sh "$HOME/Pictures/tests" --limit 0 --sort path
  photohelper-list-catalog.sh "$HOME/Pictures/tests" --sort score --limit 20
EOF
}

if ! command -v sqlite3 >/dev/null 2>&1; then
    echo "error: sqlite3 not found on PATH" >&2
    echo "  install: macOS ships sqlite3; Debian/Ubuntu 'sudo apt install sqlite3'" >&2
    exit 65
fi

# --- arg parsing ---------------------------------------------------------
MODE="list"
INCLUDE_SUPERSEDED=0
LIMIT=50
SORT_BY="capture"
INGEST_DIR=""
CATALOG_FILE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --help|-h)
            print_usage
            exit 0
            ;;
        --list)
            MODE="list"
            shift
            ;;
        --count)
            MODE="count"
            shift
            ;;
        --by-camera)
            MODE="by-camera"
            shift
            ;;
        --paths-only)
            MODE="paths-only"
            shift
            ;;
        --all)
            INCLUDE_SUPERSEDED=1
            shift
            ;;
        --limit)
            if [[ $# -lt 2 ]]; then
                echo "error: --limit requires a value" >&2
                exit 64
            fi
            LIMIT="$2"
            if ! [[ "$LIMIT" =~ ^[0-9]+$ ]]; then
                echo "error: --limit must be a non-negative integer (got '$LIMIT')" >&2
                exit 64
            fi
            shift 2
            ;;
        --sort)
            if [[ $# -lt 2 ]]; then
                echo "error: --sort requires a field name" >&2
                exit 64
            fi
            SORT_BY="$2"
            case "$SORT_BY" in
                capture|path|ingested|score) ;;
                *)
                    echo "error: --sort must be one of capture|path|ingested|score (got '$SORT_BY')" >&2
                    exit 64
                    ;;
            esac
            shift 2
            ;;
        --catalog)
            if [[ $# -lt 2 ]]; then
                echo "error: --catalog requires a path" >&2
                exit 64
            fi
            CATALOG_FILE="$2"
            shift 2
            ;;
        --)
            shift
            ;;
        -*)
            echo "error: unknown flag '$1'" >&2
            print_usage
            exit 64
            ;;
        *)
            if [[ -n "$INGEST_DIR" ]]; then
                echo "error: only one ingest-dir argument is allowed" >&2
                exit 64
            fi
            INGEST_DIR="$1"
            shift
            ;;
    esac
done

if [[ -n "$INGEST_DIR" && -n "$CATALOG_FILE" ]]; then
    echo "error: pass either <ingest-dir> OR --catalog <path>, not both" >&2
    exit 64
fi

if [[ -z "$INGEST_DIR" && -z "$CATALOG_FILE" ]]; then
    echo "error: missing <ingest-dir> or --catalog <path>" >&2
    print_usage
    exit 64
fi

# --- resolve catalog path -----------------------------------------------
if [[ -n "$INGEST_DIR" ]]; then
    CATALOG_FILE="$INGEST_DIR/.photohelper/catalog.db"
fi

if [[ ! -f "$CATALOG_FILE" ]]; then
    echo "no catalog found at $CATALOG_FILE" >&2
    echo "  run 'photohelper ingest <dir>' to create one" >&2
    exit 1
fi

# Schema-sanity check — refuse to query something that isn't a photohelper catalog.
if ! sqlite3 "$CATALOG_FILE" \
        "SELECT name FROM sqlite_master WHERE type='table' AND name='photos';" \
        2>/dev/null | grep -q '^photos$'; then
    echo "refusing to query: $CATALOG_FILE has no 'photos' table" >&2
    echo "  (file exists but doesn't look like a photohelper catalog)" >&2
    exit 2
fi

# --- query construction --------------------------------------------------
WHERE_CLAUSE=""
if [[ $INCLUDE_SUPERSEDED -eq 0 ]]; then
    WHERE_CLAUSE="WHERE superseded_at_unix_seconds IS NULL"
fi

ORDER_BY=""
case "$SORT_BY" in
    capture)  ORDER_BY="ORDER BY p.capture_time_unix_seconds" ;;
    path)     ORDER_BY="ORDER BY p.source_path" ;;
    ingested) ORDER_BY="ORDER BY p.ingested_at_unix_seconds" ;;
    score)    ORDER_BY="ORDER BY cs.aesthetic_score DESC" ;;
esac

LIMIT_CLAUSE=""
if [[ "$LIMIT" -gt 0 ]]; then
    LIMIT_CLAUSE="LIMIT $LIMIT"
fi

# --- execute -------------------------------------------------------------
case "$MODE" in
    count)
        ROWS=$(sqlite3 "$CATALOG_FILE" "SELECT COUNT(*) FROM photos $WHERE_CLAUSE;")
        if [[ $INCLUDE_SUPERSEDED -eq 0 ]]; then
            echo "$ROWS active row(s)"
        else
            echo "$ROWS row(s) (includes superseded)"
        fi
        ;;
    by-camera)
        sqlite3 -header -column "$CATALOG_FILE" "
            SELECT camera_slug,
                   COUNT(*) AS n,
                   MIN(datetime(capture_time_unix_seconds, 'unixepoch')) AS first_capture,
                   MAX(datetime(capture_time_unix_seconds, 'unixepoch')) AS last_capture
              FROM photos
              $WHERE_CLAUSE
             GROUP BY camera_slug
             ORDER BY n DESC;
        "
        ;;
    paths-only)
        sqlite3 "$CATALOG_FILE" "
            SELECT source_path
              FROM photos
              $WHERE_CLAUSE
              $ORDER_BY
              $LIMIT_CLAUSE;
        "
        ;;
    list)
        # Qualify WHERE clause for the JOIN alias.
        WHERE_CLAUSE_P=""
        if [[ $INCLUDE_SUPERSEDED -eq 0 ]]; then
            WHERE_CLAUSE_P="WHERE p.superseded_at_unix_seconds IS NULL"
        fi
        sqlite3 -header -column "$CATALOG_FILE" "
            SELECT p.source_path,
                   p.make,
                   p.model,
                   p.camera_slug,
                   p.width || 'x' || p.height AS dim,
                   datetime(p.capture_time_unix_seconds, 'unixepoch') AS captured_utc,
                   p.file_size,
                   CASE WHEN p.superseded_at_unix_seconds IS NULL
                        THEN 'active'
                        ELSE 'superseded'
                   END AS state,
                   CASE WHEN cs.aesthetic_score IS NOT NULL
                        THEN printf('%.4f', cs.aesthetic_score)
                        ELSE '-'
                   END AS score
              FROM photos p
              LEFT JOIN cull_scores cs
                ON cs.photo_id = p.id AND cs.model_slug = 'nima-aesthetic-v1'
              $WHERE_CLAUSE_P
              $ORDER_BY
              $LIMIT_CLAUSE;
        "
        if [[ "$LIMIT" -gt 0 ]]; then
            TOTAL=$(sqlite3 "$CATALOG_FILE" "SELECT COUNT(*) FROM photos $WHERE_CLAUSE;")
            if [[ "$TOTAL" -gt "$LIMIT" ]]; then
                echo ""
                echo "(showing $LIMIT of $TOTAL rows — re-run with --limit 0 for all)"
            fi
        fi
        ;;
esac
