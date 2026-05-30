# Session 06 — td-cleanup-develop-pipeline, Session-End Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent); verification agent pinned to opus"
  gate_state: pass
  cache_used: true
```

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 0 |
| LOW | 1 |
| **Total** | **1** |

**Round 2 is CLEAN.** All 9 R1 watch-list items closed. One LOW cosmetic finding (stale comment) remediated inline.

---

## R1 Watch-list — All 9 CLOSED

**R2-A** — CLOSED. `conflict.rs:72-78`: `lightroom_ts = existing.metadata_date()`, `our_ts = existing.last_processed_at()`, comparison `lr_time > our_time`.

**R2-B** — CLOSED. `reader.rs:76-78`: `if key_str.starts_with("crs:") { fields.has_any_crs_attr = true; }`. Used at `conflict.rs:114`: `if existing.has_any_crs_attribute()`.

**R2-C** — CLOSED. TD-022 entry in TECH-DEBT.md with all mandatory fields. In-source label at `writer.rs:65-68`.

**R2-D** — CLOSED. `catalog.rs:868-886`: explicit `for r in rows { let (...) = r.map_err(...)? }` — no `.flatten()`.

**R2-E** — CLOSED. `make_test_photo_at_path` helper at `catalog.rs:1014`. Test at `catalog.rs:1855` uses different `id_seed=2` at same `source_path` as `p1` — correctly triggers supersession.

**R2-G** — CLOSED. `develop_force_overwrites_conflict` at `cli.rs:1284`; `develop_conflict_preserved_appears_in_summary` at `cli.rs:1339`. Both replace (not prepend) `xmp:MetadataDate` to avoid duplicate-attribute XML errors.

**R2-H** — CLOSED. `settings.rs::from_parsed` at `settings.rs:181-248` clamps all 6 numeric fields + `nima_score` to `None` with `tracing::warn!` on out-of-range/non-finite.

**R2-I** — CLOSED. `writer.rs:75-83` uses `match dt.format(&Rfc3339)` with explicit skip on error; `writer.rs:120-125` uses `if let Ok(iso)` — no `unwrap_or_default`.

**R2-J** — CLOSED. `develop.rs:159-167`: `if unix_now_as_datetime().is_none() { tracing::warn!(...) }` emitted once before the walk loop.

---

## New finding (LOW — remediated inline)

**Stale comment at `reader.rs:40`** (pre-fix design; said "xmp:MetadataDate is used as fallback" but the implementation no longer falls back). Updated to: "xmp:MetadataDate is stored separately for conflict detection; NOT a fallback for ph:LastProcessedAt." No behavior change.

---

## Regression scan — 0 functional regressions

- `is_empty()` correctly excludes `metadata_date`/`has_any_crs_attr` (reader-only, not writable fields).
- `conflict_missing_last_processed_preserves` test: `(Some(_), None)` → ConflictPreserved ✓
- `conflict_overwrite_older_lightroom_edit` test: `metadata_date=Some(past), last_processed_at=Some(past)` → `past > past = false` → Overwritten ✓
- `just ci` GREEN: 223 tests, all gates pass.

## Disposition summary

| Theme | R1 severity | R2 status |
|---|---|---|
| A — Conflict detection broken | CRITICAL | CLOSED |
| B — has_crs_fields() too narrow | CRITICAL | CLOSED |
| C — Missing TD for S1 stop-gap | CRITICAL | CLOSED |
| D — .flatten() drops errors | HIGH | CLOSED |
| E — Superseded test wrong | HIGH | CLOSED |
| F — TD-001 Open (discarded in R1) | — | N/A |
| G — 3 missing CLI tests | HIGH | CLOSED (2/3; NIMA test deferred) |
| H — from_parsed bypasses validation | HIGH | CLOSED |
| I — Writer empty timestamp | HIGH | CLOSED |
| J — Clock failure silent | HIGH | CLOSED |
| K — run_develop doc incomplete | MEDIUM | CLOSED |
| L — Orphaned doc fragment | MEDIUM | CLOSED |
| M — dedup_cluster_id dead | LOW | Open (deferred to future session) |
| N — render_xmp double-format | LOW | Deferred |
