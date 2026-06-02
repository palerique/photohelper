# Session 15 — Dual-Mark Watermarking & Metadata-Driven RAW Rename

> **Top block = the session contract.** Authored at session-start; the detailed
> deliverable spec below is a FIRST DRAFT to be hardened through plan-review
> Round 1 → remediate → Round 2 → remediate before any implementation code lands.

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
   zero distortion); apply a full-width bottom contrast **shadow gradient**
   (pure black, 100% opacity at the bottom border fading linearly to 0% over the
   bottom 30% of image height); then composite two corner-anchored,
   proportionally-scaled image watermarks (`--mark1` top-right, `--mark2`
   bottom-left), exporting every image as a high-quality JPEG. **Compositing
   order**: resize → shadow → mark1 → mark2 (mark2 sits legibly atop the shadow).
2. **Feature 2 — `rename` subcommand**: surface culling + cluster decisions in the
   filename prefix (`Cluster-{X}_Cull-{Y}-OriginalFilename.CR3`) by copying RAW
   files (and any matching `.xmp` sidecars) into `--output` under their new names,
   leaving the source untouched.

### Design decisions locked at session-start (user-approved)

- **D-Q1 (Feature 1 home)**: Feature 1 ships as a **new filesystem-driven
  subcommand** that **reuses `photohelper-export`'s rendering internals**
  (resize / composite / JPEG-encode) promoted to shared library functions. The
  existing `export` subcommand stays catalog-driven and is **not** overloaded
  with a second input model.
- **D-Q2 (Feature 1 formats)**: Feature 1 accepts **raster (JPEG/PNG) + all
  LibRaw-decodable RAW** (cr3/cr2/nef/arw/raf/orf/rw2/dng) **best-effort**. Only
  **CR3** is decode-tested this session (we have CC0 CR3 fixtures; none for the
  others). NEF/ARW/… ship as untested code paths gated behind a new DN + TD with
  a binding trigger to add fixtures + verify decode. Per-file decode failure is
  fail-open (skip + warn) unless `--strict`.
- **D-Q3 (rename `Cull-[Y]`)**: `Y` = the **zero-padded NIMA aesthetic score**
  (e.g. `Cull-07.85`) for correct lexical sorting in any OS file explorer
  (matches the leading-zero label convention from DN-029). `X` = the
  **zero-padded dedup cluster id** (e.g. `Cluster-003`).

### Core invariants (from the spec; non-negotiable, enforced by tests)

- **Non-destructive**: `--source` is strictly read-only; every produced file
  (resized JPEGs, converted RAWs, renamed copies, copied sidecars) is written
  only under `--output`. A test asserts source bytes + mtimes are unchanged.
- **Zero distortion**: aspect ratio is locked on BOTH the base image AND each
  mark asset under every scaling operation (no stretch / squash / skew). A test
  asserts output aspect ratio == input aspect ratio within ±1px rounding. The
  shadow gradient is *generated* (not asset-scaled), spanning full width × 30%
  height by construction, so it is proportional on every orientation by design.

### What will exist by end-of-session

1. `watermark` subcommand (Feature 1) — wired into the CLI, parallelized, with a
   liveness heartbeat, exit-code semantics consistent with `export`.
2. `rename` subcommand (Feature 2) — catalog-backed, copying RAW + sidecars.
3. `photohelper-export` additions: a raster (JPEG/PNG) decode entry + a unified
   source-image loader (raster vs LibRaw dispatch) + a full-width bottom shadow
   gradient generator (black, 100%→0% over bottom 30% height) + a height-relative
   dual-mark geometry/composite module (14% / 13% of target height, 4.6% margins,
   top-right / bottom-left), all shared with the new subcommand.
4. Tests: unit (geometry, filename construction) + integration (end-to-end on CC0
   CR3 + synthetic raster fixtures; non-destructive + zero-distortion assertions).
5. Docs + ergonomics: README quickstart for both subcommands, `just` recipes +
   wrapper scripts, and ledger updates (SESSION-STATE / HANDOFF / TECH-DEBT /
   discovery-notes).

### Out of scope (deferred with binding triggers → `TECH-DEBT.md` / discovery)

- **NEF/ARW/RAF/ORF/RW2/DNG decode verification + fixtures**: shipped best-effort,
  untested. New DN (untested non-CR3 RAW decode in the watermark path) + new TD
  (acquire CC0 fixtures + verify LibRaw decode + add integration tests). Binding
  trigger: next session adding a non-Canon camera profile, or first user bug
  report of a non-CR3 RAW failing in `watermark`. Cross-references DN-014.
- **Watermark configuration via `photohelper.toml` / `ph:` sidecar** (DN-002):
  Feature 1 uses CLI flags only this session; tiered config stays deferred —
  DN-002 remains open with a cross-reference.
- **Upscaling images smaller than `--max-long-edge`**: default is **downscale-only**
  (never enlarge, to avoid quality loss); small images pass through at native
  size with marks still sized relative to their actual height. Recorded as a
  decision, revisitable on request.
