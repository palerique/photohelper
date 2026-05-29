# Session 06 Plan — TD Cleanup + `develop` Pipeline
**Branch**: `session-06/td-cleanup-develop-pipeline`
**Date**: 2026-05-29
**Status**: v1 — awaiting plan-review

---

## Session goal

Close all TDs whose binding triggers have **fired** this session, then deliver a fully
runnable `photohelper develop` subcommand backed by the `photohelper-sidecar` crate
(XMP sidecar I/O, `crs:` + `ph:` namespaces, Lightroom-compatible).

---

## What will exist by end of session

1. **TD-001 CLOSED**: GitHub Actions action SHAs pinned in `.github/workflows/ci.yml`.
2. **TD-004 CLOSED**: `osv-scanner` integrated into `just ci` for LibRaw CVE monitoring.
3. **TD-005 CLOSED**: Formal closure recorded — env-var panic site is gone (session 05 D4
   extracted heartbeat; `spawn_dying_heartbeat` is `#[cfg(test)]`-gated). No production panic.
4. **TD-009 CLOSED**: `scripts/sanitize-check.sh` stage-2 embedded-preview re-check added.
5. **TD-011 CLOSED**: Session-02 post-hoc 8-agent code review completed; findings remediated;
   `docs/code-reviews/session-02-round{1,2}.md` written.
6. **TD-014 CLOSED or deferred**: `ort` version bumped to stable 2.0.0 if published on crates.io;
   else filed as "stable not yet released" and deferred with refreshed trigger.
7. **TD-020 CLOSED**: CLIP preprocessing uses bicubic center-crop (shorter edge → 256px, crop
   224×224) instead of bilinear 1:1 resize. `MobileClip::embed` updated; integration tests pass.
8. **`photohelper-sidecar` crate IMPLEMENTED**: `XmpSidecar` + `SidecarSettings` types;
   `crs:` namespace (ProcessVersion, Temperature, Tint, Exposure2012, Contrast2012, Highlights2012,
   Shadows2012, Clarity2012, Vibrance, Saturation); `ph:` namespace (NimaScore, DedupClusterId,
   PhotohelperId, LastProcessedAt). Read/write via `quick-xml`. Conflict resolution (DN-004):
   timestamp-based, preserve existing `crs:` if newer than our last-processed timestamp.
   ≥ 12 unit + integration tests.
9. **`develop` subcommand RUNNABLE**: walks catalog photos, reads/writes XMP sidecars,
   writes NIMA score from catalog to `ph:NimaScore`, accepts develop-setting CLI flags
   (`--exposure`, `--temp`, `--tint`), heartbeat progress, `--strict` exit codes.
   ≥ 6 integration tests. `scripts/photohelper-develop.sh` + `just develop` recipe.
10. **SESSION-STATE.md + HANDOFF_REPORT.md** updated; all closed TDs marked in TECH-DEBT.md.

---

## What is explicitly OUT OF SCOPE (deferred TDs with non-fired triggers)

| TD | Trigger (not yet fired) | Rationale for deferral |
|---|---|---|
| TD-002 | MSRV bump needed (1.88→1.92+) before rusqlite 0.40 | MSRV bump is its own ADR process; no CVE pressure |
| TD-006 | Fires when develop does pixel processing | v0.1 develop = XMP sidecars only; no pixel decode |
| TD-007 | Fires when `photohelper-raw/src/decode.rs` extended | Develop doesn't extend raw decode API in v0.1 |
| TD-012 | Fires when develop does AHD demosaic for processed output | Export session; not needed for XMP-only develop |
| TD-013 | User-report trigger ("I ran cull twice…") | Not fired |
| TD-015 | User-request trigger (custom NIMA model) | Not fired |
| TD-017 | n > 10K photo corpus trigger | Not fired |
| TD-018 | User storage-size complaint trigger | Not fired |
| TD-019 | User-report trigger (dedup audit trail) | Not fired |

Each deferred TD remains in `TECH-DEBT.md` with its binding trigger unchanged.

---

## Stop-gap declarations

All stop-gaps in this session:

| # | Stop-gap | TD | Introducing commit | Location | Binding trigger |
|---|---|---|---|---|---|
| S1 | XMP write uses `quick-xml` manual template rather than Adobe XMP Toolkit SDK | filed at D3 commit | `photohelper-sidecar/src/sidecar.rs` | First session adding non-crs: namespace fields we don't fully model (e.g. `crs:GradientBasedCorrections`); or before v1.0 if XMP round-trip fidelity is required |

