# Decision 0001 — Catalog schema v1

**Status**: Accepted (session 01, 2026-05-28); v1 → v2 migration ownership
amended (session 02, 2026-05-28); §Migration policy superseded (session 04,
2026-05-29 — see § Amendments).
**Owners**: session 01 (v1 minimal schema); session 04 (`ai-culling-pipeline`,
2026-05-29 — v1 → v2 migration via match-arm approach; dup-group deferred).
**Authoritative for**: `crates/photohelper-catalog/src/schema.rs` (v1 DDL
only; v2 DDL + migration policy: `docs/decisions/0002-catalog-schema-v2.md`).

## Context

DN-005 (`docs/discovery-notes.md`) named session 01 as the owner of the
catalog schema v1 slice. Plan v5 §Deliverables 4 + §Deliverables 8
committed this decision doc as the authoritative reference for the
schema text + index rationale + the design choices that survived four
plan-review rounds and one round of session-end review.

## Decision

Ship the schema below as `PRAGMA user_version = 1`. Wrap the init in
`BEGIN IMMEDIATE; ...; COMMIT;` so partial init (process killed between
`CREATE TABLE` and `PRAGMA user_version = 1`) cannot leave the database
in an ambiguous half-initialized state.

```sql
CREATE TABLE IF NOT EXISTS photos (
    id BLOB PRIMARY KEY,
    source_path TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    mtime_unix_seconds INTEGER NOT NULL,
    mtime_anomalous INTEGER NOT NULL DEFAULT 0,
    make TEXT,
    model TEXT,
    camera_slug TEXT,
    capture_time_unix_seconds INTEGER,
    width INTEGER,
    height INTEGER,
    exif_orientation INTEGER,
    ingested_at_unix_seconds INTEGER NOT NULL,
    superseded_at_unix_seconds INTEGER
);
CREATE INDEX IF NOT EXISTS idx_photos_source_path ON photos(source_path);
CREATE INDEX IF NOT EXISTS idx_photos_camera_slug ON photos(camera_slug);
PRAGMA user_version = 1;
```

## Design rationale

### Primary key on `id` (content-derived BLOB)
`PhotoId` is 32 bytes derived from `BLAKE3(file_size_le ||
clamped_mtime_le || first_64KB || last_64KB)`. Using it as the PK gives:

- O(1) lookup for hardlink dedup (INSERT OR IGNORE on same content).
- Stable identity across path moves — same bytes + same clamped mtime
  = same row, regardless of where the file lives.
- Cross-tool catalogs eventually mergeable (sessions 06+) by id.

### `source_path TEXT NOT NULL` with NON-unique index
**Not** `UNIQUE`. Two rows with the same `source_path` and different
`id` are valid — that's the supersede case (file content changed while
the path stayed). The new row inserts; the old row's
`superseded_at_unix_seconds` gets set. Both rows are retained as an
audit trail.

The index supports the supersede-lookup path (`SELECT id FROM photos
WHERE source_path = ?1 AND superseded_at_unix_seconds IS NULL`) which
runs in `Catalog::upsert` for every photo.

### `mtime_unix_seconds INTEGER NOT NULL` + `mtime_anomalous INTEGER NOT NULL DEFAULT 0`
- The stored mtime is the CLAMPED value
  (`[1995-01-01, 2100-01-01]` per
  `photohelper-core::model::clamp_mtime`), matching what fed the
  PhotoId derivation. Reading either column gives identical mtime
  bytes — no drift between the hash and the stored value.
- `mtime_anomalous` is `1` iff the original filesystem mtime was
  outside the allowed range. Stored as a flag rather than computed at
  query time so `SELECT ... WHERE mtime_anomalous = 1` is index-free
  but constant-time (no `CASE WHEN` per row), and so the
  `--strict` policy in `cli::commands::ingest` can read it directly
  without re-deriving from the clamped value.

### `camera_slug TEXT` — NULL iff unknown
Single source of truth for "is this a recognized camera?":
`camera_slug IS NOT NULL`. Previous drafts had a redundant
`camera_known INTEGER` BOOLEAN flag — dropped at R1.T6 because two
columns encoding one bit invites silent divergence (one set, the other
not). The `camera_slug` value is the slug from
`KnownCamera::slug()` (e.g. `'canon-r8'`). Indexed for the v0.5+
"filter by camera body" query path.

### `make TEXT`, `model TEXT` — raw EXIF, populated when EXIF parses
Preserved alongside `camera_slug` so unknown bodies still carry useful
identity. A future session that adds Sony / Nikon / Fuji profiles can
re-run the `CameraRegistry` over the existing make/model values to
populate `camera_slug` retroactively without re-walking the filesystem.

