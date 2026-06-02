# Session 15 — Plan (`watermark-and-rename`), Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "Gemini 3.5 Flash (High)"
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
  agents_requested: ["general-purpose", "feature-dev:code-architect", "feature-dev:code-reviewer", "pr-review-toolkit:type-design-analyzer", "pr-review-toolkit:silent-failure-hunter", "pr-review-toolkit:comment-analyzer", "pr-review-toolkit:pr-test-analyzer", "pr-review-toolkit:code-simplifier"]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

Scope: `docs/plans/session-15.md` (plan review — no implementation code exists yet). Full 8-agent suite (Cadence A Tier 5) + 9th-agent verification. The plan is explicitly a "FIRST DRAFT to be hardened"; these are the items to nail down before any code.

## Triage summary

<table>
<tr><th>Severity</th><th>Count</th></tr>
<tr><td>CRITICAL</td><td>7</td></tr>
<tr><td>HIGH</td><td>10</td></tr>
<tr><td>MEDIUM</td><td>2</td></tr>
<tr><td>LOW</td><td>1</td></tr>
</table>

The dominant signal: **the plan repeatedly describes net-new work as "reuse of existing internals."** Themes A, B, C, H, I, Q all stem from that — `export_photo` is a private monolith, the RAW/raster color spaces differ, the shadow/alpha model is unstated, and the sidecar/filename conventions differ from what the plan asserts. Four themes carry genuine **product decisions** (B, E, F, M) surfaced to the user before remediation.

---

## Theme A — "Reuse export internals" is not achievable as written; `export_photo` is a private monolith (CRITICAL)

- [feature-dev:code-architect, code-simplifier, general-purpose]: D-Q1/D1/D3 assert reuse of "resize / composite / JPEG-encode … promoted to shared functions," but `export_photo` (`crates/photohelper-export/src/lib.rs:177-347`) inlines decode→tone-map→resize(`:235-297`)→composite→demultiply→encode as one 170-line function; only `draw_image_watermark`/`calculate_watermark_position`/`compress_jpeg` are separable and they are **private `fn`** (`lib.rs:478,349,664`). Resize is an inline block, not a function. 'CRITICAL'
- [comment-analyzer]: `TempFileGuard` is mischaracterized as an "atomic rename" mechanism — it is drop-cleanup only; the rename is done by the caller (`export.rs:480`) and the guard is **CLI-private** (`export.rs:186-219`), so "reuse `TempFileGuard`" needs a promotion step. 'HIGH'

**Remediation**: Insert an explicit **D1.0 extraction deliverable BEFORE D1b**: extract `pub fn resize_rgb(...)`, `pub fn render_to_jpeg(rgb,w,h,opts)` (resize+composite+demultiply+encode), `pub fn pixmap_to_rgb(...)`, and make `compress_jpeg`/`draw_image_watermark`/`calculate_watermark_position` `pub`; **re-point the existing `export` at the extracted functions in the same commit and green the existing export integration tests as the regression guard**. Move `TempFileGuard` + a generic `resolve_collisions<F: Fn(&DevelopRow)->String>` into `crates/photohelper-cli/src/commands/util.rs`, used by both `export` and `rename`. This single step neutralizes the duplication in A, R (geometry), I, and the rename driver clone.

## Theme B — Color-space mismatch: RAW (ACES-filmic tone-mapped) vs raster (display sRGB) composited identically → defeats the "uniform look" goal (CRITICAL · product decision)

- [feature-dev:code-architect]: D1a routes RAW through `decode_image(Linear16)` + `ToneMappingLut` (ACES filmic S-curve + sRGB OETF; white maxes ~230 not 255 — `crates/photohelper-export/src/isp.rs:11-50`, applied at `lib.rs:184-219`), while raster JPEG/PNG decode is already display-referred sRGB. The shared composite treats both as interchangeable, so the same scene as CR3 vs JPEG yields different tone/contrast — and the black shadow + marks read differently against a filmic-rolled-off vs literal-sRGB background. 'CRITICAL'

