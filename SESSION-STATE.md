# photohelper — Session State

> Living handoff document. Read FIRST at every session start; update LAST at
> every session end. Stale state = blocked progress.
>
> Keep this file SMALL. When a `## Prior session: N` block ages out (older than
> the immediately-prior session), demote it to `docs/session-archive/` per the
> rolling-archive convention. The git log is the full timeline.

**Last session**: 4 (`ai-culling-pipeline` — 2026-05-29) — **SHIPPED** via PR #7
(`0069dac`). Full AI culling pipeline: NIMA inference, catalog v2, `cull` subcommand,
143 tests. Post-ship: `photohelper-cull.sh` fixed (PHOTOHELPER_MODEL_DIR wired) +
`photohelper-list-catalog.sh` enhanced (`--sort score` + score column in list mode).

**Current session**: 5 (`dedup-mobileclip` — 2026-05-29) — branch `session-05/dedup-mobileclip`.
**PAUSED** for context refresh after plan v1 committed; plan-review pending (agents
hit network errors on first launch attempt).

**Goal**: Session 05 — duplicate-detection pipeline (DN-024 MobileCLIP embeddings +
dup_clusters). Plan at `docs/plans/session-05.md`.

**Action**: Resume plan-review — run `/plan-review` on `docs/plans/session-05.md`.
Plan v1 is committed; fire Round 1 agent suite, remediate, fire Round 2. Do NOT
begin implementation until Round 2 is clean.

**Status (session 05 so far)**: Plan v1 committed (`docs/plans/session-05.md`).
`just ci` GREEN (143 tests — no code changes from main). Plan-review Round 1 NOT
yet fired (network errors on 2026-05-29; retry in fresh context).

**Plan-review history (session 04 — COMPLETE)**:
- R1 → 6 CRITICAL + 13 HIGH + 10 MEDIUM + 3 LOW → plan v2
- R2 → 3 HIGH + 5 MEDIUM + 2 LOW → plan v3 (CLEAN)

**Sub-component review (D2b boundary — COMPLETE)**:
- R1 → 1 CRITICAL + 4 HIGH + 5 MEDIUM + 2 LOW → remediated
- R2 → 2 HIGH + 7 MEDIUM + 3 LOW → remediated → CLEAN

**Session-end review (COMPLETE)**:
- R1 → 1 CRITICAL + 4 HIGH + 9 MEDIUM + 1 LOW → 13 items remediated; just ci GREEN (143 tests)
- R2 → 0 findings; all 13 watch-list items CLOSED; CLEAN

**Plan-review history (session 03 — COMPLETE)**:
- R1 → 10 CRITICAL + 18 HIGH + 10 MEDIUM + 5 LOW → plan v2 (dc95639)
- R2 → 3 CRITICAL + 10 HIGH + 9 MEDIUM + 4 LOW → plan v3 (285675e)
- R3 → 3 CRITICAL + 4 HIGH + 2 MEDIUM + 1 LOW → plan v4 (a9f7152 + fixups)
- R4 → 0 CRITICAL + 0 HIGH + 2 MEDIUM (resolved inline) → CLEAN
All CRITICAL findings across all 4 rounds: 0 hallucinated; discard_rate=0.00.
Plan v4 (final): D6 first-chore → D0 pre-flight (binding on Session::run
receiver: &self=Arc<Nima>, &mut self=thread_local!) → D1a–D1d → D2a–D2c →
D3 → D4 → D5 → D7. Sub-component reviews at D1c + D2b boundaries.

---

## Component progress

