# Decision 0002 — Catalog schema v2

**Status**: Accepted (session 04, 2026-05-29).
**Owner**: session 04 (`ai-culling-pipeline`).
**Authoritative for**: `crates/photohelper-catalog/src/schema.rs`
(`SCHEMA_VERSION`, `MIGRATE_V1_TO_V2_SQL`).

## Context

Session 04 adds AI culling (`cull` subcommand). Scores produced by the
NIMA model must be persisted so subsequent `cull` runs skip already-scored
photos and so scores are queryable for triage and export. The v1 schema
has no such table.

Decision-doc 0001 §Migration policy described a `Vec<dyn Migration>` runner
as the future migration mechanism. This session supersedes that text: one
migration does not warrant a trait (see §Migration-runner rationale below).

## Decision

Bump `SCHEMA_VERSION` from 1 to 2 and add the `cull_scores` table via a
`MIGRATE_V1_TO_V2_SQL` constant applied in a `BEGIN IMMEDIATE` transaction.

```sql
CREATE TABLE IF NOT EXISTS cull_scores (
    photo_id                BLOB    NOT NULL REFERENCES photos(id),
    model_slug              TEXT    NOT NULL,
    aesthetic_score         REAL    NOT NULL,
    scored_at_unix_seconds  INTEGER NOT NULL,
    PRIMARY KEY (photo_id, model_slug)
);
CREATE INDEX IF NOT EXISTS idx_cull_scores_photo ON cull_scores(photo_id);
PRAGMA user_version = 2;
```

`PRAGMA foreign_keys = ON` is added to the per-connection PRAGMA block so
the `REFERENCES photos(id)` constraint is actually enforced at INSERT time.

## Column rationale

| Column | Type | Rationale |
|---|---|---|
| `photo_id` | `BLOB NOT NULL` | FK back to `photos.id` (32-byte `PhotoId`). No `ON DELETE CASCADE` — no delete path exists in v0.1 (see §FK design). |
| `model_slug` | `TEXT NOT NULL` | Identifier tying the row to a specific model binary (e.g., `"nima-aesthetic-v1"`). Composite PK with `photo_id` so the same photo can be scored by multiple models in the future. |
| `aesthetic_score` | `REAL NOT NULL` | NIMA aesthetic score in `[1, 10]`. Stored as SQLite REAL (64-bit float); no precision loss for f32 values. |
| `scored_at_unix_seconds` | `INTEGER NOT NULL` | Unix epoch time of the cull run. Enables per-run audit queries. |

## FK design

`photo_id REFERENCES photos(id)` without `ON DELETE CASCADE`:

- There is no delete path in v0.1. Deleting a photo from `photos` while
  its `cull_scores` rows remain would require a manual cascade or cleanup
  pass; since the operation does not exist yet, the simpler constraint
  (reject orphaned inserts, tolerate orphaned rows on hypothetical delete)
  is preferred.
- If a delete path is added post-v0.1, a v3 migration can add `ON DELETE
  CASCADE` or issue a cleanup `DELETE FROM cull_scores WHERE photo_id NOT
  IN (SELECT id FROM photos)`. See `docs/discovery-notes.md § DN-023`.

## Migration-runner rationale

Decision-doc 0001 §Migration policy proposed a `Vec<dyn Migration>` trait
for running migrations in sequence. **This session supersedes that text.**

A trait runner is appropriate when there are ≥3 migrations and the runner
can be tested orthogonally of any specific schema. With a single migration
(v1 → v2), a simple `match schema_version` arm plus `apply_v1_to_v2(conn)`
is:

- Easier to read — the entire migration logic is one function, one SQL
  constant, no polymorphism.
- Fewer moving parts — no trait object, no registration, no version
  ordering logic to get wrong.
- Sufficient — `CREATE TABLE IF NOT EXISTS` idempotency covers the crash-
  recovery case a migration runner would otherwise handle.

If v3+ migrations are added, revisit this decision and extract a runner at
that time.

## Migration path

| `user_version` at open | Action |
|---|---|
| `0` | Run `INIT_SQL` (v1 tables) then `MIGRATE_V1_TO_V2_SQL` |
| `1` | Run `MIGRATE_V1_TO_V2_SQL` only |
| `2` | No-op (already current) |
| `≥ 3` | Reject with `Error::CatalogSchemaTooNew` |

## Amendment to decision-doc 0001

Decision-doc 0001 §Migration policy is hereby superseded. The
`Vec<dyn Migration>` runner described there is replaced by the
match-arm approach documented here. All other sections of 0001 remain
authoritative.
