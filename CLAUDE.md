# CLAUDE.md — photohelper

> Tool-specific guidance for any Claude Code session working in this repo.
> Layers on top of your personal `~/.claude/CLAUDE.md`; supersedes nothing
> there.

## What this repo is

photohelper is a cross-platform Rust CLI that processes Canon RAW photos
(CR3 for Canon R8 first, extensible to other bodies) at high quality and
high throughput. It does AI-based culling (Aftershoot-style), AI- and
classical-pipeline image improvement (Lightroom + DxO PureRAW-style),
non-destructive edits via XMP sidecars (Lightroom-compatible `crs:` namespace
plus a private `ph:` namespace), and batch JPEG export with configurable
long-edge resize plus top-right + bottom-left watermarks (portrait/landscape
aware). It ships as a single binary on Linux/macOS/Windows with no Python
or Node runtime dependency for end users.

## Mandatory session protocol

Every Claude Code session in this repo follows the same four-step pattern. No
exceptions — not even for "small" edits. The discipline is the product.

1. **Session start** — on a fresh checkout of `main`:
   ```bash
   git switch main && git pull --ff-only origin main
   git switch -c session-NN/<kebab-slug>   # e.g. session-02/libraw-cr3-decode
   just session-start                        # reads SESSION-STATE.md etc.
   ```
   Read `SESSION-STATE.md`, the latest `docs/code-reviews/` Round-2 entry,
   `HANDOFF_REPORT.md`, and `docs/discovery-notes.md`. Declare a session goal.
2. **Work on the branch** — author `docs/plans/session-NN.md`, run the
   review on the plan (Round 1 → remediate → Round 2 → remediate), implement
   per the remediated plan, run the review on the code (Round 1 → remediate →
   Round 2 → remediate). Commit per conventional-commits — many small commits
   on the branch are welcome (they preserve the narrative for the PR reviewer).
   See `docs/quality-assurance.md` for the definitive review protocol.
3. **Session end on the branch** — update `SESSION-STATE.md` (status + next
   action), checkpoint `HANDOFF_REPORT.md` + `docs/discovery-notes.md`, run
   `just session-end` (runs `just ci`). The final session-end response MUST end
   with the two-block handoff per `docs/session-handoff-format.md`.
4. **Ship via PR** — push the branch and open a PR targeting `main`:
   ```bash
   git push -u origin session-NN/<kebab-slug>
   gh pr create --base main --head session-NN/<kebab-slug> \
       --title "session NN: <one-line summary>" \
       --body "<body pointing at docs/plans/session-NN.md and the Round-2 review>"
   gh pr checks --watch                    # wait for every CI job to go green
   gh pr merge --merge --delete-branch     # merge-commit preserves the narrative
   ```
   Never commit directly to `main`. Never merge a PR whose CI is red or yellow —
   investigate the failure first.

### Branch-name convention

- `session-NN/<kebab-slug>` — `NN` zero-padded (`01`, `02`, …); the slug names
  the primary component worked on. Examples: `session-01/cli-skeleton-and-ingest`,
  `session-02/libraw-cr3-decode`, `session-06/td-cleanup`.
- Long investigations still branch per-session; cross-session continuity lives
  in `SESSION-STATE.md`, not in a long-lived branch.

### Merge policy

- **Merge commit** (`gh pr merge --merge`), not squash. The conventional-commit
  chain tells the "what happened this session" story; squashing flattens it.
  `git log --oneline --first-parent main` then shows one merge per session.
- `--delete-branch` removes the remote branch after merge.

## Quality gates

The *discipline* is in `docs/quality-assurance.md`; the *commands* are in your
stack module (`stacks/rust.md`) and wired into `justfile` +
`.github/workflows/ci.yml`. The recipe names are stable across stacks:

- **Format** — `just fmt-check` must be clean. (`cargo fmt --all -- --check`)
- **Lint — zero warnings** — `just lint`
  (`cargo clippy --all-targets --all-features --workspace -- -D warnings`).
  A local `#[allow(...)]` requires a one-line justification comment **and** a
  `TECH-DEBT.md` entry with a concrete remediation plan.
- **Test** — `just test`
  (`cargo test --all-features --workspace --no-fail-fast`).
