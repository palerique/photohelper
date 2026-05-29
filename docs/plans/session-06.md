# Session 06 Plan — TD Cleanup + `develop` Pipeline
**Branch**: `session-06/td-cleanup-develop-pipeline`
**Date**: 2026-05-29
**Status**: v2 — R1 remediation (closes 3C+9H+6M)

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
8. **`photohelper-sidecar` crate IMPLEMENTED**: `SidecarSettings` (private fields + builder,
   validated at construction); `crs:` namespace (Temperature, Tint, Exposure2012, Contrast2012,
   Highlights2012, Shadows2012); `ph:` namespace (NimaScore, DedupClusterId, PhotohelperId,
   LastProcessedAt). Sidecar path: `<stem>.xmp` (Lightroom-compatible: extension replaced, not
   appended). Read/write via `quick-xml`. Atomic write (temp+rename). Conflict resolution (DN-004)
   with defined fallback for missing timestamps. ≥ 16 unit + integration tests.
9. **`develop` subcommand RUNNABLE**: walks catalog photos, reads/writes XMP sidecars,
   writes NIMA score from catalog to `ph:NimaScore`, accepts develop-setting CLI flags
   (`--exposure`, `--temp`, `--tint`, `--contrast`, `--highlights`, `--shadows`),
   heartbeat progress, `--strict` exit codes, `file_missing` + `derive_failed` counters.
   ≥ 9 integration tests. `scripts/photohelper-develop.sh` + `just develop` recipe.
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

**Scope note (R1-P)**: This session closes 7 TDs total: 4 with fired binding triggers (TD-009,
TD-011, TD-014, TD-020) and 3 proactive closures before their triggers fire (TD-001, TD-004,
TD-005). The "9 TDs deferred" table above is the complete list of TDs with non-fired triggers.

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

### D3 — Sidecar path convention

Sidecar path: `raw_path.with_extension("xmp")` — the RAW extension is **replaced**, not appended.
- `IMG_0001.CR3` → `IMG_0001.xmp` ✓ (Lightroom Classic / Camera Raw compatible)
- NOT `IMG_0001.CR3.xmp` ✗ (Darktable convention; Lightroom will not find it)

This is the XMP Part 3 specification for RAW files. The test name reflects this:
`sidecar_path_for_cr3_replaces_extension` asserts `photo.CR3` → `photo.xmp`.

### D3 — Conflict resolution (DN-004 closure)

Timestamps are compared as `time::OffsetDateTime` (not as strings) to handle timezone offsets.
Parse both `xmp:MetadataDate` and `ph:LastProcessedAt` to `OffsetDateTime`; string comparison
of ISO 8601 strings is incorrect when timezones differ.

When an existing XMP sidecar is present, use this decision table (covers all 4 timestamp cases):

| `xmp:MetadataDate` | `ph:LastProcessedAt` | Outcome |
|---|---|---|
| `Some(md)` | `Some(lp)` | `md > lp` → `ConflictPreserved`; else `Overwritten` |
| `Some(_)` | `None` | `ConflictPreserved` (first photohelper run; unknown prior state) |
| `None` | `Some(_)` | `ConflictPreserved` + `tracing::warn!` (existing sidecar has no timestamp) |
| `None` | `None` | If any `crs:` field exists in the sidecar: `ConflictPreserved` + `tracing::warn!`; else `Created` |

`--force` flag: always `ForcedOverwrite` regardless of timestamps.

No silent data loss: every conflict decision is logged at `INFO` (or `WARN` for missing-timestamp
cases). This closes DN-004.

**Note on XMP reader for timestamps**: malformed `xmp:MetadataDate` (unparseable ISO 8601) → treat
as absent (`None`) + `tracing::warn!`. Do not return `Err` from `read_xmp` on a single malformed field.

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

The `develop` subcommand needs photo ID, path, and NIMA score. Add
`Catalog::all_photos_with_cull_scores(model_slug: &str) -> Result<Vec<DevelopRow>, Error>`
to `photohelper-catalog`. The `model_slug` argument is `photohelper_ai::MODEL_SLUG`
(= `"nima-aesthetic-v1"`) — **not** `MODEL_MANIFEST_NAME` which is the manifest directory name,
not the catalog column value.