- **Text watermarks / config tiers in the new subcommand**: Feature 1 is
  image-mark-only per the spec; the existing `export --watermark` text path is
  untouched.

### How each deliverable is tested (meaningful assertions only)

- **Dual-mark geometry**: unit tests assert EXACT computed mark width/height and
  x/y placement for landscape, portrait, and square targets — for both marks —
  including aspect-lock width derivation and 4.6% margins. (No `toBeDefined`-style
  asserts; assert the integer coordinates.)
- **Raster decode**: integration test decodes a known-dimension JPEG and PNG
  fixture and asserts decoded width/height/pixel sample.
- **Shadow gradient**: unit test asserts the band height == `round(0.30*H)`;
  assert the bottom-border row alpha == full (≈255 black contribution) and the
  top-of-band row alpha == 0; assert full-width coverage (left + right edge
  columns at the bottom are darkened); assert a pixel one row above the band is
  unchanged. Integration test samples a bright synthetic image and asserts the
  bottom-center pixel is darkened while the top-center pixel is not.
- **Zero distortion**: assert `out_w/out_h ≈ in_w/in_h` within ±1px and that the
  long edge equals `--max-long-edge` (for images larger than the constraint).
- **Non-destructive**: snapshot source file bytes + mtime before, assert
  unchanged after running both subcommands.
- **Rename construction**: unit tests for `Cluster-{X}_Cull-{Y}-{stem}.{ext}`
  including unclustered/unscored sentinels, zero-padding, and collision suffixing;
  integration test seeds a catalog with known cull+cluster values, runs `rename`,
  asserts new RAW names, that each `.xmp` sidecar is copied + renamed to match,
  and that source files are untouched.

### Checkpoints that fire

1. **Plan-review** (now): Round 1 → remediate → Round 2 → remediate, via the
   `eight-agent-review` suite. Artifacts:
   `docs/code-reviews/session-15-plan-round{1,2}.md`.
2. **Sub-component review** at the `photohelper-export` dual-mark module boundary
   (Feature 1 core lib lands).
3. **Sub-component review** at the `rename` subcommand boundary (Feature 2 lands).
4. **Session-end review**: Round 1 → remediate → Round 2 → CLEAN, then ship PR to
   `main`. Artifacts: `docs/code-reviews/session-15-round{1,2}.md`.

### Unresolved prior-session item surfaced (NOT planned on top of)

- **Session-14 Round-3 review** (`docs/code-reviews/session-14-implementation-round3.md`)
  recorded **2 CRITICAL + 9 HIGH** still-open findings in `photohelper-sidecar`
  (`conflict.rs` TOCTOU mtime; `writer.rs` state-machine duplication, self-closing
  `rdf:RDF`, `SystemTime::from` panic, silent permission/format error swallowing),
  with **no Round-4 CLEAN artifact**, yet PR #15 merged to `main`. Session 15
  **does not touch** that code: Feature 2 copies `.xmp` sidecars **verbatim**
  (byte copy + rename) and never invokes the writer/conflict logic. Flagged here
  + in discovery for a future sidecar-focused remediation session; not a blocker
  for session 15's surface.