- **Dependency audit** — `just audit` (`cargo audit --deny warnings`).
- **Toolchain pin** — `rust-toolchain.toml` (channel `1.88.0`) is committed;
  don't bump without a `docs/adr/` entry. Most recent bump: 1.85 → 1.88
  per `docs/adr/0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md`
  (session 01 forced by `time 0.3.47`'s CVE fix).
- **Full local CI parity** — `just ci` runs exactly what
  `.github/workflows/ci.yml` runs, in the same order, so green locally ==
  green in CI.

### Rust-specific gates

- **Error handling** — libraries return `Result<T, E>` with a domain-specific
  `thiserror`-derived error enum (no `Box<dyn Error>` across public APIs).
  Binaries use `anyhow::Result` at the `main`/command boundary; convert at
  the boundary, not deeper. Never discard an error with `let _ = …` on a
  production path without a justifying comment.
- **No panics / unchecked failures on production paths** — no `panic!`,
  `unwrap()`, `expect()`, or unchecked indexing. Permitted in tests,
  `build.rs`, and the `main` startup path for unrecoverable startup faults.
  Enforced by clippy lints in workspace `Cargo.toml`
  (`unwrap_used`/`expect_used`/`panic`/`indexing_slicing` = `warn`, escalated
  to error by `cargo clippy -D warnings` in CI).
- **Docstrings on every exported item** — `missing_docs = "warn"` at workspace
  level. Add doctests where they clarify usage.
- **Unsafe** — `unsafe_code = "forbid"` at workspace level. The
  `photohelper-raw` crate overrides per-crate (LibRaw FFI). `unsafe` blocks
  are scoped to a single `ffi` module and carry a `// SAFETY:` comment.

- **Conventional commits** — `feat:`, `fix:`, `docs:`, `refactor:`, `chore:`,
  `test:`, `ci:`. One logical change per commit.

**Review cadence for this repo:** **A** (tier-graduated — solo project; see
`docs/quality-assurance.md § Review cadence`). The full 8-agent suite still
fires at plan-review, sub-component boundaries, and session-end.

## No Acceptable Trade-offs Policy

A **stop-gap fix** addresses the immediate symptom but preserves the underlying
flaw class. Stop-gaps are a legitimate tool when the immediate need (unblocking
a merge, capping a wall-clock budget, surviving a CI run) outweighs the cost of
the fundamental fix in the current session. They are NOT a final answer.

**Mandatory practice**: every stop-gap commit MUST file a TD entry in
`TECH-DEBT.md` that:

1. **Identifies the stop-gap location** — file path + line + commit SHA.
2. **Names the fundamental fix** — concrete implementation outline, not
   "investigate" or "consider rewriting".
3. **Specifies a binding trigger** — either rev-list-anchored (NEXT session
   that touches X; NEXT session-end review × 2 rounds) OR temporally bounded
   (by `2026-MM-DD`) OR event-driven (re-failure on a specific surface).
   "TODO: come back to this" is NOT a trigger.
4. **Estimates LoC + risk** — concrete scope so a future contributor can plan
   the fix without re-investigation.
5. **States the consequence of inaction** — "if unaddressed, X happens".

**Stop-gap commits without companion TDs violate this policy** and are treated
as deferral-without-a-plan (a CRITICAL finding per
`docs/quality-assurance.md § Findings triage`).

**The stop-gap MUST be labeled in-source.** A comment at the stop-gap site
cites the `TD-N` identifier so the next reader sees the obligation without
grepping.

**Why mandatory**: a single stop-gap is fine; an accumulation of stop-gaps
without TDs becomes a flaw-class debt that compounds across sessions. The
TD-and-trigger contract converts each stop-gap from "indefinite acceptance" to
"scheduled remediation".

## Working-directory expectations

Shell sessions start at the repo root. If you find yourself `cd`-ing around,
something is off — stop and re-read `SESSION-STATE.md`.

## Where things live

| Artifact                       | Path                                              | Who writes it                   |
|--------------------------------|---------------------------------------------------|---------------------------------|
| Living session handoff         | `SESSION-STATE.md`                                | Every session (session-end)     |
| Tech-debt ledger               | `TECH-DEBT.md`                                    | Every session that defers a fix |
| Handoff to stakeholders        | `HANDOFF_REPORT.md`                               | Checkpointed every session      |
| Design-gap findings            | `docs/discovery-notes.md`                         | When a gap is surfaced          |
| Per-session plan               | `docs/plans/session-NN.md`                        | First thing in each session     |
| Plan-review artifacts          | `docs/code-reviews/session-NN-plan-round{1,2}.md` | The review *on the plan*        |
| Session-end review artifacts   | `docs/code-reviews/session-NN-round{1,2}.md`      | The review *on the code*        |
| Architectural decisions        | `docs/adr/NNNN-slug.md`                           | When a decision is binding      |
| Smaller decisions              | `docs/decisions/NNNN-slug.md`                     | When worth recording, not ADR-scale |
| Bug investigations             | `docs/bugs/BUG-NNN-slug.md`                       | When triaging a real bug        |
| Deep-dive analyses             | `docs/analysis/ANL-NNN-slug.md`                   | For multi-session investigations |
| Session retrospectives         | `docs/retrospectives/session-NN.md`               | Optional; when learnings justify |
| Quality protocol               | `docs/quality-assurance.md`                       | Stable; revise only via ADR     |
| Session-end handoff format     | `docs/session-handoff-format.md`                  | Stable; canonical handoff shape |
| Stack quality gates            | `stacks/rust.md`                                  | At adoption; revise via ADR     |
