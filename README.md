# photohelper

Cross-platform Rust CLI for AI-powered Canon RAW processing: culling, denoise,
sharpen, non-destructive develop via XMP sidecars, and batch JPEG export with
configurable long-edge resize plus orientation-aware watermarks. Lightroom /
Aftershoot / DxO PureRAW-class workflow as a single binary on Linux, macOS,
and Windows.

> **Project status**: v0.1.1 shipped — `ingest`, `cull` (NIMA aesthetic),
> `dedup` (CLIP grouping), `develop` (XMP metadata), `export` (resize + watermark),
> `watermark`, and `rename` are fully implemented on **macOS arm64, Linux x86_64,
> and Windows x86_64** (all binaries are self-contained, no external runtimes required).
>
> To synchronize developed metadata with Adobe Lightroom Classic, see the
> [Lightroom Classic Synchronization Guide](docs/user-guide/lightroom-sync.md).

---

## Installation

Download the latest release archive for your platform from the
[Releases page](https://github.com/palerique/photohelper/releases).

### macOS (Apple Silicon — arm64)

```bash
tar xzf photohelper-VERSION-aarch64-apple-darwin.tar.gz
cd photohelper-VERSION-aarch64-apple-darwin

# One-time: remove quarantine flag (v0.1: unsigned binary)
xattr -dr com.apple.quarantine photohelper

# Install binary
sudo cp photohelper /usr/local/bin/

# Install ONNX models (required for cull + dedup only)
mkdir -p ~/photohelper/models
cp models/* ~/photohelper/models/
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.zshrc
source ~/.zshrc

photohelper --version
```

> ORT (ONNX Runtime) is statically linked on macOS — the binary is fully
> self-contained with no external ORT shared library. Inference uses the CPU
> execution provider. (CoreML.framework appears as a transitive link dependency
> of the static archive but is not the active inference backend.)

### macOS (Intel — x86_64)

No pre-built Intel binary is provided. Intel Macs **cannot** run the arm64
binary — Rosetta 2 only goes the other direction (Intel→Apple Silicon), not
arm64→Intel. Intel Mac users must build from source:

```bash
# Install rust + toolchain, then:
cargo build --release --target x86_64-apple-darwin
```

### Linux (x86_64)

```bash
tar xzf photohelper-VERSION-x86_64-unknown-linux-gnu.tar.gz
cd photohelper-VERSION-x86_64-unknown-linux-gnu

# Install binary
sudo cp photohelper /usr/local/bin/

# Install ONNX models (required for cull + dedup only)
mkdir -p ~/photohelper/models
cp models/* ~/photohelper/models/
echo 'export PHOTOHELPER_MODEL_DIR="$HOME/photohelper/models"' >> ~/.bashrc
source ~/.bashrc

photohelper --version
```

> ORT is statically linked on Linux — the binary is self-contained with no
> shared library dependencies beyond the standard glibc.

### Windows (x86_64)

```powershell
# 1. Unzip (Windows 10+ Explorer → "Extract All", or PowerShell)
Expand-Archive photohelper-VERSION-x86_64-pc-windows-msvc.zip -DestinationPath .
cd photohelper-VERSION-x86_64-pc-windows-msvc

# 2. Install to a permanent location
New-Item -ItemType Directory -Force "$env:LOCALAPPDATA\photohelper\models"
Copy-Item photohelper.exe "$env:LOCALAPPDATA\photohelper\"
Copy-Item models\* "$env:LOCALAPPDATA\photohelper\models\" -Recurse

# 3. Add to PATH and set model dir (add to $PROFILE for persistence)
$env:PATH += ";$env:LOCALAPPDATA\photohelper"
$env:PHOTOHELPER_MODEL_DIR = "$env:LOCALAPPDATA\photohelper\models"

# 4. Verify
photohelper --version
```

To persist across sessions, add these lines to your PowerShell profile
(`notepad $PROFILE`):

```powershell
$env:PATH += ";$env:LOCALAPPDATA\photohelper"
$env:PHOTOHELPER_MODEL_DIR = "$env:LOCALAPPDATA\photohelper\models"
```

> ORT is statically linked on Windows MSVC — **no `onnxruntime.dll` or other
> external DLL is needed**. The static ORT library has a link-time dependency on
> DirectX 12 system DLLs (DXGI, D3D12, DirectML) which are present on Windows 10
> version 1903 (May 2019) and later. Inference uses the CPU execution provider.

---

## Quickstart

> **`PHOTOHELPER_MODEL_DIR`** only needs to be set for `cull` and `dedup`
> (AI features). All other subcommands work without it.

The catalog lives at `<source-dir>/.photohelper/catalog.db` by default.
Pass `--catalog <path>` to use a custom location.

---

### Flow 1 — Full RAW pipeline (ingest → AI cull → dedup → develop → export)

```bash
# Step 1: Walk the directory and catalog every CR3 file.
photohelper ingest ~/Pictures/shoots/session-01

# Step 2: Score every photo with the NIMA aesthetic model (1–5 stars).
# Requires PHOTOHELPER_MODEL_DIR to be set — see Installation above.
photohelper cull \
  --catalog ~/Pictures/shoots/session-01/.photohelper/catalog.db

# Step 3: Group near-duplicate shots using CLIP embeddings.
photohelper dedup \
  --catalog ~/Pictures/shoots/session-01/.photohelper/catalog.db

# Step 4: Write Lightroom-compatible XMP sidecars (ratings, labels, keywords).
photohelper develop \
  --catalog ~/Pictures/shoots/session-01/.photohelper/catalog.db \
  --lr-rating --lr-keywords

# Step 5: Export the best shots (≥3 stars) as watermarked JPEGs —
# marks are composited inside the same encode step (single pass, fastest).
photohelper export \
  --catalog    ~/Pictures/shoots/session-01/.photohelper/catalog.db \
  --output     ~/exports/session-01-final \
  --long-edge  4000 \
  --min-rating 3 \
  --mark1-png  ~/assets/logo-top-right.png \
  --mark2-png  ~/assets/logo-bottom-left.png \
  --with-shadow
```

---

### Flow 2 — Watermark a folder of JPEGs / PNGs directly

Apply a bottom shadow gradient and two corner PNG marks to every image,
export as resized JPEGs. No catalog needed.

```bash
photohelper watermark \
  --source ~/Pictures/delivery \
  --mark1  ~/assets/logo-top.png \
  --mark2  ~/assets/logo-bottom.png \
  --output ~/exports/watermarked \
  --max-long-edge 2048
```

Input can be JPEG, PNG, or CR3. The output directory must not be inside
the source directory.

---

### Flow 3 — AI cull only (score shots without exporting)

```bash
# Requires PHOTOHELPER_MODEL_DIR — see Installation above.
photohelper ingest ~/Pictures/shoots/session-01
photohelper cull \
  --catalog ~/Pictures/shoots/session-01/.photohelper/catalog.db
```

Scores are stored in the catalog and automatically used by `develop`,
`export`, and `rename` — no manual inspection needed. The catalog is a
plain SQLite file at `.photohelper/catalog.db` if you want to query it
directly with any SQLite tool.

NIMA scores are floats in `[1, 10]`; star ratings map to `[1, 5]`.

---

### Flow 4 — Rename RAW files with score + cluster metadata

Copy RAW files (and their `.xmp` sidecars) into a new directory with
filenames that encode the AI score and dedup cluster id.

> **Prerequisites**: `ingest` and `cull` must have been run on the source
> directory first (scores and cluster ids come from the catalog).

```bash
photohelper rename \
  --source ~/Pictures/shoots/session-01 \
  --output ~/exports/renamed
```

Output pattern: `Cluster-{X}_Cull-{Y}-OriginalFilename.ext`
(e.g. `Cluster-007_Cull-07.85-IMG_1234.CR3`).
Sidecars are copied verbatim alongside each RAW.

---

### Flow 5 — Export without watermarks (plain resize)

```bash
photohelper export \
  --catalog    ~/Pictures/shoots/session-01/.photohelper/catalog.db \
  --output     ~/exports/plain \
  --long-edge  2048 \
  --min-rating 0        # export everything regardless of score
```

---

### Reset a catalog

The catalog lives in `<source-dir>/.photohelper/`. To start fresh:

```bash
# Preview what would be deleted (never touches original photos)
rm -rI ~/Pictures/shoots/session-01/.photohelper/

# Or just delete it — photo files are always untouched
rm -rf ~/Pictures/shoots/session-01/.photohelper/
```

---

## Subcommand reference

| Subcommand | Status | Description |
|---|---|---|
| `ingest` | **Shipped** | CR3 via LibRaw 0.22.1, BLAKE3 content IDs, SQLite catalog |
| `cull` | **Shipped** | NIMA aesthetic culling (1–5 star ratings) |
| `dedup` | **Shipped** | MobileCLIP embeddings + duplicate clustering |
| `develop` | **Shipped** | Lightroom-compatible XMP sidecars (ratings, labels, keywords) |
| `export` | **Shipped** | Batch JPEG: long-edge resize, watermarks, MozJPEG encoding |
| `watermark` | **Shipped** | Shadow + dual corner marks on JPEG/PNG/CR3 → JPEG batch |
| `rename` | **Shipped** | Copy RAW+XMP into `Cluster-X_Cull-Y-Name.ext` prefixes |
| `run` | Planned | Orchestrate ingest → cull → develop → export |
| `models` | Planned | Manage AI model bundles |
| `camera` | Planned | Inspect camera profiles |

---

## Build from source

```bash
# Prerequisites: Rust 1.88.0 + just + prek + system C/C++ toolchain
#
# macOS:
#   brew install rustup just prek pkgconf git-lfs
#   xcode-select --install
#   rustup set profile minimal && rustup install 1.88.0 --component rustfmt clippy
#   cargo install cargo-audit --locked && git lfs install
#
# Linux (Debian/Ubuntu):
#   sudo apt install build-essential pkg-config git-lfs
#   # install rustup, just, prek separately; git lfs install
#
# Windows:
#   Install vcpkg and run: vcpkg install libraw:x64-windows-static-md
#   Set VCPKG_ROOT to your vcpkg root

just install-hooks   # one-time: install pre-commit / pre-push hooks
just build           # cargo build --release
just test            # cargo test --all-features --workspace
just ci              # full local CI parity (fmt, lint, test, audit)
```

---

## Roadmap

- **v0.1.1** — All core subcommands shipped across macOS arm64, Linux x86_64, and Windows x86_64 ✓
- **v0.5** — Canon R5/R6 II profiles, semantic scene classification, AI sharpen, DirectML/CUDA acceleration
- **v1.0** — Per-camera Bayer-domain denoise (PMRID/ELD fine-tuned per body), community calibration

---

## Engineering process

Changes land via bounded, reviewed sessions:

1. **Branch** — `session-NN/<slug>` off `main`; `just session-start`
2. **Plan, review** — `docs/plans/session-NN.md`; plan-review (R1 → R2) before code
3. **Implement, review** — implementation-review (R1 → R2)
4. **Ship** — `just session-end`, PR to `main`, merge on green CI

Full protocol: `docs/quality-assurance.md`. Rules: `CLAUDE.md`.

## Quality gates

| Gate | Command |
|---|---|
| Format | `just fmt-check` |
| Lint (zero warnings) | `just lint` |
| Test | `just test` |
| Dependency audit | `just audit` |
| Full local CI parity | `just ci` |

## Layout

| Path | What |
|---|---|
| `CLAUDE.md` | Session protocol + quality gates + No-Acceptable-Trade-offs policy |
| `SESSION-STATE.md` | Living session-to-session handoff (read first each session) |
| `TECH-DEBT.md` | Tech-debt ledger (every deferral carries a binding trigger) |
| `HANDOFF_REPORT.md` | Accumulated stakeholder handoff |
| `docs/quality-assurance.md` | Review protocol (8-agent suite, double-review) |
| `docs/plans/`, `docs/code-reviews/` | Per-session plan + review artifacts |
| `crates/photohelper-*` | Rust workspace member crates |

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at
your option.