- **Ledger desync**: `SESSION-STATE.md` still pointed at session 14 mid-implementation
  while sessions 13 & 14 had merged (PRs #14, #15). Corrected at session-start
  (header pointer updated to session 15).

### Open ambiguities to resolve in plan-review

- **O1 — Gradient margins vs full-bleed**: the spec's "Dynamic Margins" line
  states 4.6% padding applies to "both marks **and the gradient**," yet the
  gradient is also "full-width … across the entire bottom" with "100% opacity at
  the **exact bottom border**" (operator note: "covering all the footer, from one
  side to the other"). These are mutually exclusive. **Default**: gradient is
  **full-bleed** (no horizontal margin, anchored flush to the bottom edge); the
  4.6% margin applies to the **marks only**. Plan-review confirms or overrides.
- **O2 — Visual reference unavailable**: the spec cites `image_b781b9.jpg` as the
  shadow exemplar; it was not provided to this session. Implementation follows the
  textual spec (full-bleed black, 100%→0% over bottom 30%). If the reference
  differs, the geometry constants are the single point of adjustment.
- **O3 — Rename metric source (catalog vs sidecar)**: the updated spec says
  `rename` "reads evaluation metrics (e.g., **from existing XMP sidecars**)."
  **Default for this session**: read cull score + cluster id from the **catalog**
  (`DevelopRow`), which is the canonical store and keeps session 15 clear of the
  `photohelper-sidecar` reader code that still carries the open session-14 Round-3
  CRITICALs. The `.xmp` files are still **copied verbatim** (no parsing). Reading
  metrics *out of* sidecars (catalog-free operation) is recorded as a candidate
  enhancement; plan-review decides whether to pull it into scope (and, if so,
  whether the session-14 sidecar findings must be remediated first).

---

## Deliverables (FIRST DRAFT — to be hardened in plan-review)

> The breakdown below is the starting point for plan-review Round 1. Sequencing,
> type design, error taxonomy, and test rows will be refined per review findings
> before any implementation commit.

### D0 — Housekeeping (first chore commit)

- Remove untracked session-14 scratch artifacts after confirming they are not
  referenced by the build: `crates/photohelper-sidecar/test_quick_xml.rs`,
  `crates/photohelper-sidecar/test_quick_xml/`, `diff.txt`.
- (Session-start already updated the `SESSION-STATE.md` current-session pointer.)

### D1 — `photohelper-export`: raster decode + dual-mark composite (Feature 1 core)

- **D1a — Source loader**: add a raster decoder (JPEG/PNG) and a unified
  `load_source_image(path) -> RgbImage` dispatching raster vs LibRaw by extension.
  Decide `image`-crate promotion (dev-dep → dep) vs a narrower decoder during
  plan-review. RAW path reuses `photohelper_raw::decode::decode_image(..)` +
  the existing tone-map ISP, exactly as `export` does today.
- **D1b — Dual-mark geometry**: pure function(s) computing, for a mark of native
  size `(mw, mh)` on target `(W, H)` at height fraction `f`:
  `mark_h = round(H*f)`, `scale = mark_h/mh`, `mark_w = round(mw*scale)`
  (aspect-locked); `margin_x = round(W*0.046)`, `margin_y = round(H*0.046)`;
  top-right `(W - margin_x - mark_w, margin_y)`, bottom-left
  `(margin_x, H - margin_y - mark_h)`. `f = 0.14` (mark1) / `0.13` (mark2).
  Fit-guard: omit + warn (or error under `--strict`) if a mark cannot fit.
- **D1c — Shadow gradient**: a generator filling the bottom `round(0.30*H)` rows
  full-width with pure black (0,0,0) at a per-row alpha ramping linearly from 255
  at the bottom border to 0 at the top of the band, composited over the resized
  base (full-bleed; no margins — see Open ambiguity O1).
- **D1d — Composite**: aspect-locked mark scaling (Bicubic) + alpha blend via
  `tiny_skia`, reusing the existing badge draw path where possible; enforces the
  resize → shadow → mark1 → mark2 ordering.
- **D1e — Unit tests** for D1b geometry (landscape/portrait/square × both marks)
  and D1c shadow (band height, alpha endpoints, full-width coverage, above-band
  pixel untouched).
- **Sub-component review** fires here.

### D2 — `watermark` subcommand (Feature 1 wiring)

- **D2a — `WatermarkArgs`**: `--source <DIR>` (required), `--mark1 <FILE>`
  (required), `--mark2 <FILE>` (required), `--max-long-edge <u32>` (required,
  ≥16), `--output <DIR>` (required), `--quality <u8>` (default high), `--force`,
  `--strict`. (Spec wrote `--max_long_edge`; we use kebab-case per repo
  convention; an underscore alias can be added if desired.)
- **D2b — Pipeline**: walk `--source` read-only; per file dispatch raster/RAW
  decode; downscale to `--max-long-edge` (aspect-locked); apply the bottom shadow
  gradient (D1c); composite mark1 + mark2 per D1b/d (order: shadow → mark1 →
  mark2); encode high-quality JPEG into `--output`. Rayon parallel + heartbeat
  + per-file fail-open (fatal under `--strict`), mirroring `export`.
- **D2c — Mark preloading**: load each mark once; PNG (alpha) primary, JPEG marks
  TBD in plan-review; aspect-locked.
- **D2d — Integration tests**: CC0 CR3 + synthetic JPEG + synthetic PNG →
  output exists, long edge == max, aspect preserved, source unchanged.

### D3 — `rename` subcommand (Feature 2)

- **D3a — `RenameArgs`**: `--source <DIR>` (required), `--output <DIR>`
  (required), `--force`, `--strict`; catalog via the existing global flag.
- **D3b — Name construction**: query `all_photos_with_cull_scores(MODEL_SLUG,
  CLIP_MODEL_SLUG)` → `DevelopRow`; filter to `--source`; build
  `Cluster-{X}_Cull-{Y}-{stem}.{ext}` with `X` = zero-padded `dedup_cluster_id`
  (sentinel for `None`), `Y` = zero-padded `nima_score` (sentinel for `None`);
  O(1) collision suffixing (reuse `export`'s approach).
- **D3c — Copy**: byte-copy RAW (temp + atomic rename via `TempFileGuard`) into
  `--output`; copy any matching `.xmp` sidecar(s) renamed to match the new RAW
  name. Handle both `<stem>.xmp` and `<name.ext>.xmp` conventions (confirm
  `SidecarPath` convention in plan-review).
- **D3d — Tests**: unit (name construction + sentinels + collisions) + integration
  (seeded catalog + RAW + XMP → new names, sidecar match, source untouched).
- **Sub-component review** fires here.

### D4 — Docs, scripts, ledgers

- README quickstart for `watermark` + `rename`; `just watermark` / `just rename`
  recipes + wrapper scripts under `scripts/`.
- Ledger updates: SESSION-STATE (component progress), HANDOFF checkpoint,
  TECH-DEBT (new NEF/ARW TD), discovery-notes (new untested-RAW DN; advance/close
  DN-036; cross-ref DN-002 + DN-014).
