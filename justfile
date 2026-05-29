# photohelper — command recipes.
#
# Keep this file small. The recipes here mirror what CI runs; drift between
# local recipes and CI is a recurring cause of "works on my machine" failures.
#
# Stack: Rust 2024 (workspace at crates/*). Recipe bodies are sourced from
# stacks/rust.md so that the recipe NAMES stay stable across stacks and the
# session protocol stays stack-independent.

# `-euo pipefail` aborts a recipe body on the first failure, undefined
# variable, or broken pipe. `-c` is required because just passes the recipe
# body as a single argument to the shell. This also closes the
# pipe-exit-code-masking class (`cmd 2>&1 | tail` masking a nonzero exit).
set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load := false

# Ensure cargo subcommands (cargo-fmt, cargo-clippy, etc.) are on PATH.
# When rustup is installed via Homebrew the toolchain bin isn't symlinked into
# /opt/homebrew/bin, so `just` recipes that invoke `cargo fmt` fail with
# "no such command: fmt" unless we prepend the active toolchain's bin directory.
export PATH := `rustup which cargo 2>/dev/null | xargs -I{} dirname {} 2>/dev/null || echo ""` + ":" + env('PATH', '')

# Default recipe — lists the others so bare `just` shows help.
default:
    @just --list --unsorted

# --- Quality gates (Rust — bodies sourced from stacks/rust.md) -------------

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --all-targets --all-features --workspace -- -D warnings

test:
    cargo test --all-features --workspace --no-fail-fast

audit:
    cargo audit --deny warnings

build:
    cargo build --release --all-features --workspace

# --- Session protocol helpers ---------------------------------------------

# Session-start: runs the state verifier and prints the required-reading list.
session-start:
    @echo "=== photohelper — Session Start ==="
    @echo ""
    @./scripts/verify-state.sh
    @echo ""
    @echo "Required reading (canonical list: docs/quality-assurance.md § Session-start protocol):"
    @echo "  1. SESSION-STATE.md"
    @echo "  2. Latest docs/code-reviews/session-*-round2.md (unresolved Round-2 items)"
    @echo "  3. HANDOFF_REPORT.md"
    @echo "  4. docs/discovery-notes.md"
    @echo "  5. docs/quality-assurance.md (the review protocol)"

# Session-end: prints the session-end checklist + runs `just ci`.
session-end:
    @echo "=== photohelper — Session End ==="
    @echo ""
    @echo "Before committing, confirm:"
    @echo "  [ ] Round 1 + Round 2 reviews complete (per the chosen cadence)"
    @echo "  [ ] docs/code-reviews/session-NN-round{1,2}.md written"
    @echo "  [ ] SESSION-STATE.md updated (Next action + Status)"
    @echo "  [ ] docs/discovery-notes.md checkpointed (if new findings)"
    @echo "  [ ] HANDOFF_REPORT.md updated"
    @echo "  [ ] just ci passes"
    @echo ""
    @just ci

# Print the git-log timeline for the two audit-trail files.
session-trail:
    @echo "=== HANDOFF_REPORT.md trail ==="
    @git log --oneline -- HANDOFF_REPORT.md || echo "(not yet committed)"
    @echo ""
    @echo "=== docs/discovery-notes.md trail ==="
    @git log --oneline -- docs/discovery-notes.md || echo "(not yet committed)"

# Create the session-end review-artifact skeletons for session NN.
review-skeleton session:
    @mkdir -p docs/code-reviews
    @if [ ! -f docs/code-reviews/session-{{session}}-round1.md ]; then \
        printf '# Session %s — Round 1 Review\n\n_(populate with findings, grouped by theme.)_\n' "{{session}}" > docs/code-reviews/session-{{session}}-round1.md; \
    fi
    @if [ ! -f docs/code-reviews/session-{{session}}-round2.md ]; then \
        printf '# Session %s — Round 2 Review\n\n_(populate with Round-2 findings; expect regressions introduced by Round-1 remediation.)_\n' "{{session}}" > docs/code-reviews/session-{{session}}-round2.md; \
    fi
    @echo "Skeleton created for session {{session}} under docs/code-reviews/."

