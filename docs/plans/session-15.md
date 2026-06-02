# Session 15 — Dual-Mark Watermarking & Metadata-Driven RAW Rename

> **Top block = the session contract.** Authored at session-start; hardened to
> **v2** via plan-review Round 1 (`docs/code-reviews/session-15-plan-round1.md`,
> 7C+10H+3M+2L across 19 themes) + four user decisions. Round 2 fires next.

---

## Session contract

- **Session**: 15 (`watermark-and-rename`)
- **Branch**: `session-15/watermark-and-rename`
- **Opened**: 2026-06-01
- **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code session 15)
- **Cadence**: A (tier-graduated; full 8-agent suite at plan-review, sub-component
  boundaries, and session-end per `docs/quality-assurance.md`)

### Goal

Ship two new **non-destructive, source-read-only** batch capabilities, both
emitting all artifacts into a designated `--output` directory:

1. **Feature 1 — `watermark` subcommand**: standardize a mixed directory of
   raster + RAW images to a uniform `--max-long-edge` constraint (aspect-locked,
   **downscale-only**, zero distortion); apply a full-bleed bottom contrast
   **shadow gradient** (pure black, 100% opacity at the bottom border fading
   linearly to 0% over the bottom 30% of image height); then composite two
   corner-anchored, proportionally-scaled image watermarks (`--mark1` top-right,
   `--mark2` bottom-left), exporting every image as a high-quality JPEG.
   **Compositing order**: resize → shadow → mark1 → mark2 (mark2 sits legibly
   atop the shadow).
2. **Feature 2 — `rename` subcommand**: surface culling + cluster decisions in the
   filename prefix (`Cluster-{X}_Cull-{Y}-OriginalFilename.ext`) by copying RAW
   files (and any matching `.xmp` sidecars) into `--output` under their new names,
   leaving the source untouched.

### Design decisions locked (user-approved)

- **D-Q1 (Feature 1 home)**: a **new filesystem-driven subcommand** that calls
  **shared library functions extracted from `photohelper-export`** (see D1.0).
  `export` stays catalog-driven and is **re-pointed at the same extracted
  functions** in the extraction commit (so there is one pipeline, not two).
- **D-Q2 (Feature 1 formats)**: raster (JPEG/PNG) + **CR3** are processed by
  default. **Other LibRaw RAW (cr2/nef/arw/raf/orf/rw2/dng) is gated behind an
  explicit `--allow-untested-raw` flag AND a post-decode sanity guard** (Q-C);
  without the flag, non-CR3 RAW is skipped with a notice (exit 0). Only CR3 is
  decode-tested this session; the gated formats get a DN + TD with binding
  triggers.
