# Session 04 — AI culling pipeline, Session-End Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "claude-sonnet-4-6 [1m] (orchestrator); opus (all sub-agents)"
  model_observed: unverifiable
  effort_claimed: MAX
  effort_observed: unverifiable
  ask_user_question_id: null
  user_response: option-1
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

**Note**: D2a+D2b already received a dedicated sub-component review (R1+R2). This session-end review focuses on D1e and D3 which are new, plus cross-cutting session-wide consistency.

## Triage summary

| Severity | Count | Themes |
|---|---|---|
| CRITICAL | 1 | A |
| HIGH | 4 | B, C, D, E |
| MEDIUM | 9 | F, G, H, I, J, K, L, M, N |
| LOW | 1 | O (also covered by other themes) |
| **Total** | **15** | |

---

## Theme A — `file_missing` counter is unreachable dead code; Step ordering wrong `CRITICAL`

**Agents**: code-architect, code-reviewer, silent-failure-hunter, code-simplifier, general-purpose — 5/8

`crates/photohelper-cli/src/commands/cull.rs:145-170`

Step 1 (`PhotoId::derive(&source_path)`) calls `std::fs::metadata` internally. If the file is missing, `metadata()` returns `Err(NotFound)` which routes to `derive_failed`. Step 2's `!source_path.exists()` check at line 167 is only reachable when Step 1 succeeded — i.e., the file existed and was hashed. The `file_missing` counter therefore **never increments** for missing files; they are silently misclassified as `derive_failed`.

The pipeline docstring promises "All five failure modes have per-photo counters" — the file-missing counter is functionally inert. Operators see `file-missing: 0` and `derive-failed: N` with no way to distinguish "files deleted since ingest" from "genuine derive errors."

**Remediation**: Swap Steps 1 and 2 — move the existence check before `PhotoId::derive`:

```rust
// Step 1: Existence pre-check.
if !source_path.exists() {
    tracing::warn!(path = %source_path.display(), "file missing since ingest; skipping");
    stats.file_missing.fetch_add(1, Ordering::Relaxed);
    return;
}
// Step 2: Re-derive PhotoId (content-change detection).
let current_id = match PhotoId::derive(&source_path) { ... };
```

This correctly counts missing files AND consolidates the missing-warn! (Theme C).

---

## Theme B — `run_cull` error arm hardcodes `EX_IOERR`; lock + permission errors exit wrong code `HIGH`

**Agent**: code-reviewer

`crates/photohelper-cli/src/main.rs:157`

```rust
Err(err) => {
    tracing::error!("{err:#}");
    ExitCode::from(exit_code::EX_IOERR)   // ← hardcoded
}
```

`run_cull` calls `Catalog::open` which can return `CatalogLockHeld` (→ should be `EX_TEMPFAIL` 75) and `Io{PermissionDenied}` (→ `EX_NOPERM` 77). The `Command::Ingest` handler at line 136 correctly calls `exit_code_for_error(&err)`. The `Cull` arm doesn't.

**Remediation**: Replace `exit_code::EX_IOERR` with `exit_code_for_error(&err)`:
```rust
Err(err) => {
    tracing::error!("{err:#}");
    ExitCode::from(exit_code_for_error(&err))
}
```

---

## Theme C — `file_missing` skip path emits no `tracing::warn!` `HIGH`

**Agent**: silent-failure-hunter

`crates/photohelper-cli/src/commands/cull.rs:167-170`

Every other skip path in `run_cull` logs the offending file path: `derive_failed` (line 148), `content_changed` (line 158), `decode_failed` (line 176), `infer_failed` (line 186), `catalog_inconsistency` (line 204) all emit `tracing::warn!`. The `file_missing` path is the only one that silently returns. Operators cannot determine which files are missing without manual cross-referencing.

