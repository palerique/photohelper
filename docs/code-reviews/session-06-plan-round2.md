# Session 06 — Plan Review Round 2

```yaml
session_config:
  schema_version: 1
  model_claimed: "Sonnet 4.6 [1m] (parent); verification agent pinned to opus"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
  gate_state: pass
  cache_used: true
```

```yaml
plugin_availability:
  schema_version: 1
  agents_requested: [general-purpose]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## Triage summary

| Severity | Count |
|---|---|
| CRITICAL | 0 |
| HIGH | 0 |
| MEDIUM | 1 |
| LOW | 0 |
| **Total** | **1** |

---

## R1 Watch-list Verification — All 10 CLOSED

**R1-A** — CLOSED. `raw_path.with_extension("xmp")` replaces the extension (lines 92-95).
Test `sidecar_path_for_cr3_replaces_extension` asserts `photo.CR3 → photo.xmp`.

**R1-B** — CLOSED. Atomic write specified (lines 350-357): write to `<path>.phdev.tmp`,
`fsync`, `fs::rename`, cleanup on error.

**R1-C** — CLOSED. Conflict resolution table (lines 108-113) covers all 4
timestamp-presence cases with defined outcomes.

**R1-D** — CLOSED. `MODEL_SLUG` (`"nima-aesthetic-v1"`) used throughout (lines 145-147,
490, 533-534); "NOT `MODEL_MANIFEST_NAME`" callouts explicit.

**R1-E** — CLOSED. SQL `SELECT p.id, p.source_path, cs.aesthetic_score` (lines 150-151).
`DevelopRow` includes `photo_id: PhotoId` (lines 158-159).

**R1-F** — CLOSED. `SidecarSettings` uses private fields + `SidecarSettingsBuilder`
(lines 301-333). "Validation runs at construction time; callers cannot construct invalid
settings."

**R1-G** — CLOSED. `WriteOutcome` has 4 variants mapping 1:1 to stats counters (lines
402-411): `Created` → `written`, `Overwritten` → `updated`, `ConflictPreserved` →
`conflict_preserved`, `ForcedOverwrite` → `force_overwritten`.

**R1-H** — CLOSED. `DevelopStats` has `file_missing` + `errored` (lines 555-562).
D4b step 7a: file-missing pre-check + `stats.file_missing++; continue`. Step 7d Err arm:
`warn!; stats.errored++; continue`.

**R1-I** — CLOSED. `mktemp /tmp/ph-sanitize-XXXXXX.jpg` in D2a (line 236). Rationale for
`mktemp` vs hardcoded path explained (lines 244-245).

**R1-J** — CLOSED. Lenient reader specified (lines 387-393): malformed known-field values →
`tracing::warn!` + treat as `None`. Read succeeds with partial data.

---

## New findings from R2 regression scan

### Theme R2-A — Vestigial `derive_failed` in summary section [MEDIUM — remediated inline]

Line 38 said "file_missing + `derive_failed` counters" but the implementation spec uses
`file_missing` + `errored`. The `derive_failed` name was from the pre-R1 design where
`PhotoId::derive` was re-called from disk. After R1-E (`DevelopRow` now carries `photo_id`
from the catalog), no derive step occurs in the develop loop, making `derive_failed`
meaningless. **Remediated immediately** by changing line 38 to "file_missing + `errored`."

All other R2 regression checks CLEAN:
- `force_overwritten` excluded from `--strict` predicate (intentional — user action, not error)
- Stub test update note present (D4b: remove `"develop"` from stub list)
- SQL `SELECT p.id` correct
- Test count math: 0+16+4+9=29; 182+29=211 ✓
- `bilinear_resize` demotion to `fn` correctly specified in D2e
- No phantom constants or type names
- `process_version` hardcoded in writer (not a user field) — no test regression
- `all_photos_wrong_model_slug_returns_none_score` test listed in D4a

---

## Disposition summary

| Theme | R1 severity | R2 status |
|---|---|---|
| A — XMP path convention | CRITICAL | CLOSED |
| B — Atomic XMP write | CRITICAL | CLOSED |
| C — Conflict resolution missing timestamps | CRITICAL | CLOSED |
| D — Phantom constant MODEL_MANIFEST_NAME_NIMA | HIGH | CLOSED |
| E — DevelopRow missing photo_id | HIGH | CLOSED |
| F — SidecarSettings public fields / Custom WB | HIGH | CLOSED |
| G — WriteOutcome↔DevelopStats mapping | HIGH | CLOSED |
| H — Per-photo error handling gaps | HIGH | CLOSED |
| I — /tmp/preview.jpg clobber | HIGH | CLOSED |
| J — XMP reader malformed value unspecified | HIGH | CLOSED |
| K — Critical test gaps | HIGH | CLOSED |
| L — D1 ordering | HIGH | CLOSED |
| M — D2 commit bundling | MEDIUM | CLOSED |
| N — Unused photohelper-core dep | MEDIUM | CLOSED |
| O — Dead SidecarSettings fields | MEDIUM | CLOSED |
| P — Trigger count errors | MEDIUM | CLOSED |
| Q — bilinear_resize visibility | MEDIUM | CLOSED |
| R — Wrong threshold in design section | MEDIUM | CLOSED |
| R2-A — derive_failed vestigial (R2 new) | MEDIUM | CLOSED (remediated inline) |

**0 CRITICAL, 0 HIGH remaining. Plan v2 is CLEAN.**

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 10
  verified: 10
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: R1-A
      present: yes
      retain: yes
      evidence_snippet: "raw_path.with_extension(\"xmp\") — the RAW extension is replaced, not appended"
    - finding_id: R1-B
      present: yes
      retain: yes
      evidence_snippet: "Write to `<path>.phdev.tmp` in the same directory"
    - finding_id: R1-C
      present: yes
      retain: yes
      evidence_snippet: "| `Some(_)` | `None` | ConflictPreserved (first photohelper run)"
    - finding_id: R1-D
      present: yes
      retain: yes
      evidence_snippet: "photohelper_ai::MODEL_SLUG (= \"nima-aesthetic-v1\") — NOT MODEL_MANIFEST_NAME"
    - finding_id: R1-E
      present: yes
      retain: yes
      evidence_snippet: "SELECT p.id, p.source_path, cs.aesthetic_score"
    - finding_id: R1-F
      present: yes
      retain: yes
      evidence_snippet: "SidecarSettings uses private fields + builder (consistent with Photo, ImageEmbedding)"
    - finding_id: R1-G
      present: yes
      retain: yes
      evidence_snippet: "pub enum WriteOutcome { Created, Overwritten, ConflictPreserved, ForcedOverwrite }"
    - finding_id: R1-H
      present: yes
      retain: yes
      evidence_snippet: "file_missing — source_path no longer exists on disk"
    - finding_id: R1-I
      present: yes
      retain: yes
      evidence_snippet: "preview_tmp=$(mktemp /tmp/ph-sanitize-XXXXXX.jpg)"
    - finding_id: R1-J
      present: yes
      retain: yes
      evidence_snippet: "Known fields with malformed values ... log tracing::warn! ... treat as None"
```