**Remediation**: Decide and document the target compositing color space. Recommended: **plain 8-bit sRGB (display-referred)** for a uniform look — RAW path for `watermark` uses LibRaw's `ProcessOptions::Srgb8` (auto-bright sRGB, close to a camera JPEG) rather than the filmic develop ISP; raster decodes straight to sRGB8. Add a test compositing a mid-gray patch via both paths, asserting post-composite values match within tolerance. **→ user decision Q-A.**

## Theme C — Shadow gradient vs premultiplied-alpha / demultiply: naive translucent-black write is silently discarded (CRITICAL)

- [feature-dev:code-architect, general-purpose, type-design-analyzer]: D1c says the gradient is "per-row alpha ramping 255→0 … composited over the resized base," but `tiny_skia::Pixmap` is premultiplied and the final stage **demultiplies** every pixel `r/a*255` assuming base alpha==255 (`lib.rs:311-335`). A gradient that lowers destination alpha makes the demultiply brighten/zero exactly the rows meant to darken → a hard black cliff, not a fade (partial darkening is thrown away when the 3-channel JPEG buffer is written). The plan's endpoint-only test (`:110-112`) passes even with this cliff bug. 'CRITICAL'

**Remediation**: Specify the shadow as a **color operation that keeps destination alpha at 255** — `out_rgb = base_rgb * (1 - t)`, `t = row_ramp/255` — mirroring `draw_image_watermark`'s "leave background alpha untouched" (`lib.rs:571`). Do NOT model it as writing the pixmap alpha channel. Make the ramp a pure function `shadow_alpha_ramp(H)->Vec<u8>` with the denominator pinned; test the **final demultiplied 3-channel buffer** at a mid-band row (not the intermediate pixmap), assert monotonic ramp, exact endpoints (`[H-1]==255`, `[H-band]==0`), the above-band row **bit-identical** to source, and a band-height-zero guard for tiny images (no divide-by-zero).

## Theme D — `rename` path safety: unsanitized filename + no `--output` containment + `--output`-inside-`--source` self-ingest (CRITICAL)

- [code-reviewer, feature-dev:code-architect, general-purpose, type-design-analyzer]: D3 builds `Cluster-{X}_Cull-{Y}-{stem}.{ext}` from `DevelopRow::source_path()` (catalog-stored, canonicalized-at-ingest, never re-validated — `row.rs:77-90,158`) with no sanitization; `--output` is never validated to actually contain the joined destination; nothing prevents `--output` being inside `--source` (recursive self-ingest); the `--source` filter is "starts_with" which over-matches (`/trip` vs `/trip-2`) and under-matches (relative vs canonical). 'CRITICAL'

**Remediation**: (1) Canonicalize `--output` once; validate every destination with `AbsPath::canonicalize_within`-style containment (`crates/photohelper-core/src/model.rs:264`, returns `Error::PathEscapesRoot`). (2) Build the name from `Path::file_stem`/`extension` only; sanitize (reject path separators/NUL/control chars; cap total length incl. the ~24-char prefix vs `NAME_MAX`; decide non-UTF-8 policy — reject or lossy-with-warn, not silent). (3) Reject (or prune) `--output` equal-to/nested-in `--source` for BOTH subcommands; the `watermark` walker must exclude the `--output` subtree (mirror the `.photohelper` skip at `ingest.rs:146-153`). (4) Filter `--source` by canonical path-component prefix.

## Theme E — A requested watermark can be silently omitted → plausible-but-wrong deliverable (CRITICAL · product decision)

- [silent-failure-hunter, feature-dev:code-architect, general-purpose]: D1b's "Fit-guard: omit + warn (or error under `--strict`)" means a non-fitting mark is dropped and the JPEG still ships — the deliverable is missing the mark the user explicitly requested, with `errored: 0`. Also, the existing `WatermarkOmitted` is treated as a **hard per-photo error** in `export` (`export.rs:497-508`) — the opposite of "omit + warn" — so blind reuse changes semantics. 'CRITICAL'

**Remediation**: For `watermark`, a mark that does not fit is an **error by default** (no output JPEG written), via a new typed variant (e.g. `MarkDoesNotFit { which, mark_dims, target_dims }`), counted in a dedicated counter that drives a non-zero exit (`EX_PARTIAL_FAIL`) **even without `--strict`**; `--strict` makes it fatal. Define the exit-code contribution explicitly. **→ user decision Q-B.**