SQL:
```sql
SELECT p.id, p.source_path, cs.aesthetic_score
FROM photos p
LEFT JOIN cull_scores cs ON cs.photo_id = p.id AND cs.model_slug = ?1
WHERE p.superseded_at_unix_seconds IS NULL
ORDER BY p.ingested_at_unix_seconds
```

`DevelopRow` struct: `photo_id: PhotoId, source_path: PathBuf, nima_score: Option<f32>`.
Private fields + accessors. Carrying `photo_id` from the catalog avoids per-photo
`PhotoId::derive` disk reads in D4b.

### D2 — TD-020 CLIP bicubic center-crop

Replace `nima::bilinear_resize(rgb, 224, 224)` in `MobileClip::embed` with:
1. Compute `scale = 256.0 / min(width, height)`.
2. Resize to `(width * scale, height * scale)` using `image` crate's `CatmullRom` filter
   (bicubic approximation) OR implement bicubic manually (~40 LoC in `mobileclip.rs`).
3. Center-crop a 224×224 window.

Prefer implementing manually to avoid adding the `image` crate (it's large; check if already
a workspace dep first). CLIP gets its own `clip_preprocess` function in `mobileclip.rs`.

After this change, `nima::bilinear_resize` is called only by `nima.rs::Nima::score()` (internal,
same file). **Demote from `pub(crate)` to `fn`** (file-private). Remove the TD-020 comment
from `nima.rs:255` explaining the `pub(crate)` elevation.

Update integration tests: the cosine_sim golden test band should tighten from ≥0.80 (bilinear
baseline; empirical cosine_sim=0.843 from ANL-003) to ≥0.90 (bicubic; cross-arch tolerance band).
This matches D2e implementation spec below.

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

**Note (R1-L)**: D1 is sequenced AFTER D2 to front-load guaranteed progress (D2 banks 5 TD
closures before the context-intensive review session begins).

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

Any TD-009-related findings from this review are tracked by D2a (do not remediate inline
during D1 — they are already scheduled).

---

### D2 — Quick TD batch

**Five separate commits (one logical change per commit per CLAUDE.md):**

**D2a — `fix(scripts): TD-009 — sanitize-check.sh stage-2 embedded-preview re-check`** (~20 LoC):
After the top-level `exiftool -G -a` allow-list check, for each fixture:
```bash
preview_tmp=$(mktemp /tmp/ph-sanitize-XXXXXX.jpg)
exiftool -b -PreviewImage "$fixture" > "$preview_tmp" 2>/dev/null || true
if [ -s "$preview_tmp" ]; then
    exiftool -G -a "$preview_tmp" | while read line; do
        # same allow-list check as stage 1
    done
fi
rm -f "$preview_tmp"
```
Use `mktemp` (not `/tmp/preview.jpg`) to avoid parallel-CI clobber — parallel jobs on the same
host share `/tmp`; a race condition would allow PII to bypass the sanitization gate.

**D2b — `chore(ci): TD-004 — osv-scanner libraw CVE monitoring`** (~10 LoC in `justfile`):
Add a `just audit-libraw` recipe that runs `osv-scanner --lockfile vendor/libraw-0.22.1.tar.gz`
if osv-scanner is installed (advisory, not blocking CI for now — osv-scanner install is not in
the GitHub Actions runner). Wire local-only for developer use. Add `.osv-scanner.toml`.

**D2c — `chore(session-06): TD-005 — formal closure (env-var panic removed in session 05 D4)`**
(no code change needed):
The `PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING` env-var panic site is gone (session 05 D4
extracted heartbeat to `heartbeat.rs`; `spawn_dying_heartbeat` is `#[cfg(test)]`).
Production code path is panic-free. Update `TECH-DEBT.md` TD-005 Status to CLOSED.

**D2d — `chore(deps): TD-014 — ort stable version check`**:
At implementation time, run `cargo search ort` or check crates.io. If ort 2.0.0 stable is
published: bump `Cargo.toml` pin from `=2.0.0-rc.12` to `=2.0.0`; run `just test`; verify
golden-vector inference determinism preserved. If not published: update TD-014 with checked
date + refreshed binding trigger.

**D2e — `fix(ai): TD-020 — CLIP bicubic center-crop preprocessing`** (~40 LoC in `mobileclip.rs`):
See § Design decisions above. Implement `clip_preprocess` in `mobileclip.rs`:
1. `scale = 256.0 / min(w, h) as f32`
2. Bicubic resize to `(w * scale, h * scale)` — implement Catmull-Rom bicubic in ~30 LoC
   (4-tap 1D filter, applied separably H then V) OR use `image` crate if already a dep.
