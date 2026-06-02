# photohelper

Cross-platform Rust CLI for AI-powered Canon RAW processing: culling, denoise,
sharpen, non-destructive develop via XMP sidecars, and batch JPEG export with
configurable long-edge resize plus orientation-aware watermarks. Lightroom /
Aftershoot / DxO PureRAW-class workflow as a single binary on Linux, macOS,
and Windows.

> **Project status**: v0.1 in progress — `ingest`, `cull` (NIMA-based aesthetic), `dedup` (CLIP grouping), `develop` (XMP-based metadata development), and `export` (batch resizing & watermark exports) are fully implemented and shipped.
>
> To learn how to synchronize your developed metadata with Adobe Lightroom Classic, please see the [Lightroom Classic Synchronization Guide](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/user-guide/lightroom-sync.md).
>
> This repo follows the **eng-protocol** — a session-based engineering
> discipline (see `CLAUDE.md` and `docs/quality-assurance.md`). Changes land
> in bounded sessions via reviewed PRs to `main`.

## Installation (pre-built binaries)

Download the latest release archive for your platform from the
[Releases page](https://github.com/palerique/photohelper/releases).

### macOS (Apple Silicon — arm64)

```bash
tar xzf photohelper-VERSION-aarch64-apple-darwin.tar.gz
cd photohelper-VERSION-aarch64-apple-darwin
xattr -dr com.apple.quarantine photohelper  # bypass Gatekeeper (v0.1: unsigned)
sudo cp photohelper /usr/local/bin/
sudo cp libonnxruntime.dylib /usr/local/lib/
mkdir -p ~/photohelper/models && cp models/* ~/photohelper/models/
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.zshrc && source ~/.zshrc
photohelper --help
```

### macOS (Intel — x86_64)

Same steps as above but use `photohelper-VERSION-x86_64-apple-darwin.tar.gz`.

### Linux (x86_64)

```bash
tar xzf photohelper-VERSION-x86_64-unknown-linux-gnu.tar.gz
cd photohelper-VERSION-x86_64-unknown-linux-gnu
sudo cp photohelper /usr/local/bin/
sudo cp libonnxruntime.so* /usr/local/lib/ && sudo ldconfig
mkdir -p ~/photohelper/models && cp models/* ~/photohelper/models/
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.bashrc && source ~/.bashrc
photohelper --help
```

### Windows

Windows builds are planned for v0.2. In the meantime, Windows users can build
from source using WSL2 (Ubuntu) or wait for v0.2.

> **Note on models**: `PHOTOHELPER_MODEL_DIR` only needs to be set if you use
> the `cull` or `dedup` subcommands (AI features). All other subcommands work
> without it.

---

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

### Apply shadow + dual corner marks (`watermark`)

Apply a bottom shadow gradient and two corner image marks (PNG badges) to every
JPEG/PNG/CR3 in a directory, exporting high-quality JPEGs to `--output`:

```bash
just watermark \
  --source ~/Pictures/shoots/session-01 \
  --mark1 ~/assets/logo-top.png \
  --mark2 ~/assets/logo-bottom.png \
  --output ~/exports/watermarked \
  --max-long-edge 2048
```

Or directly:

```bash
photohelper watermark \
  --source <SRC_DIR> --mark1 <PNG> --mark2 <PNG> \
  --output <OUT_DIR> [--max-long-edge N] [--force] [--strict]
```

The output directory must not be inside the source directory. Marks must be PNG
files. Non-CR3 RAW formats require `--allow-untested-raw`.

### Rename with catalog metadata (`rename`)

Copy RAW files (and their `.xmp` sidecars) into `--output` under prefixed filenames
derived from the catalog's NIMA score and dedup cluster id:

```bash
just rename \
  --source ~/Pictures/shoots/session-01 \
  --output ~/exports/renamed
```

Or directly:

```bash
photohelper rename \
  --source <SRC_DIR> --output <OUT_DIR> [--force] [--strict]
```

Output filenames follow the pattern `Cluster-{X}_Cull-{Y}-OriginalFilename.ext`
(e.g. `Cluster-007_Cull-07.85-IMG_1234.CR3`). Rows without a score use `Cull-NONE`;
rows without a cluster use `Cluster-NONE`. XMP sidecars are copied verbatim alongside
each RAW file.

### Avoiding the two-shell PATH drift footgun

If you use Claude Code in one terminal and a separate shell in another, remember
to `git pull --ff-only origin main` in your own shell after every PR merge.
Scripts and binaries added in a session (e.g. `scripts/photohelper-clean-catalog.sh`)
live on `main` only after the PR merges, so a shell that hasn't pulled yet will
get `zsh: no such file or directory` when trying to run them.

## Roadmap

### Shipped subcommands

| Subcommand | Status | Notes |
|---|---|---|
| `ingest` | **Shipped** | CR3 via LibRaw 0.22.1, BLAKE3 content IDs, SQLite catalog |
| `cull` | **Shipped** | NIMA aesthetic culling (1–5 star ratings based on NIMA range) |
| `dedup` | **Shipped** | MobileCLIP-based image embeddings & duplicate clustering |
| `develop` | **Shipped** | Write Lightroom-compatible XMP sidecars with ratings, labels, and keywords |
| `export` | **Shipped** | Batch JPEG export with long-edge resize, watermarks, and MozJPEG encoding |
| `watermark` | **Shipped** | Shadow gradient + dual corner marks on JPEG/PNG/CR3 → JPEG batch |
| `rename` | **Shipped** | Copy RAW+XMP into `--output` under `Cluster-X_Cull-Y-Name.ext` prefixes |
| `run` | Planned (session 10+) | Orchestrate ingest → cull → develop → export |
| `models` | Planned (session 10+) | Manage AI model bundles |
| `camera` | Planned (session 10+) | Inspect camera profiles |

### Planned milestones

- **v0.1 (AI-first MVP)** — `ingest` ✓ + AI culling (NIMA aesthetic + ARNIQA
  technical quality + MobileCLIP dup grouping → auto 1–5 star rating) + classical
  develop (demosaic, WB, exposure, tone curve) + AI RGB denoise + JPEG export +
  XMP sidecars (Lightroom-compatible `crs:` + private `ph:`).
- **v0.5** — Canon R5 / R6 II profiles, semantic scene classification, AI
  sharpen, DirectML/CUDA acceleration, per-camera noise calibration.
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