## Theme F — Untested non-CR3 RAW decodes fail-open AND can decode to garbage → silently-wrong JPEG (CRITICAL · product decision)

- [silent-failure-hunter, pr-test-analyzer, general-purpose]: D-Q2 ships best-effort decode for 7 RAW formats with only CR3 tested. Decode *failure* is fail-open (fine), but LibRaw can *succeed* on NEF/ARW with wrong dims/garbage; the Linear16 export path has **no content sanity check** (`lib.rs:184-220`), so a mis-decoded RAW becomes a silently-wrong watermarked JPEG (`errored: 0`). The DN+TD documents the risk but adds no guard. Separately, the fail-open/`--strict` dispatch itself is **in-scope and untested** (distinct from deferring NEF decode verification). 'CRITICAL'

**Remediation**: Keep best-effort (user's Q2 choice) but add a guard: gate non-CR3 RAW behind an explicit `--allow-untested-raw` opt-in (preferred) **or** add a post-decode dimension-sanity assertion treating absurd output as a decode *error*. Decode errors must short-circuit **before** any output is committed (temp-then-rename). Add tests: corrupt RAW → skip+warn, exit 2; with `--strict` → exit 1 (reuse the `cli.rs:2343` corrupt-file pattern). **→ user decision Q-C.**

## Theme G — `rename` RAW+sidecar non-atomicity; `sidecar-absent` vs `sidecar-copy-failed` conflated (CRITICAL)

- [silent-failure-hunter, code-reviewer]: D3c copies RAW then sidecar as independent steps; if the RAW commits and the `.xmp` copy then fails (EACCES/ENOSPC), `--output` holds a renamed RAW missing its sidecar (edits silently dropped). "Sidecar legitimately absent" and "sidecar copy failed" are lumped together. `std::fs::rename` is non-atomic across filesystems (EXDEV). 'CRITICAL'

**Remediation**: Copy RAW + sidecar as a **unit** — both to temps, commit both renames only after both succeed; on sidecar failure drop the RAW temp via `TempFileGuard`. Surface distinct counters: `renamed`, `sidecar_copied`, `sidecar_absent`, `sidecar_copy_failed`; the last is counted toward failures + `--strict`-fatal. Temp files live under `--output` (avoid EXDEV).

## Theme H — Sidecar convention: repo uses ONLY `<stem>.xmp`; the plan's "handle both conventions" is dead + risky (HIGH · 6-agent consensus)

- [code-architect, code-reviewer, silent-failure-hunter, comment-analyzer, pr-test-analyzer, code-simplifier]: D3c's "handle both `<stem>.xmp` and `<name.ext>.xmp`" contradicts the repo's single convention — extension-replaced `photo.CR3`→`photo.xmp` (`crates/photohelper-sidecar/src/lib.rs:4-5`; used via `with_extension("xmp")` at `export.rs:374`, `develop.rs:242`). `photo.CR3.xmp` is produced nowhere. The output sidecar must be `new_raw_path.with_extension("xmp")`. 'HIGH'

**Remediation**: Resolve now — `<stem>.xmp` only; output sidecar = renamed RAW with extension replaced. Pin the exact renamed sidecar filename in an integration assertion. If foreign `<name.ext>.xmp` detection is wanted later, make it an explicit, tested addition with defined precedence + warn-on-both — not a "TBD."

## Theme I — Filename format under-specified + divergent from `export` + numeric edge cases (HIGH)

- [comment-analyzer, code-reviewer, type-design-analyzer, code-simplifier, general-purpose, pr-test-analyzer]: "zero-padded NIMA score" is integer-sounding; the example `Cull-07.85` requires fixed-width float `{:05.2}` (the existing `format_nima_score_label`, `util.rs:15`) or lexical sort breaks (DN-029 `:329`). The plan's capitalized `Cluster-{X}_Cull-{Y}-` + underscore diverges from `export`'s lowercase `cluster-{id:03}-cull-{score:05.2}-` (`export.rs:282-296`) with no recorded rationale. Unhandled: `None` sentinel strings (unnamed; must sort predictably), `cluster_id` ≥1000 / negative (`Cluster--01` bug; DB `CHECK(cluster_id>=0)` not mirrored in type — `schema.rs:84`), NaN/inf score (reuse the `is_finite` filter at `export.rs:280`). 'HIGH'

**Remediation**: `Y` = `format_nima_score_label` (`{:05.2}`); name the `None` sentinels explicitly + assert sort order; **honor the spec's capitalized `Cluster-..._Cull-...` form** (it is the user's literal spec) but record the divergence from `export` in a `docs/decisions/` note and route both `export` and `rename` through ONE shared prefix formatter; clamp/validate score before format; handle large/negative cluster ids.

