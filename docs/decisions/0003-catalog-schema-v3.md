# Decision 0003 — Catalog schema v3: embeddings + dup\_clusters

> Session: 05 (`dedup-mobileclip`) — 2026-05-29

---

## Context

Session 05 adds the duplicate-detection pipeline. Two new tables are required:

- **`embeddings`**: stores per-photo CLIP image embeddings (float32 BLOB) for
  similarity-based dedup. One row per `(photo_id, model_slug)`.
- **`dup_clusters`**: stores cluster assignments produced by the `dedup` subcommand.
  One row per `(photo_id, model_slug)`, referencing the corresponding `embeddings` row.

---

## Schema additions (MIGRATE_V2_TO_V3_SQL)

```sql
CREATE TABLE IF NOT EXISTS embeddings (
    photo_id                    BLOB    NOT NULL REFERENCES photos(id),
    model_slug                  TEXT    NOT NULL,
    dim                         INTEGER NOT NULL CHECK(dim > 0 AND dim <= 65536),
    quantization                TEXT    NOT NULL DEFAULT 'f32',
    embedding                   BLOB    NOT NULL,
    embedded_at_unix_seconds    INTEGER NOT NULL,
    PRIMARY KEY (photo_id, model_slug)
);

CREATE TABLE IF NOT EXISTS dup_clusters (
    photo_id                    BLOB    NOT NULL,
    model_slug                  TEXT    NOT NULL,
    cluster_id                  INTEGER NOT NULL CHECK(cluster_id >= 0),
    similarity_threshold        REAL    NOT NULL,
    clustered_at_unix_seconds   INTEGER NOT NULL,
    PRIMARY KEY (photo_id, model_slug),
    FOREIGN KEY (photo_id, model_slug) REFERENCES embeddings(photo_id, model_slug)
);
```

---

## Key design decisions

### 1. `embeddings` PRIMARY KEY is `(photo_id, model_slug)`

Supports multiple embedding models per photo in future sessions. One row per photo per model.

### 2. `embeddings.dim` column

Stored explicitly so callers don't need to deserialize the BLOB to get the dimension.
`CHECK(dim > 0 AND dim <= 65536)` enforces the dimension is in a sensible range.

### 3. `embeddings.quantization` column

`DEFAULT 'f32'` — v0.1 only stores raw float32 LE bytes. When int8/f16 quantization
is added, the `quantization` column lets callers deserialize correctly.
Stop-gap: **TD-018** covers the f32-only quantization.

### 4. `dup_clusters.FOREIGN KEY` references `embeddings(photo_id, model_slug)`

A cluster assignment can only reference a photo that has actually been embedded.
This is enforced at INSERT time when `PRAGMA foreign_keys = ON` is set (already
done in `Catalog::open`).

### 5. `dup_clusters.similarity_threshold REAL` stored per-row

Stop-gap (TD-019): v0.1 has no `dedup_runs` table. The threshold is stored per-row
as provenance ("what threshold produced this cluster?"). When TD-019 lands (adding
a `dedup_runs` table), the threshold will move there. Per-row storage is redundant
(all rows from one dedup run share the same threshold) but adds no correctness risk.

### 6. `cluster_id INTEGER CHECK(cluster_id >= 0)`

Non-negative integers; stored as SQLite `INTEGER` (i64). The Rust API uses `i64`
to match SQLite's native type (avoid the u64→i64 truncation risk noted in plan R1).

### 7. ON DELETE CASCADE absent

No `photos` delete path exists in v0.1. See **DN-023** for the open FK behavior
decision. The absent CASCADE means an attempt to DELETE a photos row with existing
embeddings will fail with a FK violation — a safe default for v0.1.

### 8. Migration chain

The v1→v2 migration (`apply_v1_to_v2`) and v2→v3 migration (`apply_v2_to_v3`)
are chained in `Catalog::open`:
- Fresh DB (user_version = 0): INIT_SQL → v1→v2 → v2→v3 (reaches v3 in one open)
- v1 DB: v1→v2 → v2→v3
- v2 DB: v2→v3 only
- v3 DB: no migration
- v > 3: `Error::CatalogSchemaTooNew`

---

## Stop-gaps

| Stop-gap | TD | Binding trigger |
|---|---|---|
| f32-only BLOB quantization | TD-018 | first user request for int8/f16 quantization |
| No per-dedup-run audit trail | TD-019 | first user report "I ran dedup twice, what changed?" or before v0.3 |
| ON DELETE CASCADE absent | DN-023 | first session adding a `photos` delete path |
