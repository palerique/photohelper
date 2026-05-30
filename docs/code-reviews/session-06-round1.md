# Session 06 — td-cleanup-develop-pipeline, Session-End Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent); agents pinned to opus"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
  gate_state: pass
  cache_used: true
```

```yaml
plugin_availability:
  schema_version: 1
  agents_requested:
    - general-purpose
    - feature-dev:code-architect
    - feature-dev:code-reviewer
    - pr-review-toolkit:type-design-analyzer
    - pr-review-toolkit:silent-failure-hunter
    - pr-review-toolkit:comment-analyzer
    - pr-review-toolkit:pr-test-analyzer
    - pr-review-toolkit:code-simplifier
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 3 |
| HIGH | 7 |
| MEDIUM | 2 |
| LOW | 2 |
| **Total** | **14** |

---

## Theme A — Conflict detection broken: compares our timestamps against our timestamps, not Lightroom edits [CRITICAL]

**Agents**: feature-dev:code-architect (primary), pr-review-toolkit:silent-failure-hunter

The conflict resolution spec (`docs/plans/session-06.md:108-113`) requires comparing
`xmp:MetadataDate_existing > ph:LastProcessedAt_existing` to detect "Lightroom edited after us."

The reader (`reader.rs:131-133`) collapses both into a single `last_processed_at` field
(`ph:LastProcessedAt` preferred, `xmp:MetadataDate` as fallback). `xmp:MetadataDate` is
discarded when `ph:LastProcessedAt` is also present.

`conflict.rs:67` then compares `existing.last_processed_at()` (= our old write time) against
`incoming.last_processed_at()` (= `now()`). This comparison is **always false** (our past write
time ≤ now), so the `else` branch always fires → `Overwritten` — silently destroying any
Lightroom edits made after our last develop run.

**Verified**: comparison at conflict.rs:67-68; collapse at reader.rs:131-133.

**Remediation**: Add `metadata_date: Option<OffsetDateTime>` to `ParsedFields`/`SidecarSettings`
(reader-only). Reader stores `xmp:MetadataDate` there. Conflict resolver compares
`existing.metadata_date() > existing.last_processed_at()` in the `(Some,Some)` branch.

---

## Theme B — `has_crs_fields()` too narrow: sidecars with untracked `crs:` attrs silently overwritten [CRITICAL]

**Agents**: pr-review-toolkit:silent-failure-hunter

`has_crs_fields()` (`settings.rs:123-130`) checks only the 6 numeric fields. The reader discards
all other `crs:` attributes (e.g. `crs:WhiteBalance`, `crs:CameraProfile`) via `_ => {}`.

The `(None, None)` branch in `conflict.rs:100-114` uses `existing.has_crs_fields()` as the
overwrite guard. A Lightroom sidecar with only `crs:WhiteBalance="Custom"` (no numeric adjustments)
returns `has_crs_fields() = false`, triggering `write_xmp` and silently overwriting Lightroom's
settings.

**Verified**: `has_crs_fields()` at settings.rs:123 checks only 6 fields.

**Remediation**: Track `has_any_crs_attr: bool` in `ParsedFields` (set `true` for any `"crs:"` key
in reader). Add `has_any_crs_attribute()` to `SidecarSettings`. Use it in the `(None, None)` branch.

---

## Theme C — Missing TD entry and in-source label for S1 stop-gap (quick-xml manual XMP) [CRITICAL]

**Agents**: feature-dev:code-reviewer

The plan (`session-06.md:64-68`) declares S1: "XMP write uses quick-xml manual template — TD filed
at D3 commit." TECH-DEBT.md has no TD entry for the quick-xml sidecar stop-gap. No in-source
`// TD-NNN:` label appears in `writer.rs::render_xmp`. CLAUDE.md § No Acceptable Trade-offs
Policy: stop-gap commits without companion TDs are CRITICAL.

**Verified**: No TD entry found in TECH-DEBT.md.

**Remediation**: File `TD-022` in TECH-DEBT.md. Add `// TD-022: quick-xml manual XMP template`
to `writer.rs::render_xmp`.

---

## Theme D — `all_photos_with_cull_scores` silently drops per-row catalog errors [HIGH]

**Agents**: general-purpose

`catalog.rs:869` uses `.flatten()` which silently discards `Err(rusqlite::Error)` from individual
row reads. The `.filter_map` at line 871 also silently drops rows with corrupt `id` blobs. Every
other catalog query method propagates per-row errors explicitly.

**Verified**: `.flatten()` at catalog.rs:869 confirmed.

**Remediation**: Replace with explicit `for r in rows { let (...) = r.map_err(...)? }` pattern
matching `all_embeddings_for_model`.

---

## Theme E — Superseded test does not actually supersede any row [HIGH]

**Agents**: general-purpose

`all_photos_with_cull_scores_superseded_excluded` (catalog.rs:1811) calls `cat.upsert(&p1, 1)`
with the same `Photo` object. Since `PhotoId` is identical, `upsert` returns `AlreadyCatalogued`
— no row is marked superseded. The assertion `rows.len() == 1` trivially passes. No actual
supersession filtering is verified.

**Verified**: `cat.upsert(&p1, 1)` at catalog.rs:1815 uses same Photo.

**Remediation**: Create a second Photo with different `id_seed` but same `source_path` as `p1`.
After upserting, `p1` is superseded. Verify `rows.len() == 1`.

---

## Theme G — 3 missing CLI integration tests [HIGH]

**Agents**: pr-review-toolkit:pr-test-analyzer

Plan specifies 9 CLI tests for `develop`; 6 delivered. Missing:
1. `develop_force_overwrites_conflict` — `--force` CLI flag wiring untested
2. `develop_conflict_preserved_appears_in_summary` — `conflict-preserved:` counter untested at CLI level
3. `develop_writes_nima_score_when_culled` — NIMA→sidecar path untested at CLI level