## Theme J — Raster EXIF orientation ignored → portrait JPEGs watermarked in the wrong corner (HIGH)

- [feature-dev:code-architect]: the `image` crate does not auto-apply EXIF orientation; a portrait phone JPEG (`Orientation=6`) decodes as a landscape buffer, so mark1 ("top-right") lands at the visual bottom-right — while CR3s are upright (LibRaw pre-rotates). The aspect-ratio-only zero-distortion test would pass while the image is rotated 90°. 'HIGH'

**Remediation**: Read raster EXIF orientation and apply the transform after decode (preferred for "uniform look"), OR document raster-orientation as out-of-scope with a TD + binding trigger. Add a portrait-JPEG fixture with a non-1 orientation tag asserting the mark lands at the visually-correct corner (or the documented no-op).

## Theme K — Downscale-only not enforced; `--output` probe + walk-excludes-output / idempotency (HIGH)

- [code-architect, code-reviewer, pr-test-analyzer, silent-failure-hunter, general-purpose]: the existing resize applies `scale = limit/long_edge` **unconditionally** (`lib.rs:235-245`), so reusing it **upscales** sub-limit images — contradicting the plan's downscale-only rule, which is also untested (the zero-distortion test only covers larger-than-max). No up-front `--output` writability probe is specified (export has one at `export.rs:228-244`); the walker doesn't exclude `--output`, so a second run re-processes its own outputs. 'HIGH'

**Remediation**: `scale = (limit as f32 / long_edge).min(1.0)` (or early-skip). Reuse export's up-front output probe. Exclude the `--output` subtree from the walk + reject nesting. Tests: sub-limit image not enlarged; run-twice idempotent output count.

## Theme L — Dual-mark geometry robustness: zero-size marks, mark-wider-than-image, overlap, fit-guard untested (HIGH)

- [feature-dev:code-architect, type-design-analyzer, pr-test-analyzer]: height-relative sizing has no `.max(1)` floor → on tiny pass-through images `mark_h`/`mark_w` can round to 0 → `Pixmap::new(0,..)` returns `None` (`lib.rs:530-535`). A wide logo at 14% of height can exceed image width → negative top-right `x`, which the blit loop silently clips rather than the pre-check catching. No reasoning about mark1/mark2 overlap on extreme aspect ratios, nor mark2-vs-shadow intended overlap. The fit-guard is the primary error path and has no planned test. 'HIGH'

**Remediation**: `mark_h = round(H*f).max(1)`, `mark_w = round(mw*scale).max(1)`; if a mark cannot fit, return a typed `Result` (no panic/`None`-unwrap). Reason about co-occupancy (wide-mark → omit/clamp; assert mark2 sits within the shadow band). Pin **exact-integer** geometry unit tests for landscape/portrait/square + wide-logo + tiny-image + fit-guard Err/skip.

## Theme M — Mark assets: PNG-only loader vs spec "image file"; JPEG/no-alpha/missing-mark handling (HIGH · product decision)

- [code-reviewer, silent-failure-hunter, comment-analyzer, general-purpose, type-design-analyzer]: `PreloadedBadge::load` is PNG-only (`tiny_skia::Pixmap::decode_png`, `lib.rs:84-89`); a JPEG `--mark1` fails today. The plan leaves "JPEG marks TBD." A no-alpha mark composites as an opaque box. Mark-load failure should be fatal up-front (mirror `export.rs:320-331`), not per-file fail-open. 'HIGH'