3. Center-crop 224×224: `x = (new_w - 224) / 2; y = (new_h - 224) / 2`.
4. Return as `Vec<u8>` HWC buffer.

Remove `// TD-020: bicubic center-crop deferred` comment from `mobileclip.rs`.
Remove `// pub(crate) so mobileclip.rs can reuse for CLIP preprocessing (TD-020)` comment from `nima.rs`.
**Demote `bilinear_resize` from `pub(crate)` to `fn`** — after this change it is only called from
within `nima.rs` (NIMA preprocessing path). `pub(crate)` was elevated specifically for CLIP reuse;
that reason is now gone.

Update `integration_clip.rs` golden test: tighten cosine_sim assertion from `>= 0.80` to
`>= 0.90` (bicubic is closer to Python OpenCLIP reference; cross-arch tolerance band).

---

### D3 — `photohelper-sidecar` crate

**Sub-component review fires at end of D3 (before D4).**

**Commits**: `feat(sidecar): D3 — XmpSidecar, SidecarSettings, crs: + ph: namespaces`

Add to `photohelper-sidecar/Cargo.toml` (no `photohelper-core` — unused; would trigger lint):
```toml
quick-xml = { version = "0.36", features = ["serialize"] }
time = { workspace = true }
thiserror.workspace = true
static_assertions.workspace = true
```

#### D3a — Types

`crates/photohelper-sidecar/src/settings.rs`:

`SidecarSettings` uses **private fields + builder** (consistent with `Photo`, `ImageEmbedding`).
Validation runs at construction time; callers cannot construct invalid settings.

```rust
/// Develop settings mapping to crs: (Camera Raw) and ph: (photohelper) XMP namespaces.
/// Private fields; use `SidecarSettings::builder()` to construct.
pub struct SidecarSettings { /* private */ }

impl SidecarSettings {
    /// Builder entry point.
    pub fn builder() -> SidecarSettingsBuilder { SidecarSettingsBuilder::default() }
    // Accessors: temperature(), tint(), exposure(), contrast(), highlights(),
    // shadows(), nima_score(), dedup_cluster_id(), photohelper_id(), last_processed_at()
}

pub struct SidecarSettingsBuilder {
    temperature: Option<i32>,         // crs:Temperature, Kelvin ∈ [2000, 50000]
    tint: Option<i32>,                // crs:Tint ∈ [-150, 150]
    exposure: Option<f32>,            // crs:Exposure2012 ∈ [-5.0, 5.0]
    contrast: Option<i32>,            // crs:Contrast2012 ∈ [-100, 100]
    highlights: Option<i32>,          // crs:Highlights2012 ∈ [-100, 100]
    shadows: Option<i32>,             // crs:Shadows2012 ∈ [-100, 100]
    nima_score: Option<f32>,          // ph:NimaScore (written raw; catalog already validates)
    dedup_cluster_id: Option<i64>,    // ph:DedupClusterId
    photohelper_id: Option<String>,   // ph:PhotohelperId (43-char base64url)
    last_processed_at: Option<time::OffsetDateTime>, // ph:LastProcessedAt (ISO 8601 UTC)
}

impl SidecarSettingsBuilder {
    /// Builds and validates. Returns Err(Validation) if any field is out of range.
    pub fn build(self) -> Result<SidecarSettings, Error> { ... }
    // Setter methods: temperature(i32), tint(i32), exposure(f32), contrast(i32), etc.
}
```

**Fields removed from v0.1** (no CLI exposure; reserved for future sessions):
`clarity`, `vibrance`, `saturation`, `white_balance` (`WhiteBalance` enum). The XMP reader
silently ignores these fields when reading existing sidecars (forward-compat already specified).
`process_version` is hardcoded as `"11.0"` in the writer (always written when any `crs:` field
is set; not a user-settable value).

Validation rules (enforced in `builder().build()`):
- `temperature ∈ [2000, 50000]` or None
- `tint ∈ [-150, 150]` or None
- `exposure ∈ [-5.0, 5.0]` or None
- `contrast`, `highlights`, `shadows` each `∈ [-100, 100]` or None

#### D3b — XMP writer (atomic)