# Author the per-round plan-review skeleton (one section per agent).
plan-review-skeleton SESSION ROUND:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p docs/code-reviews
    ARTIFACT="docs/code-reviews/session-{{SESSION}}-plan-round{{ROUND}}.md"
    if [ -f "$ARTIFACT" ]; then echo "$ARTIFACT exists — refusing to overwrite"; exit 1; fi
    {
        echo "# session-{{SESSION}} plan-review round {{ROUND}}"
        echo ""
        echo "> Per docs/quality-assurance.md § Plan-review protocol."
        echo ""
        for AGENT in gp arch rev type sfh com test simp; do
            echo "## $AGENT findings"; echo ""; echo "<!-- Agent $AGENT findings live here. -->"; echo ""
        done
    } > "$ARTIFACT"
    echo "created $ARTIFACT"

# --- Hook + tooling installation ------------------------------------------

# Install the repo-local git hooks. Required once per clone.
# Runner: prek (https://github.com/j178/prek), a drop-in pre-commit replacement.
install-hooks:
    prek install
    @echo "hooks installed — see .pre-commit-config.yaml for the full set"

# Run every pre-commit hook against every tracked file.
hooks-run-all:
    prek run --all-files

# --- Full CI parity -------------------------------------------------------
#
# `ci` runs exactly what .github/workflows/ci.yml runs, in the same order, so
# `just ci` passing locally is equivalent to CI passing. Keep this list in
# sync with the workflow file.
ci: fmt-check lint test audit unsafe-isolation sanitize-check test-helpers-dev-only verify-model-sha256
    @./scripts/verify-state.sh
    @prek run --all-files
    @prek run --all-files --hook-stage pre-push

# D1d: verify NIMA ONNX model SHA-256 matches manifest.toml (LFS pointer detection included).
verify-model-sha256:
    @./scripts/verify-model-sha256.sh

# D5c E2E: verify photohelper-test-helpers appears only as a dev-dependency.
# Any non-dev consumer is a policy violation — test helpers must not be linked
# into release artifacts.
test-helpers-dev-only:
    @./scripts/check-test-helpers-dev-only.sh

# Sanitization gate for tests/fixtures/cr3/*.CR3 — every fixture must
# contain only the asserted-survivor EXIF tag set (no GPS / lens serial
# / owner / credits). Allow-list of required + forbidden tag patterns.
sanitize-check:
    @./scripts/sanitize-check.sh

# Wipe the photohelper catalog for a given ingest directory so the next
# `photohelper ingest` run starts from a clean state. Safe by default;
# pass `--yes` after the path to actually delete. Pass `--catalog <db>`
# instead of a directory to clean a custom-located catalog file.
clean-catalog *ARGS:
    @./scripts/photohelper-clean-catalog.sh {{ARGS}}

# List rows from the photohelper catalog (read-only). Default mode
# pretty-prints active rows with key metadata; pass `--count` for just
# the row count, `--by-camera` for an aggregate, or `--paths-only` for
# pipe-friendly output. See `just list-catalog --help` for the full
# flag list.
list-catalog *ARGS:
    @./scripts/photohelper-list-catalog.sh {{ARGS}}

# Run `photohelper dedup` with PHOTOHELPER_MODEL_DIR wired to the CLIP model.
dedup *ARGS:
    @./scripts/photohelper-dedup.sh {{ARGS}}

# Defense-in-depth: `crates/photohelper-raw` is the only crate allowed to
# contain `unsafe` code, and only `ffi.rs` inside it. The crate Cargo.toml
# allows `unsafe_code` for the FFI module; every other source file carries
# `#![forbid(unsafe_code)]`; this `rg` gate is the third layer, catching
# any new file that lands without the file-level forbid attribute.
unsafe-isolation:
    @./scripts/check-unsafe-isolation.sh
