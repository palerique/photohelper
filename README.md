# photohelper

Cross-platform Rust CLI for AI-powered Canon RAW processing: culling, denoise,
sharpen, non-destructive develop via XMP sidecars, and batch JPEG export with
configurable long-edge resize plus orientation-aware watermarks. Lightroom /
Aftershoot / DxO PureRAW-class workflow as a single binary on Linux, macOS,
and Windows.

> **Project status**: bootstrap — engineering protocol scaffolded; no
> application code shipped yet. See `SESSION-STATE.md` for the next session
> and the bootstrap plan for full architecture / roadmap.
>
> This repo follows the **eng-protocol** — a session-based engineering
> discipline (see `CLAUDE.md` and `docs/quality-assurance.md`). Changes land
> in bounded sessions via reviewed PRs to `main`.

## Quickstart (contributors)

```bash
# prerequisites: Rust 1.88.0 (rust-toolchain.toml pins this) + just + prek
#                + system C/C++ toolchain + pkg-config + GNU make
#                (LibRaw 0.22.1 is vendored under crates/photohelper-raw/vendor/
#                 and built via autoconf the first time `cargo build` runs)
#
# on macOS: brew install rustup just prek pkgconf git-lfs  # pkgconf provides pkg-config
#           xcode-select --install                          # if you don't have Xcode CLT yet
#           rustup set profile minimal && rustup install 1.88.0 --component rustfmt --component clippy
#           cargo install cargo-audit --locked
#           git lfs install                                 # one-time; required to pull CR3 fixtures
#
# on Debian/Ubuntu: sudo apt install build-essential pkg-config git-lfs rustup just prek
#                   git lfs install
# on Fedora:        sudo dnf install make gcc-c++ pkgconf-pkg-config git-lfs rustup just prek
#                   git lfs install

just install-hooks      # one-time: install pre-commit + pre-push hooks
just build              # cargo build --release --all-features --workspace
just test               # cargo test --all-features --workspace --no-fail-fast
just ci                 # everything CI runs (fmt-check, lint, test, audit, verify-state)
```

### Reset a catalog

To wipe the catalog (so the next ingest starts from a clean slate):

```bash
# Dry-run — shows what would be deleted; safe to run.
just clean-catalog "$HOME/Pictures/tests"

# Actually delete.
just clean-catalog "$HOME/Pictures/tests" --yes

# For a custom --catalog path (the file + its -wal / -shm / .lock siblings):
just clean-catalog --catalog /path/to/your.db --yes
```

Original photo files are never touched — only the `.photohelper/` derived
metadata is removed.

### List ingested photos

```bash
just list-catalog "$HOME/Pictures/tests"               # pretty table, first 50 rows
just list-catalog "$HOME/Pictures/tests" --count       # just the row count
just list-catalog "$HOME/Pictures/tests" --by-camera   # aggregate
just list-catalog "$HOME/Pictures/tests" --paths-only  # pipe-friendly
just list-catalog "$HOME/Pictures/tests" --limit 0 --sort path
```

Read-only against the SQLite catalog at `<ingest-dir>/.photohelper/catalog.db`.
Pass `--catalog <db-path>` instead of a directory for a custom location.
Run `just list-catalog --help` for the full flag list.

## Roadmap (per the bootstrap plan)

- **v0.1 (AI-first MVP)** — Canon R8 (CR3) ingest, AI culling (NIMA/ARNIQA
  quality + SCRFD/MediaPipe face & eye-state + MobileCLIP dup grouping → auto
  1-5 star rating), classical develop (demosaic, WB, exposure, tone curve),
  AI RGB denoise (model TBD pending session-04 plan-review), JPEG export
  with long-edge resize + watermarks, XMP
  sidecars (Lightroom-compatible `crs:` + private `ph:`).
- **v0.5** — Canon R5 / R6 II profiles, semantic scene classification, AI
  sharpen (Real-ESRGAN ×2-then-downsample), DirectML/CUDA acceleration,
  per-camera noise calibration command.
- **v1.0** — per-camera Bayer-domain denoise (PMRID/ELD fine-tuned per body)
  with community calibration — the differentiating moat versus DxO PureRAW
  and Aftershoot.

## Development

The engineering process here is not optional — it is how changes are made:

1. **Start a session** — branch `session-NN/<slug>` off `main`; `just session-start`.
2. **Plan, then review** — write `docs/plans/session-NN.md`; review it
   (Round 1 → Round 2) before any code.
3. **Implement, then review** — code to the plan; review it (Round 1 → Round 2).
4. **Ship** — `just session-end`, open a PR to `main`, merge on green CI.

Full protocol: `docs/quality-assurance.md`. Tool-specific rules: `CLAUDE.md`.
The concrete gate commands live in your stack module, `stacks/rust.md`.

## Quality gates

| Gate | Command |
|------|---------|
| Format | `just fmt-check` |
| Lint (zero warnings) | `just lint` |
| Test | `just test` |
| Dependency audit | `just audit` |
| Full local CI parity | `just ci` |

## Layout

| Path | What |
|------|------|
| `CLAUDE.md` | session protocol + quality gates + No-Acceptable-Trade-offs policy |
| `SESSION-STATE.md` | living session-to-session handoff (read first each session) |
| `TECH-DEBT.md` | tech-debt ledger (every deferral carries a binding trigger) |
| `HANDOFF_REPORT.md` | accumulated stakeholder handoff |
| `docs/quality-assurance.md` | the review protocol (8-agent suite, double-review) |
| `docs/plans/`, `docs/code-reviews/`, `docs/adr/`, … | per-session + decision artifacts |
| `stacks/rust.md` | the concrete quality-gate commands for the Rust stack |
| `crates/photohelper-*` | Rust workspace member crates |

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at
your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