**Remediation**: Add the warn! (consolidated with Theme A's swap — the warn goes in the new Step 1 existence check).

---

## Theme D — `"(user_qual=3)"` in ffi.rs comment is factually incorrect `HIGH`

**Agent**: comment-analyzer

`crates/photohelper-raw/src/ffi.rs:617`

```rust
// Run the default dcraw pipeline: AHD demosaic (user_qual=3) with
```

LibRaw initializes `user_qual = -1` (sentinel meaning "use default quality"). During `dcraw_process`, `quality = 2 + !IO.fuji_width` resolves to 3 (AHD) for non-Fuji cameras. Setting `user_qual = 3` explicitly would bypass this path and have different behavior on Fuji cameras. The comment conflates the resolved quality value with the parameter.

**Remediation**:
```rust
// Run the default dcraw pipeline: AHD demosaic for non-Fuji Bayer sensors
// (user_qual=-1 default; quality resolves to 3 = AHD internally) with
// output_bps=8 (both LibRaw defaults). No params need setting.
```

---

## Theme E — No integration test for cull idempotency (second run → `already-scored`) `HIGH`

**Agent**: pr-test-analyzer

The D3 test `cull_scores_real_canon_r8_cr3_fixture` runs cull once and checks `scored: 2`. It never runs cull a second time to verify the `already_scored` counter fires and `scored` stays at 0. The `cull.rs:198-201` match arms for `AlreadyScored` and the summary line rendering are untested end-to-end.

**Remediation**: Extend the existing fixture test to run cull a second time:
```rust
// Second run: all photos already scored.
Command::cargo_bin("photohelper")
    .unwrap()
    .env("PHOTOHELPER_MODEL_DIR", model_dir.to_str().unwrap())
    .args(["--catalog", cat_path.to_str().unwrap(), "cull"])
    .assert()
    .code(0)
    .stderr(contains("already-scored: 2"))
    .stderr(contains("scored: 0"));
```

---

## Theme F — `catalog_written` counter is dead code `MEDIUM`

**Agents**: code-architect, code-reviewer, type-design-analyzer, code-simplifier — 4/8

`crates/photohelper-cli/src/commands/cull.rs:44, 59, 197, 201`

`catalog_written` is declared, initialized, incremented in both `Inserted` and `AlreadyScored` arms — but never appears in `summary_line()` (lines 64-79) or exit-code logic (lines 222-240). It always equals `scored + already_scored` making it a redundant derived value.

**Remediation**: Remove the field, its initialization, and both `fetch_add` calls.

---

## Theme G — `"nima_mobilenet_aesthetic"` hardcoded twice in `main.rs` `MEDIUM`

**Agent**: general-purpose

`crates/photohelper-cli/src/main.rs:151,153`

```rust
match VerifiedModelBytes::from_manifest(&model_dir, "nima_mobilenet_aesthetic") {
    Ok(model) => {
        let model_path = model_dir.join("nima_mobilenet_aesthetic.onnx");
```

The manifest key / filename stem appears as a bare string literal twice. If a second model is added or the filename changes, both sites must be found manually. `MODEL_SLUG` is correctly exported from `photohelper-ai` for the scorer slug; the filename stem should be too.

**Remediation**: Add `pub const MODEL_MANIFEST_NAME: &str = "nima_mobilenet_aesthetic";` to `crates/photohelper-ai/src/model_bytes.rs`, re-export it from `lib.rs`, and replace both literals.

---

## Theme H — TD-012 in-source comment missing from `read_raw_rgb` `MEDIUM`

**Agent**: general-purpose

`crates/photohelper-raw/src/decode.rs:153` (read_raw_rgb function body)

TECH-DEBT.md TD-012 specifies `In-source: // TD-012: AHD demosaic stop-gap`. `grep -rn "TD-012" crates/` returns 0 matches. Per CLAUDE.md §No Acceptable Trade-offs Policy: "The stop-gap MUST be labeled in-source."

**Remediation**: Add inside `read_raw_rgb` or `parse_libraw_rgb_image`:
```rust
// TD-012: AHD demosaic stop-gap; upgrade to configurable algorithm when
// DxO-quality pipeline lands. See TECH-DEBT.md § TD-012.
```

---

## Theme I — "Nothing useful happened" `EX_USAGE` fires on systematic per-photo failures `MEDIUM`

**Agent**: code-architect

`crates/photohelper-cli/src/commands/cull.rs:237-239`

```rust
if walked > 0 && (scored + already_scored) == 0 {
    return Ok(exit_code::EX_USAGE);
}
```

If all N photos fail with `derive_failed` (e.g., all source files are on an unmounted volume), `EX_USAGE` (64) fires — signaling "likely wrong directory." That is misleading; the catalog was correct but the files are gone. Scripts checking exit codes will misinterpret systematic file loss as a path mismatch.

**Remediation**:
```rust
let all_per_photo_errors = derive_failed
    + decode_failed
    + infer_failed
    + stats.file_missing.load(Ordering::Relaxed)
    + stats.content_changed.load(Ordering::Relaxed);
if walked > 0 && (scored + already_scored) == 0 && all_per_photo_errors == 0 {
    return Ok(exit_code::EX_USAGE);
}
```

---

## Theme J — `RgbConversionFailed{bits:8,colors:3}` reused for buffer-size mismatch `MEDIUM`

**Agents**: type-design-analyzer, silent-failure-hunter

`crates/photohelper-raw/src/ffi.rs:726-729`

When `RgbImage::new` fails (pixel buffer length ≠ `width*height*3`), the error is mapped to `RgbConversionFailed { bits: 8, colors: 3 }`. But bits=8 and colors=3 were already validated as correct. The operator-facing error says "unexpected format: bits=8, colors=3 (expected bits=8, colors=3)" — a tautology.

**Remediation**: Map to the existing `RawImageDimensionMismatch` variant or use a `LibRawCallFailed` with a descriptive op string:
```rust
RgbImage::new(pixels, width_nz, height_nz).map_err(|_| Error::RawDecodeFailed {
    path: path.to_path_buf(),
    cause: RawDecodeCause::LibRawCallFailed {
        libraw_code: 0,
        op: "pixel buffer length != width*height*3 (LibRaw data_size inconsistency)",
    },
})
```

---

## Theme K — TD-016 TECH-DEBT.md entry is stale `MEDIUM`

**Agents**: general-purpose, comment-analyzer

`TECH-DEBT.md:271`

TD-016 status reads `"Open (prospective — cull.rs not yet created; D4 deferred due to D0 ABORT + DN-026)"`. `cull.rs` now exists and the heartbeat duplication is real. The TD-016 in-source comments in `cull.rs:127,243` are correct; only the TECH-DEBT.md entry needs updating.

**Remediation**: Update TD-016 status + stop-gap location fields in TECH-DEBT.md.

---

## Theme L — `main.rs:141` comment claims `current_exe()` failure → `EX_IOERR` (wrong) `MEDIUM`

**Agent**: comment-analyzer

`crates/photohelper-cli/src/main.rs:141`

```rust
// current_exe() failure -> EX_IOERR (cannot locate binary).
```

A `current_exe()` failure causes `.ok()` → `None` → `.and_then(...)` → `None` → `.unwrap_or_else(|| PathBuf::from("models"))`. The fallback is `"models"` (relative path), not `EX_IOERR`.

**Remediation**: `// model_dir: PHOTOHELPER_MODEL_DIR env var if set, else binary-adjacent models/. Falls back to relative "models" if current_exe() fails.`

---

## Theme M — Zero-rows early return hardcodes summary string `MEDIUM`

**Agents**: code-simplifier, pr-test-analyzer

`crates/photohelper-cli/src/commands/cull.rs:116-121`

The zero-rows fast path reproduces the summary format as a hardcoded literal. Adding or renaming a counter in `summary_line()` would silently diverge.

**Remediation**:
```rust
if rows.is_empty() {
    eprintln!("{}", CullStats::new().summary_line());
    return Ok(0);
}
```

---

## Theme N — `let _ = heartbeat_handle.join()` lacks justifying comment `LOW`

**Agent**: silent-failure-hunter

`crates/photohelper-cli/src/commands/cull.rs:218`

CLAUDE.md: "Never discard an error with `let _ = …` on a production path without a justifying comment." The corresponding `ingest.rs` line (242) HAS a justifying comment; `cull.rs` line 218 does not.

**Remediation**: Add comment: `// join() result discarded intentionally — early death is already surfaced by is_finished() WARN above.`

---

## Disposition summary

| Theme | Severity | Action |
|---|---|---|
| A | CRITICAL | Fix: swap Step 1/2; add warn! |
| B | HIGH | Fix: use exit_code_for_error |
| C | HIGH | Resolved by A |
| D | HIGH | Fix: correct comment |
| E | HIGH | Fix: add idempotency test |
| F | MEDIUM | Fix: remove dead catalog_written |
| G | MEDIUM | Fix: export MODEL_MANIFEST_NAME constant |
| H | MEDIUM | Fix: add TD-012 in-source comment |
| I | MEDIUM | Fix: refine EX_USAGE condition |
| J | MEDIUM | Fix: use LibRawCallFailed for buffer mismatch |
| K | MEDIUM | Fix: update TD-016 in TECH-DEBT.md |
| L | MEDIUM | Fix: correct comment |
| M | MEDIUM | Fix: use CullStats::new().summary_line() |
| N | LOW | Fix: add justifying comment |

## R1 watch-list (Round 2 must verify)

1. Steps 1 and 2 swapped in `run_cull`; `file_missing` properly reachable; `tracing::warn!` present.
2. `run_cull` Cull error arm calls `exit_code_for_error(&err)`.
3. ffi.rs:617 comment corrected to `user_qual=-1` / quality-3-resolved-internally.
4. Cull idempotency test added (second run asserts `already-scored: 2`).
5. `catalog_written` field removed.
6. `MODEL_MANIFEST_NAME` constant exported from photohelper-ai; main.rs uses it.
7. TD-012 in-source comment present in decode.rs/ffi.rs.
8. EX_USAGE condition guards against systematic per-photo-error case.
9. `RgbConversionFailed` not reused for buffer-size mismatch.
10. TD-016 TECH-DEBT.md entry updated (not "prospective").
11. main.rs:141 comment corrected.
12. Zero-rows early return uses `CullStats::new().summary_line()`.
13. `let _ = heartbeat_handle.join()` has justifying comment.

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 12
  verified: 10
  drifted: 2
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: A
      file: crates/photohelper-cli/src/commands/cull.rs
      line: 145
      present: yes
      retain: yes
      reason: PhotoId::derive calls fs::metadata; missing file → derive_failed; file_missing unreachable
      evidence_snippet: "let current_id = match PhotoId::derive(&source_path) {"

    - finding_id: B
      file: crates/photohelper-cli/src/main.rs
      line: 157
      present: yes
      retain: yes-with-corrected-line
      reason: Err arm hardcodes EX_IOERR; actual line 157 not 158
      evidence_snippet: "ExitCode::from(exit_code::EX_IOERR)"

    - finding_id: C
      file: crates/photohelper-cli/src/commands/cull.rs
      line: 167
      present: yes
      retain: yes
      reason: file_missing path has no tracing::warn!; every other path does
      evidence_snippet: "stats.file_missing.fetch_add(1, Ordering::Relaxed);"

    - finding_id: D
      file: crates/photohelper-raw/src/ffi.rs
      line: 617
      present: yes
      retain: yes-with-corrected-line
      reason: "(user_qual=3)" present at line 617; user_qual=-1 at init; 3 is resolved internally
      evidence_snippet: "// Run the default dcraw pipeline: AHD demosaic (user_qual=3) with"

    - finding_id: F
      file: crates/photohelper-cli/src/commands/cull.rs
      line: 44
      present: yes
      retain: yes
      reason: catalog_written declared and incremented but not in summary_line or exit-code logic
      evidence_snippet: "catalog_written: AtomicU64,"

    - finding_id: G
      file: crates/photohelper-cli/src/main.rs
      line: 151
      present: yes
      retain: yes
      reason: "nima_mobilenet_aesthetic" hardcoded; no constant in photohelper-ai
      evidence_snippet: "match VerifiedModelBytes::from_manifest(&model_dir, \"nima_mobilenet_aesthetic\") {"

    - finding_id: H
      file: crates/photohelper-raw/src/decode.rs
      line: 153
      present: yes
      retain: yes-with-corrected-line
      reason: read_raw_rgb at line 153 (not 127); no TD-012 comment anywhere in decode.rs
      evidence_snippet: "pub fn read_raw_rgb(path: &Path) -> Result<RgbImage, Error> {"

    - finding_id: I
      file: crates/photohelper-cli/src/commands/cull.rs
      line: 237
      present: yes
      retain: yes
      reason: EX_USAGE fires when scored+already_scored==0 even if all photos had derive_failed
      evidence_snippet: "if walked > 0 && (scored + already_scored) == 0 {"

    - finding_id: J
      file: crates/photohelper-raw/src/ffi.rs
      line: 726
      present: yes
      retain: yes
      reason: RgbConversionFailed{bits:8,colors:3} used for buffer mismatch when bits/colors already validated correct
      evidence_snippet: "cause: RawDecodeCause::RgbConversionFailed { bits, colors },"

    - finding_id: K
      file: TECH-DEBT.md
      line: 271
      present: yes
      retain: yes
      reason: TD-016 says "cull.rs not yet created" but cull.rs exists at 275 lines
      evidence_snippet: "Status: Open (prospective — cull.rs not yet created"

    - finding_id: M
      file: crates/photohelper-cli/src/commands/cull.rs
      line: 116
      present: yes
      retain: yes
      reason: hardcoded zero-summary string diverges from summary_line()
      evidence_snippet: "walked: 0, scored: 0, already-scored: 0, decode-failed: 0,"

    - finding_id: N
      file: crates/photohelper-cli/src/commands/cull.rs
      line: 218
      present: yes
      retain: yes
      reason: let _ = heartbeat_handle.join() lacks justifying comment per CLAUDE.md
      evidence_snippet: "let _ = heartbeat_handle.join();"
```