`crates/photohelper-sidecar/src/writer.rs`:
`pub fn write_xmp(path: &Path, settings: &SidecarSettings) -> Result<(), Error>`

**Atomic write** (prevents partial/corrupt sidecars on crash):
1. Write to `<path>.phdev.tmp` in the same directory.
2. `fsync` the temp file.
3. `fs::rename("<path>.phdev.tmp", path)` — atomic on POSIX; best-effort on Windows.
4. On error at any step, attempt `fs::remove_file("<path>.phdev.tmp")` silently.

XML template (all namespace declarations always emitted on `rdf:Description`):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="photohelper">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description rdf:about=""
      xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
      xmlns:xmp="http://ns.adobe.com/xap/1.0/"
      xmlns:ph="http://ns.photohelper.dev/1.0/"
      xmp:MetadataDate="<last_processed_at ISO 8601>"
      crs:ProcessVersion="11.0"
      [crs fields if set]
      [ph fields if set]
    />
  </rdf:RDF>
</x:xmpmeta>
```

Only write fields that are `Some`. `crs:ProcessVersion="11.0"` is always written when any `crs:`
field is set (required for Camera Raw compatibility; hardcoded, not user-settable).

#### D3c — XMP reader

`crates/photohelper-sidecar/src/reader.rs`:
`pub fn read_xmp(path: &Path) -> Result<SidecarSettings, Error>`

Reads an existing XMP sidecar. Uses `quick-xml` event-based parsing.

**Lenient read** (unknown or malformed fields are not fatal):
- Unknown field names: silently ignored (forward-compatibility).
- Known fields with malformed values (e.g. `crs:Temperature="not-a-number"`): log
  `tracing::warn!(field = "crs:Temperature", value = "...", "malformed XMP field; ignoring")`,
  treat as `None`. The read succeeds with partial data.
- `xmp:MetadataDate` with unparseable ISO 8601: treat as `None` + `tracing::warn!`.

Extracts: `crs:` fields, `ph:` fields, `xmp:MetadataDate`.

#### D3d — Conflict resolution + `WriteOutcome`

`crates/photohelper-sidecar/src/conflict.rs`:

```rust
/// 1:1 with DevelopStats counters.
#[non_exhaustive]
pub enum WriteOutcome {
    /// New sidecar file created (no prior file existed). → stats.written
    Created,
    /// Existing sidecar overwritten (our timestamp was newer). → stats.updated
    Overwritten,
    /// Existing crs: settings preserved (Lightroom or other tool is newer). → stats.conflict_preserved
    ConflictPreserved,
    /// Existing sidecar overwritten unconditionally (--force). → stats.force_overwritten
    ForcedOverwrite,
}

pub fn merge_and_write(
    path: &Path,
    incoming: &SidecarSettings,
    force: bool,
) -> Result<WriteOutcome, Error>
```

Algorithm: per the decision table in § Design decisions (4 timestamp-presence cases + force).

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
    #[error("atomic write failed for {path}: {source}")]
    AtomicWrite { path: PathBuf, #[source] source: std::io::Error },
}
```

#### D3f — lib.rs exports + `static_assertions`

```rust
static_assertions::assert_impl_all!(SidecarSettings: Send, Sync);
```

#### Tests (≥ 16):

| Test | What it verifies |
|---|---|
| `write_and_read_roundtrip_all_fields` | All crs:+ph: fields survive write→read |
| `write_with_only_ph_namespace` | No crs: fields → no crs: in output |
| `write_with_only_crs_namespace` | No ph: fields → no ph: in output |
| `temperature_out_of_range_rejected` | temp=60000 → Err(Validation) |
| `exposure_out_of_range_rejected` | exposure=10.0 → Err(Validation) |
| `conflict_preserve_newer_lightroom_edit` | incoming older than existing → ConflictPreserved |
| `conflict_overwrite_older_lightroom_edit` | incoming newer → Overwritten |
| `conflict_force_overwrite` | force=true → ForcedOverwrite regardless of timestamps |
| `conflict_missing_metadata_date_preserves` | xmp:MetadataDate absent → ConflictPreserved |
| `conflict_missing_last_processed_preserves` | ph:LastProcessedAt absent → ConflictPreserved |
| `read_unknown_fields_ignored` | Unknown crs:XyZ → no error, field absent in result |
| `read_malformed_temperature_warns_and_returns_none` | crs:Temperature="bad" → None field, no Err |
| `read_malformed_xml_returns_parse_error` | Garbage bytes in .xmp file → Err(XmlParse) |
| `read_minimal_xmp` | Bare x:xmpmeta skeleton → empty SidecarSettings |
| `sidecar_path_for_cr3_replaces_extension` | For photo.CR3 → sidecar at photo.xmp (not photo.CR3.xmp) |
| `write_xmp_atomic_no_partial_on_io_error` | Write to read-only dir → Err(Io), no .phdev.tmp left |
| `write_xmp_to_readonly_dir_returns_io_error` | Err(Io) returned, no panic |
| `tint_out_of_range_rejected` | tint=200 → Err(Validation) |
| `int_crs_field_boundary_rejected` | contrast=101 → Err(Validation) |
| `lightroom_compatible_output` | Written XML contains all required namespace declarations |