**Remediation**: Decide JPEG-mark support: either reject non-PNG marks early with a clear fatal error, or load marks through the new raster decoder (D1a) and define no-alpha behavior. Keep mark-load failure fatal before the parallel loop. **→ user decision Q-D.**

## Theme N — `rename` lacks the source-existence precheck `export` has (HIGH)

- [silent-failure-hunter, code-reviewer, code-architect]: catalog `source_path` may not exist on disk (deleted/moved since ingest — `row.rs:143-145`); `export` prechecks `exists()` + counts `file_missing` (`export.rs:364-371`), but D3 has no such step, so a deleted source silently yields fewer outputs than rows. 'HIGH'

**Remediation**: Add a per-row `exists()` precheck → `file_missing` counter → `--strict`-fatal, mirroring `export`. Test: catalog row whose RAW was deleted → `file_missing++`, not silent omission.

## Theme O — Type design / invariant expression vs the house style (HIGH)

- [pr-review-toolkit:type-design-analyzer]: the plan commits to loose float tuples (geometry), bare magic constants (`0.14/0.13/0.046/0.30`), stringly extension dispatch, and `format!`-string filenames — a regression from the private-field-newtype + fallible-constructor house style (`decode.rs` `BayerPlane`/`SensorBitDepth`, `model.rs` `AbsPath`, `sidecar/path.rs`). 'HIGH'

