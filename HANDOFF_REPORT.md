# photohelper — Handoff Report

> Accumulated summary for stakeholders / the next contributor. Each session
> appends a checkpoint block rather than rewriting history; the git log of this
> file is the versioned timeline. Demote aged blocks to `docs/session-archive/`
> per the rolling-archive convention to keep this file readable.

---

## Checkpoint 0 — bootstrap (2026-05-27)

**Status**: bootstrap
**Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session)

### What exists
- Engineering protocol adopted from the `maxim/eng-protocol-toolkit/template`
  (stack-agnostic protocol files) layered with a new `stacks/rust.md` adapter.
- Governance files in place: `CLAUDE.md`, `SESSION-STATE.md`,
  `HANDOFF_REPORT.md`, `TECH-DEBT.md`, `docs/quality-assurance.md` (cadence A),
  `docs/session-handoff-format.md`, `docs/discovery-notes.md` (seeded with
  DN-001…DN-005), `justfile`, `.pre-commit-config.yaml`,
  `.github/workflows/ci.yml`, `scripts/verify-state.sh`, `stacks/rust.md`.
- `.claude/skills/` carries the four canonical skills (`session-start`,
  `session-end`, `plan-review`, `eight-agent-review`) copied verbatim from
  the maxim toolkit's `plugins/eng-protocol/skills/` directory.
- Rust workspace scaffolded with 7 member crates (`photohelper-cli`,
  `photohelper-core`, `photohelper-raw`, `photohelper-ai`,
  `photohelper-sidecar`, `photohelper-export`, `photohelper-cameras`); each
  non-binary crate ships a one-line `lib.rs` stub so `cargo test --workspace`
  compiles green.
- Toolchain pinned in `rust-toolchain.toml` (channel `1.85.0`, components
  rustfmt + clippy, minimal profile). Workspace-level lints baseline wired
  (`missing_docs = warn`, `unsafe_code = forbid`, clippy pedantic + the
  `unwrap`/`expect`/`panic`/`indexing` warns).
- Dual MIT/Apache-2.0 license (`LICENSE-MIT`, `LICENSE-APACHE`).
- Remote `origin` points at https://github.com/palerique/photohelper.git
  (public, empty until the bootstrap commit lands).

### What is not yet in place
- No real application code yet — only stubs. The full v0.1 scope (AI culling,
  SCUNet denoise, develop, export, watermark) ships across sessions 01-N per
  the bootstrap plan at
  `/Users/ph/.claude/plans/first-create-a-structure-warm-shell.md`.
- No fixture CR3s committed yet; session 02 introduces a small CC0 RAW pack
  via `git-lfs`.
- No release/distribution wiring (musl static, codesign, Authenticode,
  Homebrew tap, winget) — that's its own session later.

### How to resume
```bash
git switch main && git pull --ff-only origin main
just session-start
cat SESSION-STATE.md
```
