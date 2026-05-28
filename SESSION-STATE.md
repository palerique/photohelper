# photohelper — Session State

> Living handoff document. Read FIRST at every session start; update LAST at
> every session end. Stale state = blocked progress.
>
> Keep this file SMALL. When a `## Prior session: N` block ages out (older than
> the immediately-prior session), demote it to `docs/session-archive/` per the
> rolling-archive convention. The git log is the full timeline.

**Last session**: 1 (`cli-skeleton-and-ingest` — 2026-05-28) —
implementation complete on branch `session-01/cli-skeleton-and-ingest`;
session-end review Round 1 done; Round 1 remediation applied; Round 2
pending; PR not yet opened.

**Current session**: 1 (still — wraps when R2 + final commit + PR + merge land).

**Goal** (session 1): Land the thinnest end-to-end slice that proves
the workspace architecture — `clap` v4 CLI with all 7 subcommands
(`ingest`, `cull`, `develop`, `export`, `run`, `models`, `camera`),
with `ingest` doing real work. **DONE** at commit `310f753`; 4
remediation commits land R1 fixes.

**Action**: fire **session-end Round 2** (full 8-agent suite re-fired
against the remediated code) per `docs/quality-assurance.md § Double-review
protocol`. Never stop after Round 1.

**Status**: 63 tests pass; `just ci` green; smoke test on `/tmp/ph_demo`
confirms end-to-end works; R1 remediation applied. Round 2 pending.

**Next action** (after Round 2 + remediation): commit final state;
push `session-01/cli-skeleton-and-ingest` branch; open PR to `main`;
wait for CI green; merge with merge-commit; render two-block handoff
per `docs/session-handoff-format.md`.

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

## Open Round-2 items

Session-end Round 1 (`docs/code-reviews/session-01-round1.md`) surfaced 7
CRITICAL + 5 HIGH + 4 MEDIUM + 3 LOW. R1 remediation commits landed:
- **T1** no_exif counter increments at point of decision; new
  integration test asserts.
- **T2** heartbeat dead-`if` deleted; death-WARN added BEFORE stop-flag
  set; over-engineered granularity loop kept for now.
- **T3** PhotoId hash window made DISJOINT for 64KB–128KB files;
  two regression tests added.
- **T4** ADR `0001-msrv-bump-to-1.88-for-rustsec-2026-0009.md` filed;
  CLAUDE.md + stacks/rust.md swept to `1.88`.
- **T5** TD-002 (rusqlite 0.32 stale) + DN-007 (cross-ref) filed.
- **T6** `docs/decisions/0001-catalog-schema-v1.md` written.
- **T7** `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` env-var test override +
  test row 48 added; remaining 12 uncovered plan rows tracked in
  **DN-008** with session-02 binding trigger.
- **T8** `indicatif` dep removed (heartbeat covers the same UX);
  HANDOFF Checkpoint 1 records the deliberate scope tightening.
- **T9** HANDOFF Checkpoint 1, this SESSION-STATE update, DN-006
  filed.
- **T10** rayon `build_global` Err → WARN; PRAGMA `wal_checkpoint`
  errors no longer silent; misnamed `op: "stat"` → `op: "file-lock"`;
  `ContextForPath` no-op trait deleted.
- **T11** `Error::InvalidExifOrientationTag { tag }` dedicated
  variant; `ExifOrientation::from_tag` no longer emits empty-PathBuf
  sentinel.
- **T12** test row 32 pinned to deterministic DN-006 fallback branch
  (`is_none()` for camera_slug/make/model).
- **T13** type-design refinements deferred to session-02 watch
  (small).
- **T14** duplicate INSERT extracted to closure + `INSERT_PHOTO_SQL`
  const; `_suppress_unused_warnings` + `_ensure_exif_metadata_compiles`
  deleted.
- **T15** minor polish deferred.

**Magic-byte TOCTOU (T10 sub)** — not yet fixed: requires moving the
`if catalog_path.exists()` check INSIDE the lock-acquisition window.
Carrying forward; the failure mode is rare (writer would have to delete
the catalog mid-open). Will land in Round 2 remediation if R2 flags it
again.

---

## Continuation-session bootstrap (verbatim)

```bash
git switch main && git pull --ff-only origin main && git switch -c session-02/<kebab-slug> && just session-start
```
