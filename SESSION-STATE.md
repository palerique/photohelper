# photohelper — Session State

> Living handoff document. Read FIRST at every session start; update LAST at
> every session end. Stale state = blocked progress.
>
> Keep this file SMALL. When a `## Prior session: N` block ages out (older than
> the immediately-prior session), demote it to `docs/session-archive/` per the
> rolling-archive convention. The git log is the full timeline.

**Last session**: 1 (`cli-skeleton-and-ingest` — 2026-05-28) —
**SHIPPED** via PR #1, merge commit `c120819` on `main`. Full
session-end double-review (R1 + R2) plus remediation landed inside the
branch; harness sync from fox/eng-protocol bundled. Session 01
post-merge two-block handoff was NOT rendered (process-gap; recorded
here for the audit trail).

**Current session**: 2 (`libraw-cr3-decode`, branch
`session-02/libraw-cr3-decode`). Plan v1 committed at `b377aed`;
plan-review Round 1 fired and artifact landed at
`docs/code-reviews/session-02-plan-round1.md` (16 CRITICAL + 17 HIGH
+ 14 MEDIUM + 9 LOW); R1 remediation in flight (plan v2 next).

**Goal** (session 2): land LibRaw FFI for Canon R8 CR3 — EXIF read
(the DN-011 critical-path remediation: kamadak-exif fails 370/370 real
CR3s, making `--strict` effectively unusable until LibRaw lands) AND
RAW pixel decode (the originally-scoped session-02 deliverable).
Rewire `ingest_one` for LibRaw EXIF; flip DN-006 integration tests
to pass on real CR3 fixtures; bundle TD-002 rusqlite 0.32 → 0.40 bump
(voluntary; calendar trigger 2026-08-01); fold in DN-008 row subset
(rows 6, 17, 39, 42, 43, 49) + R2-T18 WARN regressions. See
`docs/plans/session-02.md` for the full contract.

**Action**: **READY TO SHIP.** Every Deliverable 0-7 sub-goal landed
on `session-02/libraw-cr3-decode` across 13 commits today. The session
GOAL (LibRaw FFI for Canon R8 CR3 — EXIF read + RAW pixel decode) is
fully met end-to-end: `photohelper ingest "$HOME/Pictures/tests" --strict`
produces `walked: 371, ingested: 370, unknown-camera: 0, no-exif: 0,
errored: 0, exit 0` against the user's 370-CR3 corpus (was
`walked: 371, ingested: 0, no-exif: 370` pre-session-02). DN-006 +
DN-011 + DN-018 + DN-019 closed; TD-003 + TD-008 closed; TD-002 partial.
Deliverable 6 (test infrastructure) + Deliverable 5's 6-sub-test
verification + Deliverable 4's per-RawExifCause dispatch table /
ExifCompleteness deferred via TD-010 / TD-011 (next, see below) /
inline plan-deviation notes. Next: session-end ship workflow per
`docs/session-handoff-format.md`.

**Status**: `just ci` GREEN end-to-end on apple-silicon. 118 workspace
tests pass; libraw.a builds in ~30s on a clean checkout. End-to-end
acceptance: `photohelper ingest "$HOME/Pictures/tests" --strict` →
`walked: 371, ingested: 370, unknown-camera: 0, no-exif: 0, errored: 0`,
exit code 0. Was `walked: 371, ingested: 0, no-exif: 370` before
session 02. Workspace state clean. 13 commits this session:
- `bb87735` — fix(session-02): close TD-003 (heartbeat join) per DN-019 trigger
- `e6d53fb` — chore: gitignore `.serena/` per-machine MCP state
- `0d4a7f7` — chore(libraw): pre-flight EXIF + CVE-posture audit (Deliverable 0)
- `440388a` — chore(session-02): photohelper-raw lint scaffolding + unsafe-isolation gate (D1a setup)
- `a59ef66` — feat(session-02): Error + RawExifCause + RawDecodeCause enums (D1d)
- `c42ce2f` — feat(session-02): RawExif type + accessors (D1b types slice)
- `8b6b9e8` — feat(session-02): RawImage + Bayer-decode companion types with R2-T6 invariants (D1c)
- `51905be` — feat(libraw): vendor LibRaw 0.22.1 + autoconf build.rs + ADR-0002 LGPL (D2)
- `092383f` — feat(libraw): FFI + RawPath + read_cr3 EXIF extraction (D1a-exif)
- `f8238f4` — feat(libraw): read_raw + parse_libraw_image + RawImage::new (D1a-decode; closes TD-008)
- `7907ca8` — feat(fixtures): Git LFS CC0 R8 CR3 fixtures + sanitize-check + integration tests (D3)
- `203f58d` — refactor(session-02): atomic kamadak-exif removal + RAW_EXTS narrow + ingest LibRaw rewire (D4; closes DN-006/DN-011)
- `2323b6b` — chore(deps): rusqlite 0.32 → 0.34 partial bump (D5; TD-002 partial)
- `63002e5` — chore(session-02): Deliverable 7 polish + Deliverable 6 deferred via TD-010