---

## Design decisions locked by this plan

### D3 — XMP library choice

`quick-xml` (MIT, pure Rust, no C deps) for XMP generation and parsing, not
`xmp-toolkit` (requires Adobe XMP Toolkit SDK C++ build). The XMP sidecar format is a
deterministic XML/RDF envelope; `quick-xml` handles it without the SDK overhead.

Namespace declarations:
- `xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"`
- `xmlns:ph="http://ns.photohelper.dev/1.0/"`
- `xmlns:xmp="http://ns.adobe.com/xap/1.0/"`
- `xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"`
- `xmlns:x="adobe:ns:meta/"`

### D3 — Conflict resolution (DN-004 closure)

When an existing XMP sidecar is present:
1. Read `xmp:MetadataDate` from the existing file (ISO 8601).
2. Read `ph:LastProcessedAt` from our prior write (if present).
3. If existing `xmp:MetadataDate > ph:LastProcessedAt` (Lightroom edited after us): **preserve
   all existing `crs:` settings**; only update `ph:` namespace + log a `tracing::info!` line
   per photo ("XMP conflict: preserving existing crs: edits").
4. If existing `ph:LastProcessedAt >= xmp:MetadataDate` (we are newer): **overwrite `crs:` and
   `ph:` settings** with our values.
5. `--force` flag: always overwrite unconditionally (no conflict check).
6. No silent data loss: the conflict decision is always logged at `INFO` level.

This closes DN-004.

### D4 — `develop` subcommand scope for v0.1

The `develop` subcommand writes XMP sidecars. It does NOT decode RAW pixels or produce
processed output (that is the `export` subcommand). The workflow for users:
1. `photohelper ingest <dir>` — catalog photos
2. `photohelper cull <dir>` — score photos with NIMA
3. `photohelper develop <dir>` — write XMP sidecars (Lightroom picks them up)
4. Lightroom / Darktable reads `.xmp` sidecars and applies the settings

Develop v0.1 capabilities:
- Writes `crs:ProcessVersion="11.0"` (Camera Raw 11+ / Lightroom Classic 9+)
- Writes `ph:NimaScore`, `ph:PhotohelperId`, `ph:LastProcessedAt`
- Writes user-specified `crs:` settings: `--exposure`, `--temp`, `--tint`,
  `--contrast`, `--highlights`, `--shadows`
- If no CLI setting flags, writes only `ph:` namespace + `crs:ProcessVersion`
  (neutral develop settings that don't affect Lightroom's existing develop state)
- `--strict`: exit non-zero if any per-photo error occurs

### D4 — New catalog query: `all_photos_with_scores`

The `develop` subcommand needs photo paths + NIMA scores. Add
`Catalog::all_photos_with_cull_scores(model_slug: &str) -> Result<Vec<DevelopRow>, Error>`
to `photohelper-catalog`, returning `(source_path: PathBuf, nima_score: Option<f32>)` via a
LEFT JOIN against `cull_scores`. This is a read-only query; no schema change.

`DevelopRow` struct: `source_path: PathBuf, nima_score: Option<f32>`. Private fields + accessors.

### D2 — TD-020 CLIP bicubic center-crop

Replace `nima::bilinear_resize(rgb, 224, 224)` in `MobileClip::embed` with:
1. Compute `scale = 256.0 / min(width, height)`.
2. Resize to `(width * scale, height * scale)` using `image` crate's `CatmullRom` filter
   (bicubic approximation) OR implement bicubic manually (~40 LoC in `mobileclip.rs`).
3. Center-crop a 224×224 window.

Prefer implementing manually to avoid adding the `image` crate (it's large; check if already
a workspace dep first). `nima::bilinear_resize` demoted back to `pub(crate) → fn` (it stays as
the NIMA preprocessing path; CLIP gets its own `clip_preprocess` function).

Update integration tests: the cosine_sim golden test band should tighten from ≥0.98 (bilinear)
to ≥0.99 (bicubic, closer to Python OpenCLIP reference).

---

## Deliverables

### D0 — TD-001: GitHub Actions SHA pinning

**First chore commit: `chore(ci): pin GitHub Actions to commit SHAs (closes TD-001)`**