---

### D4 — `develop` subcommand

**Commits**: `feat(catalog): D4a — all_photos_with_cull_scores query (DevelopRow)`
+ `feat(cli): D4b — develop subcommand + XMP sidecar write`

#### D4a — Catalog query

`Catalog::all_photos_with_cull_scores(model_slug: &str) -> Result<Vec<DevelopRow>, Error>`:
```sql
SELECT p.id, p.source_path, cs.aesthetic_score
FROM photos p
LEFT JOIN cull_scores cs ON cs.photo_id = p.id AND cs.model_slug = ?1
WHERE p.superseded_at_unix_seconds IS NULL
ORDER BY p.ingested_at_unix_seconds
```
Returns all non-superseded photos with their NIMA score (None if not yet culled).
Pass `photohelper_ai::MODEL_SLUG` (= `"nima-aesthetic-v1"`) — NOT `MODEL_MANIFEST_NAME`.
`DevelopRow`: `photo_id: PhotoId, source_path: PathBuf, nima_score: Option<f32>`.
Private fields + accessors.

Tests (4): all photos returned, NIMA scores attached, superseded excluded,
`all_photos_wrong_model_slug_returns_none_score` (cull_score exists for wrong model → None).

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
2. `catalog.all_photos_with_cull_scores(photohelper_ai::MODEL_SLUG)` — uses `MODEL_SLUG`
   (`"nima-aesthetic-v1"`), NOT `MODEL_MANIFEST_NAME`. `DevelopRow` carries `photo_id` from DB.
3. If empty: print summary `walked: 0, written: 0, ...` and exit 0.
4. Build a base `SidecarSettingsBuilder` from CLI args (`--exposure`, `--temp`, etc.).
5. Spawn heartbeat thread (same `Arc<HeartbeatStop>` pattern as cull/dedup).
6. Walk results **sequentially** (no rayon — sidecar I/O per photo; heartbeat reads stats across
   threads via `AtomicU64`). Per-photo errors never abort the batch.
7. For each photo:
   - **Step a (file-missing pre-check)**: `if !source_path.exists() { warn!; stats.file_missing++; continue; }`
   - **Step b (sidecar path)**: `sidecar_path = source_path.with_extension("xmp")` (Lightroom convention)
   - **Step c (build settings)**: clone builder; set `photohelper_id = row.photo_id().to_string()`
     (from DB — no `PhotoId::derive` disk read); set `nima_score = row.nima_score`;
     set `last_processed_at = now_utc`.
   - **Step d (call sidecar)**: `match merge_and_write(&sidecar_path, &settings, args.force)`:
     - `Ok(Created)` → `stats.written++`
     - `Ok(Overwritten)` → `stats.updated++`
     - `Ok(ConflictPreserved)` → `stats.conflict_preserved++`
     - `Ok(ForcedOverwrite)` → `stats.force_overwritten++`
     - `Err(e)` → `warn!(path, error); stats.errored++; continue` (per-photo, not fatal)
8. Heartbeat shutdown (stop + join).
9. Print summary line.

`DevelopStats` (AtomicU64 — needed for heartbeat-thread reads across threads):
- `walked` — photos walked from catalog
- `written` — new XMP sidecars created (`WriteOutcome::Created`)
- `updated` — existing sidecars overwritten (`WriteOutcome::Overwritten`)
- `conflict_preserved` — Lightroom/other tool is newer (`WriteOutcome::ConflictPreserved`)
- `force_overwritten` — `--force` unconditional overwrite (`WriteOutcome::ForcedOverwrite`)
- `file_missing` — `source_path` no longer exists on disk
- `errored` — `merge_and_write` returned `Err`