- **D-Q3 (rename `Cull-[Y]` / `Cluster-[X]`)**: `Y` = NIMA score formatted
  **`{:05.2}`** via the existing `format_nima_score_label` (`util.rs:15`), e.g.
  `Cull-07.85` (lexically sortable per DN — see Q-Ledger). `X` = zero-padded
  dedup cluster id, e.g. `Cluster-003`. The capitalized `Cluster-…_Cull-…` form
  (the user's literal spec) is deliberately **different** from `export`'s
  lowercase `cluster-…-cull-…`; the divergence is recorded in a
  `docs/decisions/` note and BOTH commands route through ONE shared prefix
  formatter.
- **D-Q4 (color space — Q-A)**: the compositing target is **plain 8-bit sRGB**.
  RAW for `watermark` is decoded via LibRaw auto-bright sRGB
  (`ProcessOptions::Srgb8`), **not** the filmic `ToneMappingLut` develop ISP;
  raster decodes straight to sRGB8. RAW and raster therefore match → genuinely
  uniform mixed-directory look.
- **D-Q5 (mark fit — Q-B)**: a mark that cannot fit at its spec size is an
  **error** — that image's output JPEG is **not written**, a dedicated counter
  increments, and the run returns a non-zero exit (`EX_PARTIAL_FAIL`) even
  without `--strict`; `--strict` makes it immediately fatal.
- **D-Q6 (mark formats — Q-D)**: `--mark1`/`--mark2` accept **PNG only**
  (transparency is required for corner logos); a non-PNG or unreadable mark is a
  **fatal up-front error** before the parallel loop (mirroring `export.rs:320-331`).
- **D-Q7 (rename metric source — was O3)**: cull score + cluster id come from the
  **catalog** (`DevelopRow`); `.xmp` files are copied **verbatim** (never
  parsed). Reading metrics out of sidecars is explicitly **forbidden this
  session** (it would invoke the `photohelper-sidecar` reader that still carries
  open session-14 Round-3 findings — see Unresolved prior-session item).

### Core invariants (from the spec; non-negotiable, enforced by tests)

- **Non-destructive**: `--source` is strictly read-only; every produced file is
  written only under `--output`. Enforced two ways: (a) `--output` must not equal
  or be nested within `--source` (rejected at arg-resolution; the `watermark`
  walker also prunes the `--output` subtree), and (b) a test asserts the source
  directory's **entry-set + each file's bytes/mtime are unchanged** after a run
  (catching stray files written into `--source`, not just edits to known files).
- **Zero distortion**: aspect ratio is locked on BOTH the base image AND each
  mark asset under every scaling operation (no stretch / squash / skew). A test
  asserts output aspect ratio == input aspect ratio within ±1px. The shadow
  gradient is *generated* (full width × 30% height by construction), proportional
  on every orientation by design.
- **Orientation correctness** (spec "Orientation Agnosticism"): the loaded image
  is visually upright before compositing. RAW is pre-rotated by LibRaw; raster
  EXIF orientation is read and applied after decode so marks anchor to the
  *visual* corners (a portrait phone JPEG is not watermarked sideways).

### What will exist by end-of-session

1. **D1.0 extraction**: `photohelper-export` exposes `pub` building blocks
   (`resize_rgb`, `render_to_jpeg`, `pixmap_to_rgb`, plus `pub`
   `compress_jpeg`/`draw_image_watermark`/`calculate_watermark_position`); the
   existing `export` is re-pointed at them with its integration tests green.
   `TempFileGuard` + a generic `resolve_collisions` move to `cli/util.rs`.
2. `watermark` subcommand (Feature 1) — wired, parallelized, heartbeat, exit-code
   semantics, idempotent re-runs.
3. `rename` subcommand (Feature 2) — catalog-backed, atomic RAW+sidecar copy.
4. `photohelper-export` additions: a PNG/JPEG raster decoder + a unified
   source-image loader (raster vs LibRaw-`Srgb8` dispatch, EXIF-orientation
   applied) returning `(Vec<u8>, w, h)`; a `geometry` module (named validated
   fraction constants + a fallible `MarkPlacement`); a full-bleed bottom shadow
   generator (darkens RGB, preserves alpha 255).
5. Tests: unit (exact-integer geometry, shadow ramp, filename construction) +
   integration (decode-and-assert-pixels on CC0 CR3 + runtime-generated raster
   fixtures with sentinel pixels; non-destructive; idempotency; fail-open/strict).
6. Docs + ergonomics: README quickstart, `just` recipes + wrapper scripts, ledger
   updates, `docs/decisions/` note for the filename-scheme divergence.

### Out of scope (deferred with binding triggers → `TECH-DEBT.md` / discovery)

- **NEF/ARW/RAF/ORF/RW2/DNG decode verification + fixtures**: gated behind
  `--allow-untested-raw` + a runtime dimension-sanity guard; not decode-tested.
  File a new DN (untested non-CR3 RAW decode in the *watermark* decoder — a
  surface **distinct** from DN-014's *ingest* `RAW_EXTS`) + a new TD (acquire CC0
  fixtures + verify LibRaw decode + add integration tests). Binding trigger: first
  session adding a non-Canon camera profile, OR first user report of a non-CR3 RAW
  mis-rendering, OR `2026-12-01`. IDs assigned in D4 as the next free ids after
  the duplicate-ID reconciliation below (do NOT compute by eye — the ledger tail
  has collisions).
- **Watermark configuration via `photohelper.toml` / `ph:` sidecar** (DN-002):
  CLI flags only this session; DN-002 stays open with a cross-reference.
- **Upscaling images smaller than `--max-long-edge`**: **downscale-only**
  (`scale = min(1.0, limit/long_edge)`); small images pass through at native
  size with marks/shadow sized relative to their actual height.
- **JPEG/other marks**: PNG only this session (D-Q6); other formats deferred.
- **Reading rename metrics from sidecars** (D-Q7): forbidden until the session-14
  sidecar findings are remediated.

### Error taxonomy, counters & exit codes (shared contract)

Both subcommands reuse `EX_PARTIAL_FAIL=2` / `EX_STRICT_FAIL=1` (`main.rs:115-117`)
and surface a summary line like `export` (`export.rs:173-183`). Each defines its
OWN stats struct (export's is catalog-specific):

- **`WatermarkStats`**: `walked`, `written`, `skipped_unsupported` (unknown ext, or
  non-CR3 RAW without `--allow-untested-raw`), `skipped_existing` (output exists,
  no `--force`), `decode_failed`, `mark_doesnt_fit`, `errored`.
  `total_failures = decode_failed + mark_doesnt_fit + errored`.
- **`RenameStats`**: `matched`, `renamed`, `sidecar_copied`, `sidecar_absent`,
  `sidecar_copy_failed`, `file_missing`, `errored`.
  `total_failures = sidecar_copy_failed + file_missing + errored`.
- **Exit**: `total_failures > 0` → `EX_PARTIAL_FAIL` (2); with `--strict`, the
  FIRST failure of ANY counted class is immediately fatal → `EX_STRICT_FAIL` (1).
  `skipped_*` never fail (exit 0) unless `--strict` is set AND the skip is a
  decode/fit/copy failure (a deliberate gate-skip is not a failure).

### How each deliverable is tested (meaningful assertions only)

- **Dual-mark geometry** (pure fn, unit): assert EXACT integer `mark_w/mark_h/x/y`
  for landscape, portrait, square, a **wide-logo** asset (mark wider than tall),
  and a **tiny pass-through** image; assert the **fit-guard returns `Err`** (skip
  under non-strict, fatal under strict) when a mark cannot fit; assert `.max(1)`
  floors prevent any zero-size mark.
- **Shadow gradient** (pure `shadow_alpha_ramp(H)` + composite): band height ==
  `round(0.30*H)`; ramp monotonic with **pinned denominator**; exact endpoints
  (`[H-1]==255`, `[H-band]==0`); the row above the band is **bit-identical** to
  source; the **final demultiplied 3-channel buffer** at a mid-band row is
  partially darkened (catches the premultiply/cliff bug); base alpha stays 255;
  band-height-zero guard for tiny `H`.
- **Color uniformity**: composite a mid-gray patch via the RAW(`Srgb8`) and raster
  paths; assert post-composite values match within tolerance.
- **Raster decode**: runtime-generate a JPEG and a PNG with a known **sentinel
  pixel block**; assert decoded dims + that the sentinel survives to the expected
  output location (distinguishes "decoded" from "blank frame of right size");
  truncated JPEG → counted `decode_failed`, not a partial render. A portrait JPEG
  with `Orientation=6` → mark lands at the **visual** top-right.
- **Zero distortion / downscale-only**: `out_w/out_h ≈ in_w/in_h` within ±1px;
  long edge == max for larger-than-constraint inputs; a **sub-limit** image is
  emitted at native size (NOT upscaled).
- **Non-destructive**: snapshot the `--source` entry-set + each file's bytes+mtime
  before; assert unchanged (incl. no new files) after both subcommands;
  `--output` and `--source` are distinct dirs; reject `--output` nested in
  `--source`.
- **Idempotency**: run `watermark` twice; output count stable (walker excludes
  `--output`; `skipped_existing` without `--force`).
- **Best-effort RAW dispatch**: corrupt/undecodable RAW → `decode_failed`,
  skip+warn, exit 2; with `--strict` → exit 1; non-CR3 RAW without
  `--allow-untested-raw` → `skipped_unsupported`, exit 0, notice.
- **Rename construction** (pure fn): exact `Cluster-{X}_Cull-{Y}-{stem}.{ext}`
  including named `None`-cluster/`None`-score **sentinels** (assert sort order),
  `{:05.2}` width, cluster id ≥1000 / negative handling, NaN/inf score filtered;
  **stem sanitization** (path separators, NUL, control chars, overlong);
  collision suffixing incl. **case-insensitive** collision (darwin); RAW+sidecar
  get the **same** suffix.
- **Rename integration**: seed a catalog with known cull+cluster; assert exact
  new RAW names, that each `.xmp` is copied + renamed to
  `new_raw.with_extension("xmp")`, source untouched, a deleted-source row →
  `file_missing`, a sidecar-copy failure → no half-renamed output + distinct
  counter.

### Checkpoints that fire

1. **Plan-review** (in progress): Round 1 ✓ → remediate (this v2) → Round 2.
   Artifacts: `docs/code-reviews/session-15-plan-round{1,2}.md`.
2. **Sub-component review** at the D1.0 extraction + `photohelper-export`
   geometry/shadow boundary (re-pointed `export` must stay green).
3. **Sub-component review** at the `rename` subcommand boundary.
4. **Session-end review**: Round 1 → Round 2 → CLEAN, then ship PR to `main`.

### Unresolved prior-session item surfaced (NOT planned on top of)

- **Session-14 session-end review** (`docs/code-reviews/session-14-implementation-round3.md`)
  retained **15 verified findings** (2 CRITICAL + 9 HIGH + 4 MEDIUM + 2 LOW raw;
  3 hallucinations discarded, `discard_rate 0.16`) in `photohelper-sidecar`
  (`conflict.rs` TOCTOU mtime; `writer.rs` state-machine duplication, self-closing
  `rdf:RDF`, `SystemTime::from` panic, silent permission/format swallowing), with
  **no Round-4 CLEAN artifact**, yet PR #15 (merge `d4575fca`,
  `session-14/xmp-library-upgrade`) merged to `main`. Note: there are **two**
  "Merge pull request #15" commits in history (the other is `17da1eb9`,
  session-12) — the SHA disambiguates. Session 15 **does not touch** that code:
  Feature 2 copies `.xmp` **verbatim** (byte copy + rename) and reads metrics
  from the catalog (D-Q7), never invoking the writer/conflict/reader logic.
  Flagged for a future sidecar-focused remediation session.
- **Ledger desync** (corrected at session-start): `SESSION-STATE.md` had pointed
  at session 14 mid-implementation while sessions 13 & 14 (PRs #14, #15) had
  merged.

### Open ambiguities — resolved

- **O1 — gradient margins vs full-bleed**: RESOLVED — the shadow is **full-bleed**
  (no horizontal margin, flush to the bottom edge); the 4.6% margin applies to the
  **marks only**. (Operator note: "covering all the footer, from one side to the
  other.")
- **O2 — visual reference**: `image_b781b9.jpg` was not provided; implementation
  follows the textual spec; the named geometry constants are the single
  adjustment point if the exemplar differs.
- **O3 — rename metric source**: RESOLVED as **D-Q7** (catalog only; sidecars
  copied verbatim).

---

## Deliverables

### D0 — Housekeeping (first chore commit)

- `rg`/`grep`-confirm that nothing in `Cargo.toml`/`mod` declarations references
  the untracked scratch artifacts, then remove them:
  `crates/photohelper-sidecar/test_quick_xml.rs`,
  `crates/photohelper-sidecar/test_quick_xml/`, `diff.txt`.
- (Session-start already corrected the `SESSION-STATE.md` pointer.)

### D1.0 — Extract shared rendering primitives (FIRST code commit; regression-guarded)

The plan-review CRITICAL (Theme A): `export_photo`
(`crates/photohelper-export/src/lib.rs:177-347`) is a private monolith; the
"reuse" the rest of D1 depends on does not exist yet. Before any new feature code:

- Extract, in `photohelper-export`:
  - `pub fn resize_rgb(rgb: &[u8], w: u32, h: u32, long_edge: Option<u32>, downscale_only: bool) -> Result<tiny_skia::Pixmap, ExportError>` (lifts `lib.rs:235-297`; `downscale_only` adds the `scale = min(1.0, …)` clamp — Theme K).
  - `pub fn render_to_jpeg(rgb: &[u8], w, h, opts: &RenderOptions) -> Result<Vec<u8>, ExportError>` = build pixmap (alpha 255) → `resize_rgb` → optional shadow → composite marks/badges → `pixmap_to_rgb` → `compress_jpeg`.
  - `pub fn pixmap_to_rgb(pixmap: &Pixmap) -> Vec<u8>` (lifts the demultiply loop `lib.rs:311-335`, also currently duplicated in a test).
  - Make `compress_jpeg` (`:664`), `draw_image_watermark` (`:478`),
    `calculate_watermark_position` (`:349`) `pub`.
- **Re-point `export_photo`** at `render_to_jpeg` (after its existing
  `decode_image(Linear16)` + `ToneMappingLut` step — export keeps its filmic look;
  `RenderOptions` defaults reproduce export's current behavior: long-edge badge
  basis, no shadow, upscale-allowed).
- Move `TempFileGuard` (`export.rs:186-219`) + a generic
  `resolve_collisions<F: Fn(&str)->String>` (extracted from `export.rs:271-314`,
  keeping the macOS/Windows case-fold) into `crates/photohelper-cli/src/commands/util.rs`.
- **Regression guard**: the existing `export` integration tests (`cli.rs`) MUST
  stay green after this commit. Sub-component review fires here.

### D1 — Raster loader, geometry, shadow (Feature 1 core lib)

- **D1a — Source loader**: add `image` as a **new dependency of `photohelper-export`**
  (`default-features=false, features=["jpeg","png"]`; it is currently only a
  dev-dep of `photohelper-cli` — Theme Q6), verify MSRV-1.88 + `cargo audit`.
  `pub fn load_source_image(path, allow_untested_raw) -> Result<LoadedImage, …>`
  returning `(Vec<u8> rgb8, w, h)`:
  - raster (jpeg/png): `image` decode → `to_rgb8`, **apply EXIF orientation**.
  - RAW: `decode_image(path, ProcessOptions::Srgb8)` (plain sRGB, D-Q4); CR3
    always; non-CR3 RAW only if `allow_untested_raw`, then a **post-decode
    dimension-sanity guard** (reject absurd dims as a decode error).
  - dispatch via an exhaustive `SourceKind::classify(path) -> Option<…>`
    (`eq_ignore_ascii_case`); `None` = unsupported extension, a **distinct**
    outcome from decode-failure so logs/counters can tell them apart (Theme O/F).
- **D1b — Geometry module** (named, validated): consts `MARK1_HEIGHT_FRAC=0.14`,
  `MARK2_HEIGHT_FRAC=0.13`, `MARK_MARGIN_FRAC=0.046`, `SHADOW_BAND_FRAC=0.30` in
  ONE module. `MarkPlacement::fit(target,(mw,mh),height_frac,margin_frac) ->
  Result<MarkPlacement, GeometryError>`: `mark_h=round(H*f).max(1)`,
  `scale=mark_h/mh`, `mark_w=round(mw*scale).max(1)`,
  `margin_x=round(W*0.046)`, `margin_y=round(H*0.046)`; top-right
  `(W-margin_x-mark_w, margin_y)`, bottom-left `(margin_x, H-margin_y-mark_h)`;
  returns `Err(MarkDoesNotFit{…})` when an origin would underflow or a mark
  exceeds bounds (u32 coords make negatives unrepresentable).
- **D1c — Shadow gradient**: `shadow_alpha_ramp(H) -> Vec<u8>` (len
  `round(0.30*H)`, pinned denominator, monotonic 255→0). Composite as a **color
  operation that keeps destination alpha 255**: `out_rgb = base_rgb*(1 - t)`,
  `t = ramp[row]/255` — NOT a write to the pixmap alpha channel (Theme C).
  Full-bleed (all columns). `None` band for `H` too small to yield ≥1 row.
- **D1d — Composite**: parametrize the existing draw path with
  `BadgeSizeBasis { LongEdge(Scale) | Height(f32) }` + explicit `margin_frac`
  (Theme R — no new module duplicating scale/bounds/blend); enforce
  resize→shadow→mark1→mark2 ordering; a `MarkDoesNotFit` is propagated (D-Q5),
  not silently clipped.
- **D1e — Unit tests** per the geometry + shadow rows in the test list above.
- **Sub-component review** fires (folded with D1.0).

### D2 — `watermark` subcommand (Feature 1 wiring)

- **D2a — `WatermarkArgs`**: `--source <DIR>` (required), `--mark1 <FILE>`
  (required, PNG), `--mark2 <FILE>` (required, PNG), `--max-long-edge <u32>`
  (required; reuse `validate_long_edge`, ≥16), `--output <DIR>` (required),
  `--allow-untested-raw` (bool), `--force` (bool), `--strict` (bool). JPEG quality
  is a fixed `const WATERMARK_JPEG_QUALITY: u8` (no `--quality` flag — Theme R).
- **D2b — Setup**: canonicalize `--source`/`--output`; **reject `--output` equal
  to / nested within `--source`**; up-front `--output` writability probe (reuse
  export's, `export.rs:228-244`); **preload both PNG marks fatally up-front**
  (D-Q6); build a deterministic walked-file list (sorted) **excluding the
  `--output` subtree** (Theme K).
- **D2c — Pipeline** (rayon + heartbeat, `"watermark: …"` label): per file
  `load_source_image` → `render_to_jpeg` (downscale-only, shadow + mark1 + mark2)
  → temp-then-atomic-rename into `--output`. Per-file fail-open per the error
  taxonomy; `--strict` fatal on first failure.
- **D2d — Integration tests** per the raster/decode/orientation/zero-distortion/
  non-destructive/idempotency/dispatch rows above (decode outputs + sentinel
  pixels; no `exists()+len>0` non-tests).

### D3 — `rename` subcommand (Feature 2)

- **D3a — `RenameArgs`**: `--source <DIR>` (required), `--output <DIR>`
  (required), `--force`, `--strict`; catalog via the existing global flag.
- **D3b — Selection + names**: query
  `all_photos_with_cull_scores(MODEL_SLUG, CLIP_MODEL_SLUG)` → `DevelopRow`;
  canonicalize `--source` and filter rows by **canonical path-component prefix**
  (not string `starts_with`); per row, **existence precheck** (`file_missing` if
  gone — Theme N). Build the name via a shared, **sanitizing** `RenamedFilename`
  builder (Theme D/I/O): `Cluster-{X}_Cull-{Y}-{sanitized_stem}.{ext}`, `X`/`Y`
  via the shared prefix formatter (`{:05.2}` score, named `None` sentinels),
  reject path-separators/NUL/control in the stem, cap length vs `NAME_MAX`,
  validate every destination with `canonicalize_within` containment under
  `--output` (Theme D). O(1) collision suffix via the shared `resolve_collisions`
  (same suffix applied to RAW **and** sidecar).
- **D3c — Atomic unit copy**: copy RAW + its `<stem>.xmp` sidecar (Theme H —
  extension-replaced only) as a **unit**: both to temps under `--output`, commit
  both renames only after both succeed; on sidecar failure drop the RAW temp via
  `TempFileGuard`. Output sidecar = `new_raw_path.with_extension("xmp")`. Distinct
  counters (`sidecar_copied`/`sidecar_absent`/`sidecar_copy_failed`).
- **D3d — Tests** per the rename construction + integration rows above.
- **Sub-component review** fires here.

### D4 — Docs, scripts, ledgers

- README quickstart for `watermark` + `rename`; `just watermark` / `just rename`
  recipes + wrapper scripts under `scripts/` (mirror `scripts/photohelper-*.sh`).
- `docs/decisions/NNNN-rename-filename-scheme.md` recording the capitalized
  `Cluster-…_Cull-…` divergence from `export`'s lowercase scheme + the shared
  formatter.
- Ledger updates: SESSION-STATE (component progress), HANDOFF checkpoint;
  **discovery-notes**: first **reconcile the pre-existing duplicate IDs**
  (DN-029 at `:241`/`:329`; DN-033 at `:284`/`:305` — renumber the later
  collisions to the next free ids), THEN file the new untested-RAW DN; cross-ref
  DN-002, and reconcile DN-036 **with a note** (capability delivered in
  `watermark`; `export`-pipeline integration of dynamic badges remains the
  DN-036/`export_photo` trigger — do NOT hard-close), and reword the DN-014
  cross-ref (DN-014 governs *ingest* `RAW_EXTS`; the new DN governs the *watermark*
  decoder — neither trigger subsumes the other). **TECH-DEBT**: file the NEF/ARW
  fixtures+verification TD with an in-source `TD-N` label + binding trigger (next
  free TD id at filing; ledger tail ≈ TD-040 — confirm, don't assume).