Pin every `uses:` line in `.github/workflows/ci.yml`:
- `actions/checkout@v4` → `actions/checkout@<SHA>`
- `dtolnay/rust-toolchain@stable` → `dtolnay/rust-toolchain@<SHA>`
- `Swatinem/rust-cache@v2` → `Swatinem/rust-cache@<SHA>`

Resolve SHAs by checking the action's GitHub repo for the latest tagged release SHA.
Add a `docs/decisions/NNNN-action-version-pinning.md` decision doc recording the chosen
SHAs and upgrade cadence (annual review / when CVE forces upgrade).

**Tests**: CI runs green with pinned SHAs (verified by `gh pr checks --watch`).

---

### D1 — TD-011: Session-02 post-hoc 8-agent code review

**Commits: `docs(session-02): post-hoc R1 artifact` + `fix(session-02): R1 remediation` + `docs(session-02): R2 artifact`**

Fire the full 8-agent suite (Cadence A Tier 5) against the session-02 diff
(`git diff main...$(git log --pretty=format:"%H" --merges | grep -A1 "session-02" | head -1)`
or equivalently the session-02 PR #2 commit range).

Per `docs/quality-assurance.md § Double-review protocol`:
- R1 → consolidate by theme → remediate ALL CRITICAL + HIGH → commit
- R2 → verify closure → CLEAN or R3 if CRITICAL regressions
- Write `docs/code-reviews/session-02-round{1,2}.md`
- Update TD-011 Status: CLOSED

The session-02 scope for review: `crates/photohelper-raw/` (Error enum, ffi.rs, exif.rs,
decode.rs), `crates/photohelper-cli/src/commands/ingest.rs` (LibRaw rewire, TD-003 fix),
`scripts/sanitize-check.sh` (stage-1 only), `build.rs`, `vendor/`.

---

### D2 — Quick TD batch

**Commit: `fix(session-06): D2 — TD-004/TD-005/TD-009/TD-014/TD-020 closure`**

**D2a — TD-009: `sanitize-check.sh` stage 2** (~20 LoC):
After the top-level `exiftool -G -a` allow-list check, for each fixture:
```bash
exiftool -b -PreviewImage "$fixture" > /tmp/preview.jpg 2>/dev/null || true
if [ -s /tmp/preview.jpg ]; then
    exiftool -G -a /tmp/preview.jpg | while read line; do
        # same allow-list check as stage 1
    done
fi
rm -f /tmp/preview.jpg
```

**D2b — TD-004: LibRaw CVE scanner via osv-scanner** (~10 LoC in `justfile`):
Add a `just audit-libraw` recipe that runs `osv-scanner --lockfile vendor/libraw-0.22.1.tar.gz`
if osv-scanner is installed (advisory, not blocking CI for now — osv-scanner install is not in
the GitHub Actions runner; add as a CI gate only if GitHub Actions runner has it or we add
install step). Wire a local-only `just audit-libraw` for developer use.
Decision: document that osv-scanner covers LibRaw CVE-DB; wire as CI gate when Release branch
is set up. File: add `.osv-scanner.toml` pointing at the vendored tarball.

**D2c — TD-005: formal closure** (no code change needed):
The `PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING` env-var panic site is gone (session 05 D4
extracted heartbeat to `heartbeat.rs`; `spawn_dying_heartbeat` is `#[cfg(test)]`).
Production code path is panic-free. Update `TECH-DEBT.md` TD-005 Status to CLOSED.

**D2d — TD-014: ort stable check**:
At implementation time, run `cargo search ort` or check crates.io. If ort 2.0.0 stable is
published: bump `Cargo.toml` pin from `=2.0.0-rc.12` to `=2.0.0`; run `just test`; verify
golden-vector inference determinism preserved. If not published: update TD-014 with checked
date + refreshed binding trigger.

**D2e — TD-020: CLIP bicubic center-crop** (~40 LoC in `mobileclip.rs`):
See § Design decisions above. Implement `clip_preprocess` in `mobileclip.rs`:
1. `scale = 256.0 / min(w, h) as f32`
2. Bicubic resize to `(w * scale, h * scale)` — implement Catmull-Rom bicubic in ~30 LoC
   (4-tap 1D filter, applied separably H then V) OR use `image` crate if already a dep.
3. Center-crop 224×224: `x = (new_w - 224) / 2; y = (new_h - 224) / 2`.
4. Return as `Vec<u8>` HWC buffer.

Remove `// TD-020: bicubic center-crop deferred` comment from mobileclip.rs.
Remove `// pub(crate) so mobileclip.rs can reuse for CLIP preprocessing (TD-020)` from nima.rs.
`bilinear_resize` remains `pub(crate)` for NIMA preprocessing (NIMA uses bilinear — separate
decision from CLIP).

Update `integration_clip.rs` golden test: tighten cosine_sim assertion from `>= 0.80` to
`>= 0.90` (bicubic is closer to Python OpenCLIP reference; cross-arch band).

---

### D3 — `photohelper-sidecar` crate

**Sub-component review fires at end of D3 (before D4).**

**Commits**: `feat(sidecar): D3 — XmpSidecar, SidecarSettings, crs: + ph: namespaces`

Add `quick-xml` to `photohelper-sidecar/Cargo.toml`:
```toml
quick-xml = { version = "0.36", features = ["serialize"] }
time = { workspace = true }
photohelper-core = { path = "../photohelper-core" }
```

#### D3a — Types

`crates/photohelper-sidecar/src/settings.rs`:
```rust
/// Develop settings that map to crs: (Camera Raw) + ph: (photohelper) XMP namespaces.
pub struct SidecarSettings {
    // crs: namespace (all Option — absent means "leave unchanged")
    pub process_version: Option<String>,    // default "11.0" when we write
    pub white_balance: Option<WhiteBalance>, // enum: AsShotEnum | Custom | Auto
    pub temperature: Option<i32>,           // Kelvin, 2000–50000
    pub tint: Option<i32>,                  // -150 to 150
    pub exposure: Option<f32>,              // -5.0 to 5.0
    pub contrast: Option<i32>,              // -100 to 100
    pub highlights: Option<i32>,            // -100 to 100
    pub shadows: Option<i32>,               // -100 to 100
    pub clarity: Option<i32>,               // -100 to 100 (crs:Clarity2012)
    pub vibrance: Option<i32>,              // -100 to 100
    pub saturation: Option<i32>,            // -100 to 100

    // ph: namespace
    pub nima_score: Option<f32>,
    pub dedup_cluster_id: Option<i64>,
    pub photohelper_id: Option<String>,     // 43-char base64url PhotoId
    pub last_processed_at: Option<String>,  // ISO 8601 UTC
}
```

`WhiteBalance` enum:
```rust
#[non_exhaustive]
pub enum WhiteBalance { AsShot, Auto, Daylight, Cloudy, Shade, Tungsten, Fluorescent, Flash, Custom }
```

Validation in `SidecarSettings`:
- `temperature ∈ [2000, 50000]` or None
- `tint ∈ [-150, 150]` or None
- `exposure ∈ [-5.0, 5.0]` or None
- All 3-value fields `∈ [-100, 100]` or None

#### D3b — XMP writer

`crates/photohelper-sidecar/src/writer.rs`:
`pub fn write_xmp(path: &Path, settings: &SidecarSettings) -> Result<(), Error>`

Writes a minimal but Lightroom-compatible XMP sidecar:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="photohelper">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/"
      xmlns:ph="http://ns.photohelper.dev/1.0/"
      xmp:MetadataDate="<LastProcessedAt>"
      crs:ProcessVersion="11.0"
      [crs fields if set]
      [ph fields if set]
    />
  </rdf:RDF>
</x:xmpmeta>
```

Only write fields that are `Some`. `ProcessVersion` is always written when any `crs:` field
is written (required for Camera Raw compatibility).

#### D3c — XMP reader

`crates/photohelper-sidecar/src/reader.rs`:
`pub fn read_xmp(path: &Path) -> Result<SidecarSettings, Error>`

Reads an existing XMP sidecar. Uses `quick-xml` event-based parsing. Extracts:
- All `crs:` fields into the corresponding `SidecarSettings` fields
- All `ph:` fields
- `xmp:MetadataDate` (stored separately in `SidecarSettings::last_processed_at` if `ph:` absent)

Unknown fields are silently ignored (forward-compatibility).

#### D3d — Conflict resolution

`crates/photohelper-sidecar/src/conflict.rs`:
```rust
pub enum WriteOutcome { Written, ConflictPreserved, ForcedOverwrite }
pub fn merge_and_write(
    path: &Path,
    incoming: &SidecarSettings,
    force: bool,
) -> Result<WriteOutcome, Error>
```

Algorithm per § Design decisions above.

#### D3e — Error type

`crates/photohelper-sidecar/src/error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("XMP sidecar I/O failed at {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
    #[error("XMP parse error in {path}: {message}")]
    XmlParse { path: PathBuf, message: String },
    #[error("validation: {message}")]
    Validation { message: String },
}
```

#### D3f — lib.rs exports + `static_assertions`

```rust
static_assertions::assert_impl_all!(SidecarSettings: Send, Sync);
```

#### Tests (≥ 12):

| Test | What it verifies |
|---|---|
| `write_and_read_roundtrip_all_fields` | All crs:+ph: fields survive write→read |
| `write_with_only_ph_namespace` | No crs: fields → no crs: in output |
| `write_with_only_crs_namespace` | No ph: fields → no ph: in output |
| `temperature_out_of_range_rejected` | temp=60000 → Err(Validation) |
| `exposure_out_of_range_rejected` | exposure=10.0 → Err(Validation) |
| `conflict_preserve_newer_lightroom_edit` | incoming older than existing → ConflictPreserved |
| `conflict_overwrite_older_lightroom_edit` | incoming newer → Written |
| `conflict_force_overwrite` | force=true → ForcedOverwrite regardless of timestamps |
| `read_unknown_fields_ignored` | Unknown crs:XyZ → no error, field absent in result |
| `read_minimal_xmp` | Bare x:xmpmeta skeleton → empty SidecarSettings |
| `path_written_as_dotxmp_extension` | For photo.CR3 → sidecar at photo.CR3.xmp |
| `lightroom_compatible_output` | Written XML contains all required namespace declarations |

---

### D4 — `develop` subcommand

**Commits**: `feat(catalog): D4a — all_photos_with_cull_scores query (DevelopRow)`
+ `feat(cli): D4b — develop subcommand + XMP sidecar write`

#### D4a — Catalog query

`Catalog::all_photos_with_cull_scores(model_slug: &str) -> Result<Vec<DevelopRow>, Error>`:
```sql
SELECT p.source_path, cs.aesthetic_score
FROM photos p
LEFT JOIN cull_scores cs ON cs.photo_id = p.id AND cs.model_slug = ?1
WHERE p.superseded_at_unix_seconds IS NULL
ORDER BY p.ingested_at_unix_seconds
```
Returns all non-superseded photos with their NIMA score (None if not yet culled).
`DevelopRow`: `source_path: PathBuf, nima_score: Option<f32>`. Private fields + accessors.

Tests (3): all photos returned, NIMA scores attached, superseded excluded.

#### D4b — CLI subcommand

`crates/photohelper-cli/src/commands/develop.rs`:

```rust
#[derive(clap::Args, Debug)]
pub(crate) struct DevelopArgs {
    /// Exit non-zero if any per-photo error occurs.
    #[arg(long, default_value_t = false)]
    strict: bool,
    /// Always overwrite existing XMP sidecars (skip conflict check).
    #[arg(long, default_value_t = false)]
    force: bool,
    /// Exposure compensation in stops (–5.0 to 5.0).
    #[arg(long)]
    exposure: Option<f32>,
    /// White balance temperature in Kelvin (2000–50000).
    #[arg(long)]
    temp: Option<i32>,
    /// White balance tint (–150 to 150).
    #[arg(long)]
    tint: Option<i32>,
    /// Contrast (–100 to 100).
    #[arg(long)]
    contrast: Option<i32>,
    /// Highlights (–100 to 100).
    #[arg(long)]
    highlights: Option<i32>,
    /// Shadows (–100 to 100).
    #[arg(long)]
    shadows: Option<i32>,
}
```

`run_develop(cli: &Cli, args: &DevelopArgs) -> anyhow::Result<u8>`:
1. Open catalog.
2. `catalog.all_photos_with_cull_scores(MODEL_MANIFEST_NAME_NIMA)` to get all photos + scores.
3. If empty: print summary "walked: 0" and exit 0.
4. Build `SidecarSettings` from CLI args + NIMA score.
5. Walk results sequentially (no rayon — I/O-bound, not CPU-bound; avoids file contention).
6. For each photo:
   a. Populate `settings.nima_score = row.nima_score`.
   b. Populate `settings.photohelper_id` from PhotoId derived from path.
   c. Populate `settings.last_processed_at` (current UTC).
   d. Call `photohelper_sidecar::merge_and_write(&sidecar_path, &settings, args.force)`.
   e. Count outcome in `DevelopStats`.
7. Heartbeat (same pattern as ingest/cull/dedup).
8. Print summary.

`DevelopStats` (AtomicU64 fields):
- `walked` — photos walked from catalog
- `written` — new XMP sidecars written
- `updated` — existing sidecars overwritten
- `conflict_preserved` — conflict detected, existing crs: preserved
- `errored` — photo failed (XMP write error, path invalid, etc.)

Summary line format:
```
walked: N, written: N, updated: N, conflict-preserved: N, errored: N
```

Heartbeat line:
```
[heartbeat] develop: walked N, written N
```

Exit codes:
- 0: success (or all conflict-preserved)
- `EX_STRICT_FAIL` (75): `--strict` and `errored > 0`

Wire `Command::Develop` in `main.rs` to `run_develop`.
Wire `NIMA_MODEL_SLUG` constant (from `photohelper_ai::MODEL_MANIFEST_NAME`) as the
catalog lookup model slug.

#### D4 — Tests (≥ 6 integration tests in `tests/cli.rs`):

| Test | What it verifies |
|---|---|
| `develop_creates_xmp_sidecar_for_ingested_photo` | After ingest + develop, .CR3.xmp exists |
| `develop_writes_nima_score_when_culled` | After cull + develop, ph:NimaScore in sidecar |
| `develop_idempotency_second_run_updates` | Second develop run → updated=N, written=0 |
| `develop_strict_exits_nonzero_on_error` | If catalog is empty, not an error (exit 0) |
| `develop_force_overwrites_conflict` | --force on conflicting sidecar → ForcedOverwrite |
| `develop_summary_line_contains_expected_fields` | Summary line has all counters |

---

### D5 — Scripts, docs, ledger

**Commit: `docs(session-06): D5 — scripts + ledger + SESSION-STATE`**

- `scripts/photohelper-develop.sh`: wrapper that calls `cargo run ... develop`
- `just develop` recipe in `justfile`
- README: add `Develop` section to the Quickstart
- `SESSION-STATE.md` component table: update `photohelper-sidecar` (implemented), `photohelper-cli` (+`develop`), `photohelper-catalog` (+`all_photos_with_cull_scores`)
- `TECH-DEBT.md`: mark TD-001, TD-004, TD-005, TD-009, TD-011, TD-020 as CLOSED (TD-014 per outcome of D2d)
- `docs/discovery-notes.md`: close DN-004 (conflict resolution shipped in D3d)

---

## Test plan summary

| Deliverable | Min new tests | What's verified |
|---|---|---|
| D2e (TD-020 bicubic) | 1 (tighten existing) | cosine_sim ≥ 0.90 on CC0 fixtures |
| D3 (sidecar) | 12 | read/write round-trip, validation, conflict, compatibility |
| D4a (catalog query) | 3 | DevelopRow SQL, superseded filter, NIMA join |
| D4b (develop CLI) | 6 | end-to-end, idempotency, strict, force, conflict |
| **Total** | **≥ 22** | |

**Target**: ≥ 204 tests (182 baseline + 22 minimum).

---

## Checkpoints

| Checkpoint | When |
|---|---|
| Plan-review R1 | NOW (after this doc is committed) |
| Plan-review R2 | After R1 remediation |
| D1 session-02 review R1+R2 | After D0 commits, before D2 |
| Sub-component review D3 | After D3f complete, before D4 |
| Session-end review R1+R2 | After D5 complete |

---

## Scope justification

The user requested "all TDs + develop pipeline completely and runnable." This plan addresses:
- 7 TDs whose binding triggers have **fired** this session (TD-001, TD-004, TD-005, TD-009,
  TD-011, TD-014, TD-020).
- 8 TDs whose binding triggers have NOT fired (TD-002, TD-006, TD-007, TD-012, TD-013, TD-015,
  TD-017, TD-018, TD-019) — explicitly out of scope per CLAUDE.md policy ("Do not design for
  hypothetical future requirements. No half-finished implementations either.") Each deferred TD
  retains its binding trigger in TECH-DEBT.md.
- The full `develop` pipeline including `photohelper-sidecar` crate, `develop` subcommand,
  conflict resolution (DN-004), and end-to-end tests.