Summary line format:
```
walked: N, written: N, updated: N, conflict-preserved: N, force-overwritten: N, file-missing: N, errored: N
```

Heartbeat line:
```
[heartbeat] develop: walked N, written N
```

Exit codes:
- 0: success
- `EX_STRICT_FAIL` (75): `--strict` and `(file_missing + errored) > 0`
  (`conflict_preserved` and `force_overwritten` are NOT errors)

Wire `Command::Develop` in `main.rs` to `run_develop`.
**Also update `stub_subcommands_exit_69_with_not_yet_implemented_message` test** in `tests/cli.rs`
to remove `"develop"` from the stub list (analogous to how `"cull"` was removed in session 04).

#### D4 — Tests (≥ 9 integration tests in `tests/cli.rs`):

| Test | What it verifies |
|---|---|
| `develop_creates_xmp_sidecar_for_ingested_photo` | After ingest + develop, `photo.xmp` exists (not `photo.CR3.xmp`) |
| `develop_writes_nima_score_when_culled` | After cull + develop, `ph:NimaScore` in sidecar |
| `develop_cli_flags_written_to_sidecar` | `--temp 5500 --exposure 1.5` → crs:Temperature + crs:Exposure2012 in sidecar |
| `develop_idempotency_second_run_updates` | Second develop run → `updated=N, written=0` |
| `develop_empty_catalog_exits_zero` | Empty catalog → exit 0, `walked: 0` |
| `develop_strict_exits_nonzero_on_file_missing` | Ingest photo, delete it, run `--strict` → exit 75, `file-missing: 1` |
| `develop_force_overwrites_conflict` | `--force` on conflicting sidecar → `force-overwritten: 1` |
| `develop_conflict_preserved_appears_in_summary` | Existing sidecar with future MetadataDate → `conflict-preserved: 1` |
| `develop_summary_line_contains_expected_fields` | Summary line contains all counter names |

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
| D2e (TD-020 bicubic) | 0 new (tighten existing assertion) | cosine_sim ≥ 0.90 on CC0 fixtures (threshold tightened in-place) |
| D3 (sidecar) | 16 | round-trip, validation (5 boundaries), conflict (4 cases), atomic write, Lightroom path, reader lenient |
| D4a (catalog query) | 4 | DevelopRow SQL, superseded filter, NIMA join, wrong-model-slug returns None |
| D4b (develop CLI) | 9 | path convention, NIMA score, CLI flags in sidecar, idempotency, empty catalog, strict, force, conflict counter, summary |
| **Total** | **≥ 29** | |

**Target**: ≥ 211 tests (182 baseline + 29 minimum).

---

## Deliverable ordering (R1-L remediation)

**D0 → D2 → D1 → D3 → D4 → D5**

D2 (5 independent TD closures) is ordered before D1 (high-variance 8-agent review) to front-load
guaranteed progress. If D1 consumes the remaining context window, D0+D2 have already banked 7 TD closures.

---

## Checkpoints

| Checkpoint | When |
|---|---|
| Plan-review R1 | NOW (after this doc is committed) |
| Plan-review R2 | After R1 remediation |
| D2 batch | After D0; before D1 |
| D1 session-02 review R1+R2 | After D2; before D3 |
| Sub-component review D3 | After D3f complete; before D4 |
| Session-end review R1+R2 | After D5 complete |

---

## Scope justification

The user requested "all TDs + develop pipeline completely and runnable." This plan addresses:
- **7 TDs closed this session**: 4 with fired binding triggers (TD-009, TD-011, TD-014, TD-020)
  + 3 proactive closures before triggers fire (TD-001, TD-004, TD-005).
- **9 TDs deferred** (TD-002, TD-006, TD-007, TD-012, TD-013, TD-015, TD-017, TD-018, TD-019) —
  explicitly out of scope per CLAUDE.md policy ("Do not design for hypothetical future
  requirements. No half-finished implementations either.") Each deferred TD retains its binding
  trigger in TECH-DEBT.md.
- The full `develop` pipeline including `photohelper-sidecar` crate (Lightroom-compatible XMP
  sidecar I/O, atomic write, conflict resolution), `develop` subcommand (closes DN-004), and
  end-to-end tests.