| Component             | Status                                  | Notes                                                                                                         |
|-----------------------|-----------------------------------------|---------------------------------------------------------------------------------------------------------------|
| `photohelper-cli`     | **implemented (session 01)**            | clap v4 + 7 subcommands; `ingest` real; stubs exit 69; heartbeat + summary via eprintln!.                     |
| `photohelper-core`    | **implemented (session 01)**            | model (PhotoId, AbsPath, CameraId, KnownCamera, ExifOrientation, Aspect, ExifMetadata, IngestOutcome, Photo); error (13 variants); catalog_glue. |
| `photohelper-raw`     | **implemented (session 02+04)**         | Session 02: LibRaw 0.22.1 FFI, exif::read_cr3, decode::read_raw, full type suite. Session 04 D1e: decode::read_raw_rgb (dcraw_process pipeline + RgbImage output via 6 new C shim accessors + parse_libraw_rgb_image + extract_rgb_image). 4 integration tests (3 Bayer + 1 RGB). |
| `photohelper-ai`      | **implemented (session 04)**            | VerifiedModelBytes + NimaScore + Nima (ONNX Runtime 2.0.0-rc.12, thread_local! per-worker Session, SHA-256 model verification). NIMA model in Git LFS. MODEL_SLUG + MODEL_MANIFEST_NAME constants. |
| `photohelper-sidecar` | scaffolded                              | XMP read/write (crs:/ph: namespaces) lands when `develop` is wired (~session 04).                             |
| `photohelper-export`  | scaffolded                              | resize + watermark + mozjpeg encode land when `export` is wired (~session 05).                                |
| `photohelper-cameras` | **implemented (session 01)**            | CameraProfile trait + CanonR8 stub + CameraRegistry::for_exif with normalization.                             |
| `photohelper-catalog` | **implemented (sessions 01+04)**        | Session 01: Catalog::open, upsert, PhotoRow, v1 schema. Session 04 D2a+D2b: schema v2 (cull_scores table + FK enforcement + SCHEMA_VERSION=2), CullRow (PathBuf, private fields + accessors), InsertScoreOutcome, unsuperseded_unscored_rows (NOT IN SQL filter, ORDER BY ingested_at), insert_cull_score (INSERT OR IGNORE + changes(), range guard, TD-013 labeled). Decision docs 0001 amended + 0002 authored. |

---

## Prior session: 1 — shipped (R1 + R2 remediation summary)

Session 01 (`cli-skeleton-and-ingest`) shipped via PR #1 merge commit
`c120819`. Session-end Round 1 (`docs/code-reviews/session-01-round1.md`)
surfaced 7 CRITICAL + 5 HIGH + 4 MEDIUM + 3 LOW; R1 remediation commits
landed in `0f28627`. Session-end Round 2
(`docs/code-reviews/session-01-round2.md`) surfaced 13 CRITICAL + 14 HIGH
+ 12 MEDIUM + 7 LOW, of which several were regressions inside R1's own
remediation commit. R2 remediation landed at `681a3a2`.

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
  rolled into DN-008's session-02 row enumeration. **Session-02
  plan-review Round 1 (`docs/code-reviews/session-02-plan-round1.md`
  § PR1-T4) flagged that R2-T18 closure as written is 3/4 not 4/4 —
  the heartbeat-death WARN is deferred via "if added"; remediation in
  session 02 plan v2.**
- **R2-T19** (replace 128KB PhotoId test with discriminating fixture):
  **closed inline at R2 remediation `681a3a2`** — the discriminating
  test exists at `crates/photohelper-core/src/model.rs:770`
  (`photoid_derive_window_disjoint_distinguishes_overlap_region_changes`).
  Per session-02 plan-review PR1-T30: the plan v1's claim to close
  R2-T19 again is redundant.
- **R2-T15** (`open_with_retry_delay` dead public API): deferred to
  session-02 row-13 cross-process file-lock test per DN-008.
- **R2-T22 / R2-T23** (R1 review count drifts): cosmetic; not blocking.
- All MEDIUM and LOW items per R2 artifact's disposition summary.

**No carry-forward CRITICAL items.** All R2 CRITICALs are either
closed inline above or filed as DN/TD with binding triggers.

---

## Continuation-session bootstrap (verbatim)

Session 02 is in flight on branch `session-02/libraw-cr3-decode`.
Resume from a fresh context by staying on the branch:

```bash
git switch session-02/libraw-cr3-decode && just session-start
```

Then read this file (re-orientation), the latest
`HANDOFF_REPORT.md` checkpoint, `docs/discovery-notes.md`, the
session-02 plan at `docs/plans/session-02.md`, and the in-flight
plan-review artifact at
`docs/code-reviews/session-02-plan-round1.md`. Proceed to the **Action**
above (complete R1 remediation → fire plan-review Round 2 → begin
implementation).

After session 02 merges, the next session's bootstrap is the canonical:

```bash
git switch main && git pull --ff-only origin main && git switch -c session-03/<kebab-slug> && just session-start
```