10 plan-review commits remain on the branch from the prior phase:
- `b377aed` — plan v1
- `354406f` — plan-review Round 1 artifact
- `b64425f` — SESSION-STATE.md drift cleanup
- `5d5dc9a` — R1 remediation cross-doc fixes
- `69b6a5b` — plan v2 (R1 remediation, closes 16 CRITICAL + 17 HIGH)
- `c80acf3` — plan-review Round 2 artifact
- `0e54129` — R2 remediation cross-doc filings (DN-016/017/018 + SCUNet scrub)
- `dc41dee` — plan v3 (R2 remediation, closes 9 CRITICAL + 14 HIGH)
- `37373f4` — plan-review Round 3 artifact
- `dd62166` — R3 remediation plan v3.1 + TD-005/006/007 + SESSION-STATE update

**Plan-review history (3 rounds; diminishing-returns observation)**:
R1 surfaced 16 CRITICAL + 17 HIGH + 14 MEDIUM + 9 LOW; v2 closed
most. R2 surfaced 9 CRITICAL + 14 HIGH + 12 MEDIUM + 6 LOW (mostly
regressions inside R1 remediation); v3 closed most. R3 surfaced 7
CRITICAL + 9 HIGH + 8 MEDIUM + 4 LOW (mostly regressions inside R2
remediation including the R2-T1 phantom-ID anti-pattern reborn at
R3 level — orchestrator self-criticism); v3.1 + TD-005/006/007
addressed inline + via TD-with-binding-trigger respectively.

**R4 NOT fired** — per agent consensus across R3, "R4 not required
if R3 remediation cleanly closes R3 CRITICALs." Targeted R3
remediation landed the audit-trail corrections (R3-T1 phantom IDs;
R3-T2 fabricated LibRaw symbol) + critical lint coordination
(R3-T3 panic-lint allow + cfg!(debug_assertions) gate; R3-T4
trybuild dep coordination) + design fixes (R3-T5 SensorBitDepth
constructor; R3-T7 assert.success contract clarification; R3-T8
sanitize-check preview descent; R3-T11 Acceptance 8 wording) and
filed TD-005/006/007 for the remaining design items (RawDecodeCause
dispatch; PathBuf empty-path) — session 02 implementation will
surface and close those in real code.

**Plan amendments this window**: plan v3.1 → v3.2.
- Deliverable 0 / 2 / Acceptance 7 / Plan revisions log: LibRaw pin
  escalated `=0.21.4` → `=0.22.1` per Deliverable 0 pre-flight
  (rationale in `docs/analysis/ANL-001-libraw-cr3-preflight.md` and the
  `0d4a7f7` commit body); cross-series jump exceeded the implementer's
  plan-granted authority so user consultation under the No-Acceptable-
  Trade-offs Policy approved the choice. DN-018 closed.
- Deliverable 1a § unsafe_code discipline: `src/lib.rs` removed from the
  list of files carrying file-level `#![forbid(unsafe_code)]`. Plan v3.1
  prescribed forbid on lib.rs but rustc disallows downgrading `forbid`
  in submodules, which would make `ffi.rs`'s unsafe blocks fail to
  compile no matter what attribute `ffi.rs` carries. Crate-level
  Cargo.toml `allow` is the lib.rs baseline; `exif.rs` / `decode.rs` /
  any future non-FFI files carry the file-level forbid; the
  `unsafe-isolation` CI gate is the third defense layer. Folded into
  the `440388a` scaffolding commit.

---

## Component progress

| Component             | Status                                  | Notes                                                                                                         |
|-----------------------|-----------------------------------------|---------------------------------------------------------------------------------------------------------------|
| `photohelper-cli`     | **implemented (session 01)**            | clap v4 + 7 subcommands; `ingest` real; stubs exit 69; heartbeat + summary via eprintln!.                     |
| `photohelper-core`    | **implemented (session 01)**            | model (PhotoId, AbsPath, CameraId, KnownCamera, ExifOrientation, Aspect, ExifMetadata, IngestOutcome, Photo); error (13 variants); catalog_glue. |
| `photohelper-raw`     | **implemented (session 02)**            | LibRaw 0.22.1 vendored (1.6 MB tarball under `vendor/`); autoconf-driven build.rs + cc-compiled C shim (`cpp/photohelper_libraw_shim.c`) over LibRaw struct types. `exif::read_cr3(path) → RawExif` + `decode::read_raw(path) → RawImage`. Error / RawExifCause / RawDecodeCause enums + RawExif + RawImage + BayerPlane + CfaPattern + SensorLevels + SensorBitDepth + WhiteBalance + CamRgbToXyzD65Matrix all with R2-T6 invariants. Three-layer unsafe-isolation defense (workspace forbid + file-level forbid + rg gate). 3 integration tests pass against CC0 R8 CR3 fixtures. |
| `photohelper-ai`      | scaffolded                              | ort/tract + culling/denoise models land in sessions 03+.                                                      |
| `photohelper-sidecar` | scaffolded                              | XMP read/write (crs:/ph: namespaces) lands when `develop` is wired (~session 04).                             |
| `photohelper-export`  | scaffolded                              | resize + watermark + mozjpeg encode land when `export` is wired (~session 05).                                |
| `photohelper-cameras` | **implemented (session 01)**            | CameraProfile trait + CanonR8 stub + CameraRegistry::for_exif with normalization.                             |
| `photohelper-catalog` | **implemented (session 01)** (8th crate)| Catalog::open with file-lock + WAL + magic-byte + schema-version + wal_checkpoint warn; upsert with BEGIN IMMEDIATE + supersede + poison ROLLBACK; PhotoRow boundary; v1 schema authoritatively documented in `docs/decisions/0001-catalog-schema-v1.md`. |

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
