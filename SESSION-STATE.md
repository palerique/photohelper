# photohelper — Session State

> Living handoff document. Read FIRST at every session start; update LAST at
> every session end. Stale state = blocked progress.
>
> Keep this file SMALL. When a `## Prior session: N` block ages out (older than
> the immediately-prior session), demote it to `docs/session-archive/` per the
> rolling-archive convention. The git log is the full timeline.

**Last session**: 1 (`cli-skeleton-and-ingest` — 2026-05-28) —
implementation complete; session-end Round 1 + 2 done; R1+R2
remediation applied; harness sync from fox/eng-protocol landed.

**Current session**: 1 (R2 REMEDIATION APPLIED — ready for `just ci` +
PR push).

**Goal** (session 1): Land the thinnest end-to-end slice that proves
the workspace architecture — `clap` v4 CLI with all 7 subcommands
(`ingest`, `cull`, `develop`, `export`, `run`, `models`, `camera`),
with `ingest` doing real work. **DONE** at commit `310f753`; R1
remediation at `0f28627`; harness sync at `02d43d1`; R2 review at
`docs/code-reviews/session-01-round2.md`; R2 remediation in flight.

**Action**: complete the R2 remediation commit, run `just ci`,
push `session-01/cli-skeleton-and-ingest`, open PR to `main`, wait for
green CI, merge with merge-commit, render two-block handoff per
`docs/session-handoff-format.md`. Per `docs/quality-assurance.md §
Double-review protocol`: R3 fires only if R2 remediation surfaces
CRITICAL-class regressions — `just ci` green is the gating signal.

**Status**: 63 tests pass after R2 remediation (heartbeat
env-override test replaced 1-for-1 with deterministic R2-T6 version;
no net delta); `just ci` pending re-run; smoke test on user's
`/Users/ph/Pictures/tests` (371 real Canon R8 CR3s) surfaced 3
production bugs (R2-T5/T12/T13) all now remediated.

---

## Component progress

| Component             | Status                                  | Notes                                                                                                         |
|-----------------------|-----------------------------------------|---------------------------------------------------------------------------------------------------------------|
| `photohelper-cli`     | **implemented (session 01)**            | clap v4 + 7 subcommands; `ingest` real; stubs exit 69; heartbeat + summary via eprintln!.                     |
| `photohelper-core`    | **implemented (session 01)**            | model (PhotoId, AbsPath, CameraId, KnownCamera, ExifOrientation, Aspect, ExifMetadata, IngestOutcome, Photo); error (13 variants); catalog_glue. |
| `photohelper-raw`     | scaffolded                              | LibRaw FFI + CR3 decode land in session 02.                                                                   |
| `photohelper-ai`      | scaffolded                              | ort/tract + culling/denoise models land in sessions 03+.                                                      |
| `photohelper-sidecar` | scaffolded                              | XMP read/write (crs:/ph: namespaces) lands when `develop` is wired (~session 04).                             |
| `photohelper-export`  | scaffolded                              | resize + watermark + mozjpeg encode land when `export` is wired (~session 05).                                |
| `photohelper-cameras` | **implemented (session 01)**            | CameraProfile trait + CanonR8 stub + CameraRegistry::for_exif with normalization.                             |
| `photohelper-catalog` | **implemented (session 01)** (8th crate)| Catalog::open with file-lock + WAL + magic-byte + schema-version + wal_checkpoint warn; upsert with BEGIN IMMEDIATE + supersede + poison ROLLBACK; PhotoRow boundary; v1 schema authoritatively documented in `docs/decisions/0001-catalog-schema-v1.md`. |

---

## R1 + R2 remediation summary

Session-end Round 1 (`docs/code-reviews/session-01-round1.md`) surfaced
7 CRITICAL + 5 HIGH + 4 MEDIUM + 3 LOW; R1 remediation commits landed
in `0f28627`. Session-end Round 2 (`docs/code-reviews/session-01-round2.md`)
surfaced 13 CRITICAL + 14 HIGH + 12 MEDIUM + 7 LOW, of which several
were regressions inside R1's own remediation commit. R2 remediation
applied (this commit window).

### R1 closure (from `docs/code-reviews/session-01-round1.md`)

All R1 watch-list items closed via R1 remediation (`0f28627`). See
that commit for details; the R2 review verified each closure.

### R2 closure (highlights from `docs/code-reviews/session-01-round2.md`)

