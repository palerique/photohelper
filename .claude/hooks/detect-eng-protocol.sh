#!/usr/bin/env bash
# eng-protocol — SessionStart nudge (bash port of fox's detect-eng-protocol.mjs).
#
# Defensive + non-blocking: emits a one-line reminder ONLY when the project
# looks like an eng-protocol repo (SESSION-STATE.md + docs/quality-assurance.md
# both present). Silent and exit 0 everywhere else, so it's safe to keep the
# hook installed across all projects/worktrees.
#
# Why bash (not node like fox)? Photohelper is a Rust project — Node isn't a
# guaranteed dep. Bash is universal on macOS + Linux (Windows hosts the hook
# under WSL or Git-Bash, both of which ship bash).

set -u   # detect typos; do NOT set -e because partial failures must not block

dir="${CLAUDE_PROJECT_DIR:-$PWD}"
session_state="${dir}/SESSION-STATE.md"
qa="${dir}/docs/quality-assurance.md"

if [[ -f "$session_state" && -f "$qa" ]]; then
    printf 'eng-protocol repo detected — run /session-start to begin a session (reads SESSION-STATE.md, the latest Round-2 review, HANDOFF_REPORT.md, and docs/discovery-notes.md).\n'

    # Worktree list (best-effort; failure is non-fatal).
    if worktrees=$(git -C "$dir" worktree list 2>/dev/null); then
        printf 'Active worktrees:\n'
        printf '%s\n' "$worktrees"
    else
        # Breadcrumb instead of silent swallow.
        printf 'eng-protocol hook degraded (worktree list unavailable)\n' >&2
    fi

    # Branch warning (best-effort).
    if branch=$(git -C "$dir" rev-parse --abbrev-ref HEAD 2>/dev/null); then
        if [[ "$branch" == "main" || "$branch" == "master" ]]; then
            printf "⚠ On '%s' — sessions must work on a session-NN/<kebab-slug> branch.\n" "$branch"
        fi
    else
        printf 'eng-protocol hook degraded (branch read unavailable)\n' >&2
    fi

elif [[ -f "$session_state" || -f "$qa" ]]; then
    # Partial-state nudge — one of the two anchor files is present but not
    # both. Loud (stderr) but non-blocking.
    if [[ -f "$session_state" ]]; then
        missing="docs/quality-assurance.md"
    else
        missing="SESSION-STATE.md"
    fi
    printf '⚠ eng-protocol partial scaffolding detected — %s is missing while the other anchor file is present. Either restore the missing file or remove the other one to clear the warning.\n' "$missing" >&2
fi

exit 0
