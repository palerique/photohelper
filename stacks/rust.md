# Stack module — Rust (CLI / batch image pipeline)

> Fill the `<<STACK_…>>` tokens in `CLAUDE.md`, the gate recipes in `justfile`,
> the gate jobs in `.github/workflows/ci.yml`, and the hooks in
> `.pre-commit-config.yaml` from the blocks below. These are *defaults* — adjust
> versions and flags as the project evolves, then record the choice in
> `docs/adr/`.

## Toolchain pin — `<<STACK_TOOLCHAIN_PIN>>`

Pin in `rust-toolchain.toml` at the repo root:

```toml
[toolchain]
channel = "1.88.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

(Bumped from `1.85.0` in session 01 per
`docs/adr/0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md` —
`time 0.3.47`'s fix for RUSTSEC-2026-0009 requires 1.88.)

Commit `Cargo.lock` (this is a binary workspace). CI honors `rust-toolchain.toml`
automatically via `dtolnay/rust-toolchain@stable`.

## Tools to install (host machine, one-time)

```bash
brew install rustup just prek                          # or your platform equivalent
rustup set profile minimal
rustup install 1.88.0 --component rustfmt --component clippy
rustup default 1.88.0
cargo install cargo-audit --locked                      # dependency-vuln audit
```

## justfile gate recipe bodies

Paste these over the `<<FILL>>` placeholder bodies in `justfile`. Keep the
recipe NAMES (`fmt`, `fmt-check`, `lint`, `test`, `audit`, `build`) — they are
stable across stacks so the session protocol stays stack-independent.

```just
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
```

> `--workspace` ensures every member crate is exercised. `--no-fail-fast` keeps
> running other crates after the first failure so one session sees every
> failing test, not just the first.

## CI gate job steps

Replace each placeholder gate job's `run:` steps in `.github/workflows/ci.yml`.
Action versions are written with `# <<pin to SHA>>` comments — replace each
tag with a commit SHA per your supply-chain policy before going public.

```yaml
  fmt:
    name: format check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4                       # <<pin to SHA>>
      - uses: dtolnay/rust-toolchain@stable             # <<pin to SHA>>
        with: { components: rustfmt }
      - run: cargo fmt --all -- --check

  lint:
    name: lint (zero warnings)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4                       # <<pin to SHA>>
      - uses: dtolnay/rust-toolchain@stable             # <<pin to SHA>>
        with: { components: clippy }
      - uses: Swatinem/rust-cache@v2                    # <<pin to SHA>>
      - run: cargo clippy --all-targets --all-features --workspace -- -D warnings

  test:
    name: test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4                       # <<pin to SHA>>
      - uses: dtolnay/rust-toolchain@stable             # <<pin to SHA>>
      - uses: Swatinem/rust-cache@v2                    # <<pin to SHA>>
      - run: cargo test --all-features --workspace --no-fail-fast

  audit:
    name: dependency audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4                       # <<pin to SHA>>
      - uses: dtolnay/rust-toolchain@stable             # <<pin to SHA>>
      - uses: Swatinem/rust-cache@v2                    # <<pin to SHA>>
      - run: cargo install cargo-audit --locked
      - run: cargo audit --deny warnings
```

## pre-commit hook tweak

In `.pre-commit-config.yaml`, scope the `fmt-check` hook to Rust files so it
does not run on every commit touching only `*.md` / `*.toml`:

```yaml
      - id: fmt-check
        name: format check
        entry: just fmt-check
        language: system
        files: \.rs$
        pass_filenames: false
```

`lint` and `test` already run as `pre-push` hooks — leave those as-is.

## CLAUDE.md convention tokens

- `<<STACK_ERROR_CONVENTION>>`: libraries return `Result<T, E>` with a
  domain-specific `thiserror`-derived error enum (no `Box<dyn Error>` across
  public APIs). Binaries (`photohelper-cli`) use `anyhow::Result` at the
  `main`/command boundary; convert at the boundary, not deeper. Never discard
  an error with `let _ = …` on a production path without a comment justifying
  why it is safe.
- `<<STACK_PANIC_RULE>>`: no `panic!`, `unwrap()`, `expect()`, or unchecked
  indexing on a production path. Permitted in tests, `build.rs`, and the
  `main` startup path for unrecoverable startup faults (e.g. config absent).
  Enforced by clippy lints in workspace `Cargo.toml [workspace.lints.clippy]`:
  `unwrap_used = "warn"`, `expect_used = "warn"`, `panic = "warn"`,
  `indexing_slicing = "warn"`. Suppressions require an inline justification
  comment plus a `TECH-DEBT.md` entry (see `CLAUDE.md § No Acceptable
  Trade-offs Policy`).
- `<<STACK_DOC_RULE>>`: every public item (`pub fn`, `pub struct`, `pub enum`,
  `pub mod`, `pub trait`, …) carries a doc comment. Enforced by `missing_docs
  = "warn"` in `[workspace.lints.rust]` and `cargo clippy -D warnings` in CI
  (warnings become errors there). Add doctests where they clarify usage.
- `<<STACK_UNSAFE_RULE>>`: `unsafe_code = "forbid"` at workspace level. Crates
  that need FFI (currently only `photohelper-raw` for LibRaw) override per-
  crate; the `unsafe` blocks are scoped to a single `ffi` module and carry a
  `// SAFETY:` comment justifying each invocation.

## Recommended `[workspace.lints]` baseline (already wired in `Cargo.toml`)

```toml
[workspace.lints.rust]
missing_docs = "warn"
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
panic = "warn"
indexing_slicing = "warn"
```

Per-crate overrides go in that crate's `Cargo.toml [lints]` table.

## Cross-platform packaging notes (for distribution sessions, not bootstrap)

The release pipeline targets a single binary per OS with no Python/Node
runtime dependency:

- **Linux**: `cargo zigbuild --target x86_64-unknown-linux-musl` (and
  `aarch64-unknown-linux-musl`) for fully static binaries.
- **macOS**: universal2 (`aarch64-apple-darwin` + `x86_64-apple-darwin` joined
  via `lipo`); `codesign` + `notarytool` for Gatekeeper.
- **Windows**: `x86_64-pc-windows-msvc` with `-C target-feature=+crt-static`;
  Authenticode-sign with an EV certificate.

Heavy AI model files (ONNX) are downloaded on first use via
`photohelper models pull <name>` and cached under `~/.cache/photohelper/models/`,
keeping the base binary under ~80 MB.
