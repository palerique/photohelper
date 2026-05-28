# ADR-0001 — Bump MSRV from 1.85 to 1.88 to consume RUSTSEC-2026-0009 fix

**Status**: Accepted (session 01, 2026-05-28)
**Decided by**: session 01 implementation (`310f753`) per R1.T4 finding.
**Supersedes**: the original 1.85 pin in plan v5 §Dependencies +
  `rust-toolchain.toml`.

## Context

Plan v5 §Dependencies pinned `time 0.3.47` (released 2026-02-05) to
include the fix for **RUSTSEC-2026-0009** — a stack-exhaustion
denial-of-service in the `time::*::parse` value-parsing entry points
(`Date::parse`, `OffsetDateTime::parse`, `PrimitiveDateTime::parse`,
`Time::parse`, `UtcDateTime::parse`, `UtcOffset::parse`, and
`parsing::Parsed::parse_item`) when fed maliciously crafted
**RFC-2822** input. The original ADR text said
`time::format_description::parse` (which parses format strings, not
values) — R2-T7 corrected the attribution. The vulnerability affected
versions `< 0.3.47`. The fix shipped with a constraint:

```
time@0.3.47 requires rustc 1.88.0
time-core@0.1.8 requires rustc 1.88.0
time-macros@0.2.27 requires rustc 1.88.0
```

`rust-toolchain.toml` originally pinned `1.85.0`. `cargo audit --deny
warnings` (a `CLAUDE.md § Quality gates` requirement) flagged the
advisory under the 1.85 pin. Pinning to `time 0.3.46` was not an option
— that version doesn't exist on crates.io; the immediate-prior version
that runs on 1.85 (`0.3.45`) carries the vulnerability.

## Decision

Bump MSRV from `1.85` to `1.88`. Both `Cargo.toml`'s `rust-version` and
`rust-toolchain.toml`'s `channel` updated to `1.88.0`. `CLAUDE.md`'s
Quality-gates section and `stacks/rust.md`'s toolchain-pin section
swept in the same commit so contributors landing on the repo install
the correct toolchain.

## Consequences

**Positive**:
- `cargo audit --deny warnings` is clean.
- The supply-chain hygiene policy `CLAUDE.md § Quality gates` requires
  is upheld at session-end without an allowlist.

**Neutral**:
- 1.88 is the current stable as of session-01 (2026-05-28); the gap is
  three releases, not a leap. Future MSRV bumps within the 1.88 → next
  range remain non-ADR if no governance text changes.

**Negative** (acknowledged):
- Any downstream consumer pinned to `< 1.88` cannot build photohelper.
  v0.1 has no downstream consumers yet; flag again when v0.1 ships.
- New contributors may need to `rustup install 1.88.0` if their
  global default is older. `rust-toolchain.toml` auto-installs on
  first `cargo` invocation in the repo, so this is one-time noise.

## Alternatives considered

- **Pin `time` to `0.3.45` + add `RUSTSEC-2026-0009` to a
  cargo-audit allowlist**: rejected — direct violation of
  `CLAUDE.md § No Acceptable Trade-offs Policy`. A CVE is exactly the
  kind of thing the policy says shouldn't be deferred.
- **Pin `time` to `0.3.46`**: not viable — the version doesn't exist.
- **Drop the `time` dependency entirely**: would require rewriting
  EXIF datetime parsing in `commands/ingest.rs::parse_exif_datetime`
  by hand. Possible but disproportionate; the dep is well-maintained
  and the MSRV bump is a one-line change to two files.

## Trigger to revisit

If a credible photohelper consumer materializes whose toolchain floor
is `< 1.88`, revisit by (a) waiting for the next stable Rust + RUSTSEC
re-evaluation cycle to see if a 1.85-compatible fix appears, or
(b) accepting the consumer-pinning trade-off.
