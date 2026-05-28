#!/usr/bin/env bash
# photohelper-clean-catalog.sh — wipe a photohelper catalog directory.
#
# Removes the `.photohelper/` sub-directory (or a `--catalog`-flagged
# sibling file set) so the next `photohelper ingest` run starts from a
# clean catalog. ONLY the catalog state is affected — original photo
# files are untouched.
#
# Safe by default: dry-runs and prints what would be deleted. Pass
# `--yes` (or `-y`) to actually delete.
#
# Usage:
#   scripts/photohelper-clean-catalog.sh <ingest-dir>             # dry-run
#   scripts/photohelper-clean-catalog.sh <ingest-dir> --yes       # delete
#   scripts/photohelper-clean-catalog.sh --catalog <db-path>      # dry-run
#   scripts/photohelper-clean-catalog.sh --catalog <db-path> --yes
#
# Exit codes:
#   0  cleaned (or dry-run with no errors, or nothing to clean — both
#      are the desired "catalog is gone" end-state)
#   2  refused (target doesn't look like a photohelper catalog)
#   64 misuse (bad CLI flags)

set -euo pipefail

print_usage() {
    cat >&2 <<'EOF'
Usage: photohelper-clean-catalog.sh <ingest-dir> [--yes]
       photohelper-clean-catalog.sh --catalog <db-path> [--yes]

Options:
  <ingest-dir>      Directory previously passed to `photohelper ingest`.
                    Removes <ingest-dir>/.photohelper/ on confirmation.
  --catalog <path>  Catalog DB path previously passed to --catalog.
                    Removes <path> + <path>-wal + <path>-shm + <path>.lock.
  --yes, -y         Actually delete (omit for dry-run).
  --help, -h        This message.

Examples:
  photohelper-clean-catalog.sh "$HOME/Pictures/tests"           # dry-run
  photohelper-clean-catalog.sh "$HOME/Pictures/tests" --yes     # do it
  photohelper-clean-catalog.sh --catalog /tmp/my.db --yes
EOF
}

# --- arg parsing ---------------------------------------------------------
CONFIRM=0
INGEST_DIR=""
CATALOG_FILE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --help|-h)
            print_usage
            exit 0
            ;;
        --yes|-y)
            CONFIRM=1
            shift
            ;;
        --catalog)
            if [[ $# -lt 2 ]]; then
                echo "error: --catalog requires a path argument" >&2
                print_usage
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
                echo "error: only one ingest-dir argument is allowed (got '$INGEST_DIR' and '$1')" >&2
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

# --- compute target file set --------------------------------------------
TARGETS=()
if [[ -n "$INGEST_DIR" ]]; then
    CATALOG_DIR="$INGEST_DIR/.photohelper"
    if [[ ! -e "$CATALOG_DIR" ]]; then
        echo "nothing to clean: $CATALOG_DIR does not exist"
        exit 0
    fi
    # Safety check: refuse to clean directories that don't look like a
    # photohelper catalog (must contain catalog.db).
    if [[ ! -f "$CATALOG_DIR/catalog.db" ]]; then
        echo "refusing to clean: $CATALOG_DIR/catalog.db not found" >&2
        echo "  (the directory exists but doesn't look like a photohelper catalog)" >&2
        exit 2
    fi
    TARGETS+=("$CATALOG_DIR")
else
    if [[ ! -f "$CATALOG_FILE" ]]; then
        echo "nothing to clean: $CATALOG_FILE does not exist"
        exit 0
    fi
    # Sanity check on the filename — refuse if it doesn't end in .db
    # (avoids `rm -f /etc/passwd` typos).
    if [[ "$CATALOG_FILE" != *.db ]]; then
        echo "refusing to clean: $CATALOG_FILE does not end in '.db'" >&2
        echo "  (catalog files end in .db; pass the right path or rename)" >&2
        exit 2
    fi
    TARGETS+=("$CATALOG_FILE")
    [[ -e "${CATALOG_FILE}-wal" ]] && TARGETS+=("${CATALOG_FILE}-wal")
    [[ -e "${CATALOG_FILE}-shm" ]] && TARGETS+=("${CATALOG_FILE}-shm")
    [[ -e "${CATALOG_FILE}.lock" ]] && TARGETS+=("${CATALOG_FILE}.lock")
fi

# --- show + execute -----------------------------------------------------
echo "would remove:"
for t in "${TARGETS[@]}"; do
    if [[ -d "$t" ]]; then
        size=$(du -sh "$t" 2>/dev/null | awk '{print $1}')
        printf "  %s/ (dir, %s)\n" "$t" "$size"
    else
        size=$(du -h "$t" 2>/dev/null | awk '{print $1}')
        printf "  %s (file, %s)\n" "$t" "$size"
    fi
done

if [[ $CONFIRM -eq 0 ]]; then
    echo ""
    echo "dry-run: pass --yes to actually delete"
    exit 0
fi

for t in "${TARGETS[@]}"; do
    rm -rf "$t"
done

echo ""
echo "cleaned: ${#TARGETS[@]} target(s) removed"