**Verified**: `develop_force_overwrites_conflict` absent from cli.rs.

**Remediation**: Add 3 tests. First two use synthetic CR3 fixtures + manual sidecar writes.
Third inserts a `cull_scores` row directly via SQL (avoiding LFS dependency).

---

## Theme H — `from_parsed` bypasses validation; out-of-range XMP values silently accepted [HIGH]

**Agents**: pr-review-toolkit:type-design-analyzer

`SidecarSettings::from_parsed` (`settings.rs:144`) constructs `SidecarSettings` directly from
`ParsedFields` without running any of the range checks in `SidecarSettingsBuilder::build()`.
A corrupt sidecar with `crs:Temperature="999999"` produces `temperature = Some(999999)` —
violating the `[2000, 50000]` invariant with no error or warning.

**Verified**: `from_parsed` at settings.rs:144 confirmed unchecked.

**Remediation**: Add lenient-clamp in `from_parsed`: out-of-range values → `None` + `tracing::warn!`.
Add `is_finite()` check for `nima_score`. Preserves "trust the file" philosophy while preventing
invariant violations.

---

## Theme I — Writer `unwrap_or_default()` could silently write empty `xmp:MetadataDate=""` [HIGH]

**Agents**: pr-review-toolkit:silent-failure-hunter

`writer.rs:67,106`: `dt.format(&Rfc3339).unwrap_or_default()`. If formatting fails, writes
`xmp:MetadataDate=""` — a permanent spurious warning on every subsequent read with no self-healing.

**Verified**: `unwrap_or_default()` at writer.rs:67 confirmed.

**Remediation**: Use `if let Ok(iso) = dt.format(&Rfc3339)` — skip the attribute entirely on
failure rather than writing an empty string. Apply to both line 67 and line 106.

---

## Theme J — Clock failure in `unix_now_as_datetime()` silently skips timestamp, no warning [HIGH]

**Agents**: pr-review-toolkit:silent-failure-hunter

`develop.rs:175`: when `unix_now_as_datetime()` returns `None`, `last_processed_at` is omitted
from the builder with no `tracing::warn!`. The resulting sidecar has no timestamps. On next run,
the photo enters `(None, Some(_))` → `ConflictPreserved` permanently — stuck with initial settings,
no operator visibility.

**Verified**: No warn! found at develop.rs:175 for `None` path.

**Remediation**: Add `tracing::warn!` once (before the loop) when `now_utc.is_none()`.

---

## Theme K — `run_develop` doc omits catalog-query error propagation [MEDIUM]

**Agents**: pr-review-toolkit:comment-analyzer

`develop.rs:101`: "Returns `Err` only for fatal setup failures (catalog open, heartbeat spawn)."
The function also propagates `?` from `all_photos_with_cull_scores()` at line 119.

**Remediation**: "Returns `Err` only for fatal setup failures (catalog open, photo query, heartbeat spawn)."

---

## Theme L — Orphaned `row_count` doc fragment prepended to `all_photos_with_cull_scores` [MEDIUM]

**Agents**: pr-review-toolkit:comment-analyzer, general-purpose

`catalog.rs:834`: `/// Total rows in \`photos\` (visible to driver for summary tally).` is the
`row_count()` docstring accidentally left before `all_photos_with_cull_scores`. `row_count()` at
line 885 lacks its doc.

**Remediation**: Move the fragment to `row_count()`.

---

## Themes M, N [LOW]

- **M**: `dedup_cluster_id` in SidecarSettings/builder is dead in production (develop never sets
  it — no dup_clusters JOIN). Note in a discovery-note for future session.
- **N**: `render_xmp` formats `dt.format(&Rfc3339)` twice (lines 67, 106); cache once.
  `develop.rs:132-137` has 6 unnecessary `let cli_*` variable copies; reference `args.*` directly.

---

## Disposition summary

| Theme | Severity | Status |
|---|---|---|
| A — Conflict detection broken | CRITICAL | Open |
| B — has_crs_fields() too narrow | CRITICAL | Open |
| C — Missing TD for S1 stop-gap | CRITICAL | Open |
| D — .flatten() drops errors | HIGH | Open |
| E — Superseded test wrong | HIGH | Open |
| F — TD-001 Open (discarded) | — | Discarded |
| G — 3 missing CLI tests | HIGH | Open |
| H — from_parsed bypasses validation | HIGH | Open |
| I — Writer empty timestamp | HIGH | Open |
| J — Clock failure silent | HIGH | Open |
| K — run_develop doc incomplete | MEDIUM | Open |
| L — Orphaned doc fragment | MEDIUM | Open |
| M — dedup_cluster_id dead | LOW | Open |
| N — render_xmp double-format | LOW | Open |

## R2 watch-list

- [ ] R2-A: Conflict uses `existing.metadata_date() > existing.last_processed_at()`
- [ ] R2-B: `has_any_crs_attribute()` tracks all `crs:` attrs; used in `(None,None)` branch
- [ ] R2-C: TD-022 in TECH-DEBT.md; in-source label in writer.rs
- [ ] R2-D: `.flatten()` replaced with explicit error propagation in all_photos_with_cull_scores
- [ ] R2-E: Superseded test uses different Photo at same source_path
- [ ] R2-G: 3 missing tests: force, conflict-preserved, nima-score
- [ ] R2-H: `from_parsed` clamps out-of-range values to None + warn
- [ ] R2-I: `unwrap_or_default()` replaced with match+skip
- [ ] R2-J: `tracing::warn!` added for clock failure

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 13
  verified: 9
  drifted: 3
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discarded_by_9th_agent: 1
  discard_rate: 0.077
```