**Remediation** (balanced against Theme R — express invariants where it *removes* a runtime-by-convention check, don't over-build): name + validate the four fraction constants in ONE module; an exhaustive `SourceKind` classify→`Option` (unsupported-extension distinct from decode-failure, so the fail-open log can tell them apart); a sanitizing `RenamedFilename` builder shared by `export`+`rename`; reuse `validate_long_edge` + clap range on the arg structs. Do NOT build a heavyweight geometry module — parametrize the existing `calculate_watermark_position` (Theme R).

## Theme P — Test quality: outputs never decoded, gradient not isolated, non-destructive too weak, fixtures missing (HIGH)

- [pr-review-toolkit:pr-test-analyzer, general-purpose]: the repo has **no** test that decodes an output image's pixels/dimensions — the nearest analog asserts only `exists()` + `len>0` (`cli.rs:1892`), which passes on a fully broken pipeline; D2d restates that anti-pattern. No raster fixtures exist (`tests/fixtures/` is CR3-only) and generation is unspecified. Non-destructive needs a **directory-listing delta** (stray output written into `--source`), not just bytes+mtime of known files. Collision/case-insensitive collision, exact-integer geometry, fit-guard, `--strict`, empty-source, no-matching-rows, `--force`/overwrite, `--max-long-edge<16`, and mark2-atop-shadow ordering are all untested. 'HIGH'

**Remediation**: Enumerate concrete test rows with exact assertions; generate raster fixtures at runtime via the `image` crate with a **sentinel pixel block** (so "decoded" is distinguishable from "blank frame of right size"); decode outputs and assert dims/sample pixels; assert the source dir entry-set is unchanged; add the missing edge/error-path rows. Reuse seeded-catalog patterns (`cli.rs:1431-1461`) and the corrupt-file/strict patterns (`cli.rs:2322-2366`).

## Theme Q — Ledger / prose accuracy (HIGH)

- [pr-review-toolkit:comment-analyzer, general-purpose]:
  - **Q1**: session-14 count should read "**15 verified findings** (2C+9H+4M+2L raw; 3 hallucinations discarded, discard_rate 0.16)", not "2 CRITICAL + 9 HIGH" (`session-14-implementation-round3.md:27,97-108`).
  - **Q2**: "PR #15" is ambiguous — there are **two** "Merge pull request #15" commits (session-12 `17da1eb9` and session-14 `d4575fca`); anchor with the SHA.
  - **Q3**: `docs/discovery-notes.md` has **duplicate DN-029** (`:241`, `:329`) and **duplicate DN-033** (`:284`, `:305`) — "next free id" is uncomputable by eye; cite by content+line, state the next-free id explicitly, and renumber in the D4 discovery-notes pass.
  - **Q4**: "Cross-references DN-014" rests on a false premise — DN-014 governs *ingest* `RAW_EXTS=["cr3"]` (`ingest.rs:31`) and its trigger is "a non-Canon `CameraProfile`," which session 15 does not add; the new untested-RAW DN covers the *separate* `watermark` decoder. Reword so neither trigger is assumed to subsume the other.
  - **Q5**: "advance/close DN-036" contradicts DN-036's own trigger ("Next session that touches the `export` pipeline or `export_photo` logic", `:313-319`) vs the plan's "export untouched" stance. Reconcile-with-note (capability delivered in `watermark`; export-integration still deferred), don't hard-close.
  - **Q6**: `image` is a **dev-dependency** of `photohelper-cli` (not a plain dep; `code-simplifier`'s "plain dep + unused" claim is incorrect per 9th-agent check), and `photohelper-export` has **no** `image` dep — so D1a is a **net-new dependency on `photohelper-export`**, not a "promotion." Reword D1a; pin `default-features=false, features=["jpeg","png"]` and verify MSRV-1.88 + `cargo audit`.
  'HIGH'

**Remediation**: Apply Q1–Q6 prose fixes; add a D4 sub-step to renumber the pre-existing duplicate DN ids.

## Theme R — Over-scoping / missed reuse / premature flags (MEDIUM)

- [pr-review-toolkit:code-simplifier, type-design-analyzer, code-reviewer]: the "dual-mark geometry module" is a parametrization of the existing `calculate_watermark_position` (`lib.rs:349`, already computes the exact top-right/bottom-left formulas) + `draw_image_watermark` (already aspect-locks, bounds-guards, Bicubic-scales, alpha-blends) — only the size basis (height vs long-edge) and margin (4.6% per-axis vs 1.5% long-edge) differ. `--quality` on `watermark` is a premature flag (spec says fixed "high-quality"); `--max-long-edge` should reuse `validate_long_edge`; the `--max_long_edge` underscore-alias musing and the `RgbImage` loader-return-type are dead weight. 'MEDIUM'

**Remediation**: Parametrize the existing draw path (a `BadgeSizeBasis { LongEdge(Scale) | Height(f32) }` + explicit `margin_frac`) instead of a new module; make `calculate_watermark_position` `pub` so geometry tests assert it directly. Hard-code a high JPEG quality constant (drop `--quality` unless the user wants it); reuse `validate_long_edge`; loader returns `(Vec<u8>, w, h)` not `RgbImage`.

## Theme S — D0 housekeeping safety (LOW)

- [general-purpose, code-reviewer, code-simplifier]: D0 deletes untracked scratch (`test_quick_xml*`, `diff.txt`); add an explicit `rg`/`grep` confirmation that no `Cargo.toml`/`mod` references them before `rm`. 'LOW'

**Remediation**: Add the grep-confirm step to D0.

---

## Disposition summary

<table>
<tr><th>Theme</th><th>Severity</th><th>Action</th></tr>
<tr><td>A — reuse-is-extraction</td><td>CRITICAL</td><td>Add D1.0 extraction deliverable; re-point export</td></tr>
<tr><td>B — color space</td><td>CRITICAL</td><td>Decide compositing space (Q-A); add cross-path test</td></tr>
<tr><td>C — gradient/demultiply</td><td>CRITICAL</td><td>Darken-RGB-keep-alpha-255; test final buffer</td></tr>
<tr><td>D — rename path safety</td><td>CRITICAL</td><td>canonicalize_within + sanitize + reject nesting</td></tr>
<tr><td>E — silent watermark omit</td><td>CRITICAL</td><td>Error-by-default (Q-B)</td></tr>
<tr><td>F — untested RAW silent-wrong</td><td>CRITICAL</td><td>Gate/sanity-guard (Q-C) + dispatch tests</td></tr>
<tr><td>G — RAW+sidecar atomicity</td><td>CRITICAL</td><td>Unit copy + distinct counters</td></tr>
<tr><td>H — sidecar convention</td><td>HIGH</td><td>`&lt;stem&gt;.xmp` only; pin renamed name</td></tr>
<tr><td>I — filename format</td><td>HIGH</td><td>`{:05.2}` + shared formatter + sentinels + decision note</td></tr>
<tr><td>J — raster orientation</td><td>HIGH</td><td>Apply EXIF orientation or TD-defer + test</td></tr>
<tr><td>K — downscale-only / idempotency</td><td>HIGH</td><td>min(1.0) clamp + output probe + walk-exclude</td></tr>
<tr><td>L — geometry robustness</td><td>HIGH</td><td>.max(1) floors + overlap reasoning + exact tests</td></tr>
<tr><td>M — mark assets</td><td>HIGH</td><td>JPEG-mark decision (Q-D) + fatal-up-front</td></tr>
<tr><td>N — rename existence precheck</td><td>HIGH</td><td>file_missing counter</td></tr>
<tr><td>O — type design</td><td>HIGH</td><td>Named constants + SourceKind + RenamedFilename</td></tr>
<tr><td>P — test quality</td><td>HIGH</td><td>Decode outputs + sentinel fixtures + edge rows</td></tr>
<tr><td>Q — ledger accuracy</td><td>HIGH</td><td>Q1–Q6 prose fixes + DN renumber</td></tr>
<tr><td>R — over-scoping</td><td>MEDIUM</td><td>Parametrize not new-module; trim flags</td></tr>
<tr><td>S — D0 safety</td><td>LOW</td><td>grep-confirm before rm</td></tr>
</table>

No findings deferred to `TECH-DEBT.md` at plan stage (all are plan-text remediations); the plan's own out-of-scope deferrals (NEF/ARW fixtures, raster orientation if chosen) must carry binding triggers when filed during implementation.

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 30          # atomic code/ledger citations underpinning the 19 themes
  verified: 28
  drifted: 2                  # Y (DN-036 trigger at :318 within :313-319); D (formula at :358-359, fn at :349)
  hallucinated: 0             # no whole theme fabricated
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  corrections:
    - id: V
      note: "code-simplifier claimed `image` is a plain [dependencies] entry and unused; 9th-agent confirms it is a [dev-dependencies] entry of photohelper-cli. general-purpose's framing (net-new dep for photohelper-export) is correct; theme Q6 retained with corrected facts."
  details:
    - {finding_id: A, file: crates/photohelper-export/src/lib.rs, line: 177, present: yes, retain: yes, reason: "export_photo 177-347 monolith; resize inline 234-297"}
    - {finding_id: B, file: crates/photohelper-export/src/lib.rs, line: 664, present: yes, retain: yes, reason: "compress_jpeg private fn"}
    - {finding_id: C, file: crates/photohelper-export/src/lib.rs, line: 478, present: yes, retain: yes, reason: "draw_image_watermark long-edge scale + WatermarkOmitted guard 510-517"}
    - {finding_id: D, file: crates/photohelper-export/src/lib.rs, line: 349, present: yes, retain: yes, reason: "calculate_watermark_position fn @349; formula @358-359"}
    - {finding_id: E, file: crates/photohelper-export/src/lib.rs, line: 311, present: yes, retain: yes, reason: "demultiply r/a*255 311-335; base alpha 255 @267,292"}
    - {finding_id: F, file: crates/photohelper-export/src/isp.rs, line: 11, present: yes, retain: yes, reason: "ACES filmic; applied lib.rs 213-219"}
    - {finding_id: G, file: crates/photohelper-export/src/lib.rs, line: 84, present: yes, retain: yes, reason: "PreloadedBadge::load PNG-only decode_png @89"}
    - {finding_id: H, file: crates/photohelper-cli/src/commands/export.rs, line: 320, present: yes, retain: yes, reason: "badge preload fatal with_context+? @326, bail! @329"}
    - {finding_id: I, file: crates/photohelper-cli/src/commands/export.rs, line: 271, present: yes, retain: yes, reason: "upfront collision map; to_lowercase case-fold @300-301"}
    - {finding_id: J, file: crates/photohelper-cli/src/commands/export.rs, line: 186, present: yes, retain: yes, reason: "TempFileGuard drop-cleanup only; rename by caller @480; CLI-private"}
    - {finding_id: K, file: crates/photohelper-cli/src/commands/export.rs, line: 364, present: yes, retain: yes, reason: "source_path.exists() precheck → file_missing @366"}
    - {finding_id: L, file: crates/photohelper-cli/src/commands/export.rs, line: 282, present: yes, retain: yes, reason: "lowercase cluster-{id:03}- @282 + cull-{:05.2}- @289"}
    - {finding_id: M, file: crates/photohelper-cli/src/commands/export.rs, line: 228, present: yes, retain: yes, reason: "create_dir_all @228 + .ph_write_test probe @236"}
    - {finding_id: N, file: crates/photohelper-cli/src/commands/export.rs, line: 144, present: yes, retain: yes, reason: "pub fn validate_long_edge ≥16; value_parser @112"}
    - {finding_id: O, file: crates/photohelper-catalog/src/catalog.rs, line: 866, present: yes, retain: yes, reason: "LEFT JOINs; superseded IS NULL @880; ORDER BY ingested @881"}
    - {finding_id: P, file: crates/photohelper-catalog/src/row.rs, line: 149, present: yes, retain: yes, reason: "nima_score:Option<f32>@192; dedup_cluster_id:Option<i64>@197; canonicalized-at-ingest"}
    - {finding_id: Q, file: crates/photohelper-catalog/src/schema.rs, line: 84, present: yes, retain: yes, reason: "CHECK(cluster_id >= 0)"}
    - {finding_id: R, file: crates/photohelper-cli/src/commands/ingest.rs, line: 31, present: yes, retain: yes, reason: 'RAW_EXTS=&["cr3"]; follow_links(false)@146; skip .photohelper@151'}
    - {finding_id: S, file: crates/photohelper-core/src/model.rs, line: 264, present: yes, retain: yes, reason: "canonicalize_within → Error::PathEscapesRoot on non-prefix"}
    - {finding_id: T, file: crates/photohelper-sidecar/src/lib.rs, line: 4, present: yes, retain: yes, reason: "extension-replaced photo.CR3→photo.xmp; with_extension @export.rs:374"}
    - {finding_id: U, file: crates/photohelper-cli/src/commands/util.rs, line: 15, present: yes, retain: yes, reason: 'format_nima_score_label → "{score:05.2}"'}
    - {finding_id: V, file: crates/photohelper-cli/Cargo.toml, line: 45, present: yes, retain: yes-with-corrected-line, reason: "image is [dev-dependencies] of cli (header @38); photohelper-export has none → net-new dep there"}
    - {finding_id: W, file: crates/photohelper-cli/src/main.rs, line: 115, present: yes, retain: yes, reason: "EX_STRICT_FAIL=1 @115; EX_PARTIAL_FAIL=2 @117"}
    - {finding_id: X, file: docs/discovery-notes.md, line: 241, present: yes, retain: yes, reason: "duplicate DN-029 @241,329; duplicate DN-033 @284,305"}
    - {finding_id: Y, file: docs/discovery-notes.md, line: 318, present: drifted, retain: yes-with-corrected-line, reason: "DN-036 'touches export' trigger at :318"}
    - {finding_id: Z, file: docs/code-reviews/session-14-implementation-round3.md, line: 27, present: yes, retain: yes, reason: "triage 2C+9H+4M+2L; verification total:18 verified:15 hallucinated:3 discard:0.16"}
    - {finding_id: AA, file: GIT, line: 0, present: yes, retain: yes, reason: "two 'Merge pull request #15' — d4575fca (s14) + 17da1eb9 (s12)"}
    - {finding_id: AB, file: crates/photohelper-cli/tests/cli.rs, line: 1892, present: yes, retain: yes, reason: "export test asserts exists()+len>0 only; no pixel/dim decode"}
    - {finding_id: AC, file: crates/photohelper-cli/src/commands/export.rs, line: 497, present: yes, retain: yes, reason: "WatermarkOmitted falls into Err arm → errored++ @503"}
    - {finding_id: AD, file: crates/photohelper-export/src/lib.rs, line: 222, present: yes, retain: yes, reason: "fast-path bypass encodes directly when no resize + no watermark"}
```
