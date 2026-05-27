#!/usr/bin/env bash
# verify-state.sh — stack-agnostic repo-state verifier for the eng-protocol.
#
# Confirms the governance scaffolding exists and is non-trivial, so a session
# cannot start (or a push cannot land) against a repo whose discipline files
# have been deleted or emptied. Stack-specific gates (fmt/lint/test) are NOT
# checked here — they run as their own `just` recipes + CI jobs.
#
# Exit 0 + "STATUS: ready"   — scaffolding intact.
# Exit 1 + "STATUS: blocked" — something required is missing/empty.
#
# Wired into: `just session-start`, `just ci`, the pre-push hook, and CI.

set -euo pipefail

fail=0
note() { printf '  %s\n' "$1"; }
err()  { printf '  ✗ %s\n' "$1" >&2; fail=1; }

echo "verify-state: checking eng-protocol scaffolding…"

# --- Required files must exist and exceed a minimum size (not just a stub). ---
# A function + positional calls (no associative arrays) so this runs on the
# stock macOS bash 3.2 as well as Linux bash 5.x.
check_file() {
  local path="$1" min="$2" size
  if [[ ! -f "$path" ]]; then
    err "missing required file: $path"
    return
  fi
  # `wc -c` is portable across macOS/Linux; avoid stat's differing flags.
  size=$(wc -c < "$path" | tr -d '[:space:]')
  if (( size < min )); then
    err "file too small (likely a stub): $path (${size}B < ${min}B)"
  else
    note "✓ $path (${size}B)"
  fi
}

check_file "README.md" 256
check_file "CLAUDE.md" 512
check_file "SESSION-STATE.md" 256
check_file "TECH-DEBT.md" 256
check_file "HANDOFF_REPORT.md" 128
check_file "docs/quality-assurance.md" 1024
check_file "docs/session-handoff-format.md" 512
check_file "docs/discovery-notes.md" 128

# --- Required taxonomy directories must exist. -------------------------------
for dir in docs/plans docs/code-reviews docs/adr docs/decisions docs/bugs docs/analysis docs/retrospectives; do
  if [[ ! -d "$dir" ]]; then
    err "missing required directory: $dir"
  else
    note "✓ $dir/"
  fi
done

# --- Branch hygiene: warn (not fail) if working directly on main. -----------
current_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
if [[ "$current_branch" == "main" || "$current_branch" == "master" ]]; then
  note "⚠ on '$current_branch' — sessions must work on a session-NN/<slug> branch (see CLAUDE.md)"
fi

# --- Template-not-yet-adapted guard: loud, non-fatal reminder. ---------------
if grep -RInq -- "<<FILL FROM stacks" justfile 2>/dev/null; then
  note "⚠ justfile still has <<FILL>> placeholders — adapt the gate recipes (see BOOTSTRAP.md)"
fi
if grep -RIlq -- "<PROJECT_NAME>" CLAUDE.md 2>/dev/null; then
  note "⚠ CLAUDE.md still has <PROJECT_NAME> placeholders — fill them in for this repo"
fi

echo ""
if (( fail == 0 )); then
  echo "STATUS: ready"
  exit 0
else
  echo "STATUS: blocked"
  exit 1
fi
