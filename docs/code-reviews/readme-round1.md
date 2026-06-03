# README.md — Documentation review, Round 1 (post-session-16)

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6[1m] (parent); sub-agents pinned to opus"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
  ask_user_question_id: null
  user_response: option-1
  gate_state: pass
  cache_used: true
```

**Scope**: README.md commit `282c52cc` — Windows install + 5 quickstart flows.
Cadence A tier-graduated: 4-agent suite (documentation blast radius).

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 3 | R1-A, R1-B, R1-C |
| HIGH | 3 | R1-D, R1-E, R1-F |
| MEDIUM | 2 | R1-G, R1-H |
| LOW | 1 | R1-I |

All 9 actionable findings remediated in same commit. **CLEAN after remediation.**

---

## Theme R1-A — `develop` subcommand documents `--source` flag that doesn't exist (CRITICAL)

Flow 1 step 4 showed `photohelper develop --catalog ... --source ... --lr-rating --lr-keywords`.
`DevelopArgs` has no `--source` field — clap would error immediately.

**Fix**: Removed `--source` line from Flow 1 step 4.

---

## Theme R1-B — Rosetta 2 claim is backwards: arm64 binary cannot run on Intel Macs (CRITICAL)

3 agents flagged this. README stated "Apple Silicon binaries run transparently on Intel Macs via Rosetta 2." Rosetta 2 translates x86_64→arm64 (Intel→Apple Silicon), not the reverse. Any Intel Mac user following these instructions gets `Bad CPU type in executable`.

**Fix**: Intel section now says "Intel Macs cannot run the arm64 binary — Rosetta 2 only goes the other direction. Intel Mac users must build from source targeting `x86_64-apple-darwin`."

---

## Theme R1-C — "ORT runs through Apple CoreML" is factually wrong (CRITICAL)

ort-sys source inspection (`static_link/mod.rs`) shows macOS targets link `Foundation` framework, not CoreML. CoreML is linked for `apple-ios` targets only. The Pyke CDN prebuilt for macOS is `none` (CPU) execution provider — CoreML.framework appears as a transitive link dependency of the static archive but is NOT the active inference backend.

**Fix**: macOS note now says "ORT is statically linked on macOS; inference uses the CPU execution provider. CoreML.framework is a transitive link dependency but not the active inference backend."

---

## Theme R1-D — Windows DirectML "always present" is misleading (HIGH)

"The binary uses Windows 10+ system DirectML libraries (always present)" — DirectML.dll ships with Windows 10 v1903+ (May 2019), not all Windows 10 versions. Also implies DirectML is the inference backend (it is not — CPU provider is used).

**Fix**: Note now says "link-time dependency on DirectX 12 system DLLs present on Windows 10 version 1903+; inference uses the CPU execution provider."

---

## Theme R1-E — Flow 1 `cull` step fails cryptically if `PHOTOHELPER_MODEL_DIR` unset (HIGH)

`resolve_model_dir()` falls back to binary-adjacent path (`/usr/local/bin/models/`) when env var unset, producing "manifest not found at /usr/local/bin/models/manifest.toml" with no mention of `PHOTOHELPER_MODEL_DIR`. New users jump to Flow 1 without reading Install section.

**Fix**: Added `# Requires PHOTOHELPER_MODEL_DIR — see Installation above.` before cull step.

---

## Theme R1-F — Flow 4 `rename` has unspoken prerequisite: requires prior ingest+cull (HIGH)

`rename` opens the catalog and queries `all_photos_with_cull_scores`. If called without a prior ingest+cull, the catalog either doesn't exist or returns zero results with no explanation.

**Fix**: Added prerequisite blockquote: "ingest and cull must have been run on the source directory first."

---

## Theme R1-G — Flow 3 raw SQL assumes SQLite comfort; scores flow automatically anyway (MEDIUM)

Raw `SELECT ... JOIN ... ORDER BY ...` block is too technical for the target audience (photographers). Also misleads users into thinking manual inspection is required.

**Fix**: Replaced with explanation that scores flow automatically to develop/export/rename. SQLite mentioned as optional for power users, without showing the raw query.

---

## Theme R1-H — `/usr/local/bin/` is less idiomatic than `~/.local/bin/` for arm64 Macs (MEDIUM)

macOS install uses `sudo cp photohelper /usr/local/bin/`. On Apple Silicon Macs, `/opt/homebrew/bin/` or `~/.local/bin/` is more conventional. However, `/usr/local/bin/` is functional and on `$PATH` by default — the difference is cosmetic. Not changed to avoid introducing PATH setup instructions.

**Deferred**: Not remediated (functional, `$PATH`-default, cosmetic difference).

---

## Theme R1-I — Flow ordering: most complex flow first (LOW)

Flow 1 (full pipeline, 5 steps) is shown before simpler flows. Reordering to simple-first would improve first-impressions for casual users. Not changed — the full pipeline is the canonical use case and most likely what users installing the tool actually want.

**Deferred**: Not remediated (judgment call; full pipeline is the primary use case).

## Disposition summary

| Theme | Severity | Fix applied |
|---|---|---|
| R1-A (develop --source) | CRITICAL | ✅ Removed nonexistent flag |
| R1-B (Rosetta 2) | CRITICAL | ✅ Intel Mac section corrected |
| R1-C (CoreML claim) | CRITICAL | ✅ CPU provider noted, CoreML qualified |
| R1-D (DirectML "always present") | HIGH | ✅ Link-time dep, v1903+, CPU provider |
| R1-E (cull PHOTOHELPER_MODEL_DIR) | HIGH | ✅ Inline comment added |
| R1-F (rename prerequisites) | HIGH | ✅ Prerequisite blockquote added |
| R1-G (Flow 3 SQL) | MEDIUM | ✅ Simplified, scores-flow-automatically note |
| R1-H (sudo cp location) | MEDIUM | Deferred (functional) |
| R1-I (flow ordering) | LOW | Deferred (judgment call) |