- **R2-T1 Magic-byte TOCTOU** — VERIFIED-AND-CLOSED. The R1.T10
  sub-item 3 framing was based on a misread of line refs: lock IS
  acquired at `catalog.rs:121` (`Ok(()) => break` in the `try_lock`
  loop) BEFORE the magic-byte check at `:151`. No TD needed; in-code
  comment added at `catalog.rs:150` to make the in-lock guarantee
  visible without grepping. Five other agents flagged this as a CRITICAL
  policy violation (ungoverned deferral) — they assumed the deferral
  was real; only Agent 6 (comment-analyzer) verified the code.
- **R2-T2 `IngestOutcome::NoExifFields`** — variant + dead `apply_outcome`
  arm deleted; `#[non_exhaustive]` dropped to make the match exhaustive
  at compile time.
- **R2-T3 `query_row(...).ok()`** — replaced at both sites in
  `catalog.rs::upsert` with explicit `QueryReturnedNoRows`-vs-other
  match arms (was masking real SQLite errors as "row missing").
- **R2-T4 + R2-T6 Heartbeat** — `granularity = min(interval, 100ms)`
  so sub-100ms env overrides actually take effect; test rewritten to
  deterministic 80-CR3 fixture + 1ms interval + `[heartbeat]`
  substring assertion (was `expect(true).toBe(true)` per global
  testing standards).
- **R2-T5 EXIF lying WARN** — `parse_failed` flag gates the
  "succeeded with zero fields" WARN; user's prod trace will no
  longer emit contradictory log pairs.
- **R2-T7 ADR-0001** — vulnerable `time` API surface re-attributed to
  the RFC-2822 value-parsing entry points (was incorrectly named as
  `time::format_description::parse`).
- **R2-T8 + decision doc 0001** — `Catalog::open` init transaction now
  uses `BEGIN IMMEDIATE` matching the decision doc's prose contract.
- **R2-T9 + R2-T20 `ExifOrientation::from_tag`** — rustdoc says
  `InvalidExifOrientationTag`; sole production caller now logs a WARN
  on the discard path instead of silently dropping.
- **R2-T11 `op: "mkdir-p"` → `"lock-file-create"`** — fixed sibling
  misnaming R1.T10 missed.
- **R2-T12 `--strict` fail-open** — strict now fails when
  `no_exif > 0` (was only failing on unknown_camera / anomalous /
  errored). Surfaces DN-006/DN-011: makes strict effectively unusable
  for CR3 in v0.1 — intentional escalation, session-02 LibRaw EXIF
  is the remediation.
- **R2-T13** — DN-011 filed; DN-006 binding trigger upgraded.
- **R2-T17** — TD-003 (heartbeat-join) + DN-011 (T13 MtimeFacts) +
  DN-012 (T15 polish) filed with binding triggers.
- **R2-T16** — DN-008 rewritten: deleted `.with_context()` boundary
  claim removed; row list reconciled.
- **R2-T24** — `eight-agent-review` SKILL.md frontmatter adds
  `AskUserQuestion` to `allowed-tools` (gate was working via harness
  fallback; now declared).
- **R2-T25** — HANDOFF Checkpoint 1 test count corrected (33 → 30 in
  model.rs / 32 across crate).
- **R2-T26** — unused `kamadak-exif` + `tracing` deps removed from
  `photohelper-core`.
- **R2-T27** — `Error::Io` doc-comment op-tag list extended with
  `"file-lock"` + `"lock-file-create"`.

### R2 items deferred to session 02 with binding triggers

- **R2-T18** (regression tests for the 4 R1.T10 WARN paths):
  rolled into DN-008's session-02 row enumeration.
- **R2-T19** (replace 128KB PhotoId test with discriminating fixture):
  see R2 commit; if deferred, captured in DN-008 with session-02 trigger.
- **R2-T15** (`open_with_retry_delay` dead public API): deferred to
  session-02 row-13 cross-process file-lock test per DN-008.
- **R2-T22 / R2-T23** (R1 review count drifts): cosmetic; not blocking.
- All MEDIUM and LOW items per R2 artifact's disposition summary.

**No carry-forward CRITICAL items.** All R2 CRITICALs are either
closed inline above or filed as DN/TD with binding triggers.

---

## Continuation-session bootstrap (verbatim)

Session 01 is still open (paused for context refresh — not yet merged).
The resume path is to stay on the same branch:

```bash
git switch session-01/cli-skeleton-and-ingest && just session-start
```

Then read this file + the latest `HANDOFF_REPORT.md § Checkpoint 2`
(the pause-state checkpoint) and proceed to the **Action when context
restored** above (fire session-end Round 2).

After session 01 merges, the next session's bootstrap is the canonical:

```bash
git switch main && git pull --ff-only origin main && git switch -c session-02/<kebab-slug> && just session-start
```
