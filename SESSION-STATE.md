# photohelper — Session State

> Living handoff document. Read FIRST at every session start; update LAST at
> every session end. Stale state = blocked progress.
>
> Keep this file SMALL. When a `## Prior session: N` block ages out (older than
> the immediately-prior session), demote it to `docs/session-archive/` per the
> rolling-archive convention. The git log is the full timeline.

**Last session**: 0 (bootstrap — 2026-05-27) — engineering protocol adopted from
the maxim `eng-protocol-toolkit/template`; Rust workspace scaffolded; CI
wired; not yet shipped to anyone.

**Current session**: 1 (`cli-skeleton-and-ingest`) on branch
`session-01/cli-skeleton-and-ingest` (not yet created).

**Goal**: Land the thinnest end-to-end slice that proves the workspace
architecture — `clap` v4 CLI with all subcommands stubbed (`ingest`, `cull`,
`develop`, `export`, `run`, `models`, `camera`), with `ingest` doing real
work: walk a directory recursively, filter RAW extensions, read EXIF, compute
content-derived `PhotoId`, write catalog rows to a SQLite database at
`<root>/.photohelper/catalog.db`.

**Action**: branch `session-01/cli-skeleton-and-ingest`; `just session-start`;
author `docs/plans/session-01.md` (deliverables, scope, tests, checkpoints);
run `/eng-protocol:plan-review` (Round 1 → remediate → Round 2 → remediate)
before any code is written.

**Status**: bootstrap complete; no application code yet (only one-line stubs
in each crate so `cargo test --workspace` compiles green).

**Next action**: see "Action" above — author the session-01 plan and submit
to plan-review.

---

## Component progress

| Component             | Status        | Notes                                                                                                            |
|-----------------------|---------------|------------------------------------------------------------------------------------------------------------------|
| `photohelper-cli`     | scaffolded    | Prints version stub; clap subcommands land in session 01.                                                        |
| `photohelper-core`    | scaffolded    | Empty `version()` only; `Photo` / `PhotoId` / `Pipeline` trait land in session 01.                               |
| `photohelper-raw`     | scaffolded    | Empty; LibRaw FFI + CR3 decode land in session 02.                                                               |
| `photohelper-ai`      | scaffolded    | Empty; `ort` integration + culling/denoise models land in sessions 03+ per the AI roadmap in the bootstrap plan. |
| `photohelper-sidecar` | scaffolded    | Empty; XMP read/write (crs:/ph: namespaces) lands when `develop` is wired (~session 04).                         |
| `photohelper-export`  | scaffolded    | Empty; resize + watermark + mozjpeg encode land when `export` is wired (~session 05).                            |
| `photohelper-cameras` | scaffolded    | Empty; `CameraProfile` trait + `CanonR8` stub land in session 01 (EXIF make/model match only).                   |

---

## Open Round-2 items

_(none — bootstrap)_

---

## Continuation-session bootstrap (verbatim)

```bash
git switch main && git pull --ff-only origin main && git switch -c session-01/cli-skeleton-and-ingest && just session-start
```