### `capture_time_unix_seconds INTEGER`, `width`/`height INTEGER`, `exif_orientation INTEGER`
All NULLable. Set when EXIF parses; NULL under the DN-006 fallback
(kamadak-exif can't parse CR3 ISO-BMFF synthetics). `exif_orientation`
stores the raw EXIF tag value 1..=8; `ExifOrientation::from_tag` /
`to_tag` round-trip at the type boundary.

### `ingested_at_unix_seconds INTEGER NOT NULL`, `superseded_at_unix_seconds INTEGER`
Audit trail. `superseded_at` NULL on the current row; set on the
previous row when content at the same `source_path` changes.

## What's deliberately NOT in v1

- **Cull score columns** (`quality`, `blur`, `eye_state`, etc.) —
  session 03+ when `cull` lands. Per DN-005 the migration framework
  (currently absent — single-table v1 doesn't need it) materializes
  alongside that schema change.
- **Dup-group table** — session 03+ alongside MobileCLIP embedding
  storage.
- **XMP sidecar pointer** (`xmp_sidecar_path`) — session 04+.
- **Develop-settings JSON / column-per-setting** — session 04+.
- **Foreign keys** — v1 is single-table; FK semantics + cascade
  rules get designed when the second table arrives.
- **`CHECK` constraints** — keep schema simple; invariants are
  enforced in Rust (`PhotoRow::from_row` typed conversions).

## Migration policy

> **SUPERSEDED by `docs/decisions/0002-catalog-schema-v2.md` §
> Migration-runner rationale (session 04, 2026-05-29).** The
> `Vec<dyn Migration>` trait runner described in the original text below
> was NOT adopted. The v1 → v2 migration uses a simple `match` arm +
> `apply_v1_to_v2(conn)` function (no trait). See decision-doc 0002 for
> the full rationale and the migration table.

Original text (preserved for audit trail):

v1 stays at `PRAGMA user_version = 1` forever. The next change
(v1 → v2 in **session 03**, rescheduled from session 02 per
§ Amendments) introduces the migration FRAMEWORK simultaneously with
adding tables, because:

- A single-statement migration doesn't justify framework overhead.
- Two-step migrations (add column → migrate data → drop old column)
  benefit from per-step idempotency tracking, which IS what a
  framework gives you.
- The framework lives in `photohelper-catalog::migrations` as a
  `Vec<&'static dyn Migration>` and a per-version applier; **session 03**
  adds it + adds migration `v1 → v2` alongside the cull-score + dup-group
  tables (DN-005).

Session 02 (`libraw-cr3-decode`) does NOT touch the schema shape — it
populates existing-NULL CR3 columns (`make`/`model`/`capture_time_unix_seconds`/`width`/`height`/`exif_orientation`)
with real LibRaw-extracted values. No new columns; no
`PRAGMA user_version` bump; no migration framework needed. The
NULL-population path is DML, not DDL.

## Trigger to revisit

- DN-005 closure: this slice is closed; **session 03** reopens for the
  cull/dup-group additions and the v1 → v2 migration framework.
- If real Canon R8 fixtures (session 02) surface new EXIF fields we
  want to catalog (e.g. lens make/model, ISO, shutter speed), file a
  DN-NNN and add columns under the session-03 migration.

## Amendments

### 2026-05-28 (session 02) — v1 → v2 migration framework rescheduled from session 02 to session 03

Rationale: session 02 (`libraw-cr3-decode`) ships LibRaw FFI for Canon R8
CR3 — EXIF read (the DN-011 critical-path remediation: kamadak-exif
fails 370/370 real CR3s) plus RAW pixel decode plus the TD-002 rusqlite
0.32 → 0.40 bump. The migration-framework + cull-score + dup-group
table work belongs to the `cull` subcommand pipeline (session 03), not
the RAW-pipeline session. Bundling it with LibRaw would double session
02's scope without architectural payoff (the LibRaw FFI surface is
orthogonal to schema migration).

Surfaced by `docs/code-reviews/session-02-plan-round1.md § PR1-T8` (4
agents converged: plan defers migration framework to session 03 while
this decision doc committed session 02 — internal contradiction). Plan
v2 + this amendment land in lockstep.

Session 02's schema interaction is limited to: (a) populating
previously-NULL columns for CR3 rows via `Catalog::upsert` (no SQL
changes), (b) the TD-002 rusqlite version bump (no SQL changes).
Session 03's first plan commit MUST include "migration framework v1 →
v2" as a §Deliverables item; if it doesn't, the session-03 plan-review
must reject. (Identical binding-trigger discipline to DN-011's
session-02 LibRaw EXIF requirement.)

### 2026-05-29 (session 04) — §Migration policy superseded; v1→v2 shipped without trait runner

Session 04 (`ai-culling-pipeline`) shipped the v1 → v2 migration using a
`match` arm + `apply_v1_to_v2(conn)` function — no `Vec<dyn Migration>`
trait runner. Rationale and the definitive migration table are in
`docs/decisions/0002-catalog-schema-v2.md`. §Migration policy above is
superseded; all other sections of this document remain authoritative.
