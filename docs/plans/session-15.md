# Session 15 — Dual-Mark Watermarking & Metadata-Driven RAW Rename

> **Top block = the session contract.** Hardened to **v3** via plan-review
> Round 1 (7C+10H+3M+2L, 19 themes) + Round 2 (2C+7H+6M+3L, all v2-introduced
> regressions) + four user decisions. Round 3 fires next (R2 surfaced
> CRITICAL-class regressions). Artifacts: `docs/code-reviews/session-15-plan-round{1,2}.md`.

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
   **Compositing order**: resize → shadow → mark1 → mark2.
2. **Feature 2 — `rename` subcommand**: surface culling + cluster decisions in the
   filename prefix (`Cluster-{X}_Cull-{Y}-OriginalFilename.ext`) by copying RAW
   files (and any matching `.xmp` sidecars) into `--output` under their new names,
   leaving the source untouched.

### Design decisions locked (user-approved)

- **D-Q1 (Feature 1 home)**: a **new filesystem-driven subcommand** calling
  **shared library functions extracted from `photohelper-export`** (D1.0); `export`
  is re-pointed at the same functions (one pipeline, not two).
- **D-Q2 (Feature 1 formats)**: raster (JPEG/PNG) + **CR3** by default. Other
  LibRaw RAW (cr2/nef/arw/raf/orf/rw2/dng) is gated behind `--allow-untested-raw`
  **plus a post-decode sanity guard**; without the flag it is skipped with a
  notice (exit 0). **The sanity guard bounds geometry (dimensions + 3-channel)
  only — it cannot detect silent color corruption (channel swap, wrong WB) on an
  untested format; that residual risk is exactly why these formats stay gated and
  decode-unverified** (RT-K). Only CR3 is decode-tested; gated formats get a
  DN + TD with binding triggers.
- **D-Q3 (rename `Cull-[Y]` / `Cluster-[X]`)**: `Y` = NIMA score formatted
  **`{:05.2}`** via the existing `format_nima_score_label` (`util.rs:15`), e.g.
  `Cull-07.85`. `X` = zero-padded dedup cluster id, e.g. `Cluster-003`. The
  capitalized `Cluster-…_Cull-…` form (user's literal spec) deliberately differs
  from `export`'s lowercase `cluster-…-cull-…`; the divergence is recorded in a
  `docs/decisions/` note and BOTH commands route through ONE shared prefix
  formatter.
- **D-Q4 (color space — Q-A)**: the compositing target is **plain 8-bit sRGB**.
  RAW for `watermark` is decoded via LibRaw auto-bright sRGB
  (`ProcessOptions::Srgb8`, `decode.rs:138`), **not** the filmic `ToneMappingLut`
  develop ISP; raster decodes to sRGB8. Both feed `render_to_jpeg` as
  display-referred 8-bit RGB → uniform look.
- **D-Q5 (mark fit — Q-B)**: a mark that cannot fit at its spec size is an
  **error** — no output JPEG written, a dedicated `mark_doesnt_fit` counter
  increments, the run returns `EX_PARTIAL_FAIL` even without `--strict`;
  `--strict` makes the first such failure immediately fatal.
- **D-Q6 (mark formats — Q-D)**: `--mark1`/`--mark2` accept **PNG only**; a
  non-PNG or unreadable mark is a **fatal up-front error** before the parallel
  loop (mirroring `export.rs:320-331`).
- **D-Q7 (rename metric source)**: cull score + cluster id come from the
  **catalog** (`DevelopRow`); `.xmp` files are copied **verbatim** (never
  parsed). Reading metrics out of sidecars is **forbidden this session** (it would
  invoke the `photohelper-sidecar` reader that still carries open session-14
  findings). **`rename` MUST NOT reuse `run_export`'s per-photo body**, which
  calls `read_xmp` for rating (`export.rs:374-401`) — that would drag the sidecar
  reader back in (RT-O); `rename` is an independent driver.

### Core invariants (from the spec; non-negotiable, enforced by tests)

- **Non-destructive**: `--source` is strictly read-only; every produced file is
  written only under `--output`. Enforced: (a) `--output` must not equal or be
  nested within `--source` (rejected at arg-resolution; the `watermark` walker
  also prunes the `--output` subtree by **canonical** path / `same_file`, not a
  lexical prefix, and uses `follow_links(false)` so a symlink inside `--source`
  pointing into `--output` cannot cause re-ingestion — RT-G); (b) each output
  destination is validated by **lexical containment** — `--output` is canonicalized
  once at setup (it exists), then `output_canonical.join(sanitized_name)` must have
  `parent() == output_canonical` and a single-component name (the stem sanitizer
  already rejects separators/`..`, so this is sufficient and stronger than, and
  replaces, the infeasible `canonicalize_within`-on-a-nonexistent-leaf approach —
  RT-A); (c) a test asserts the `--source` entry-set + each file's bytes/mtime are
  unchanged (catching stray files, not just edits).
- **Zero distortion**: aspect ratio is locked on BOTH the base image AND each mark
  asset under every scaling operation. The shadow gradient is generated (full
  width × 30% height), proportional on every orientation by design.
- **Orientation correctness** (spec "Orientation Agnosticism"): the loaded image is
  visually upright before compositing. RAW is pre-rotated by LibRaw; raster EXIF
  orientation is read and applied after decode. A malformed/unsupported orientation
  tag is a **defined outcome**, not a silent no-op (see D1a / error taxonomy).

### Error taxonomy, counters & exit codes (shared contract)

Both subcommands reuse `EX_PARTIAL_FAIL=2` / `EX_STRICT_FAIL=1` (`main.rs:115-117`)
and surface a summary line like `export` (`export.rs:173-183`). Each has its OWN
stats struct:

- **`WatermarkStats`**: `walked`, `written`, `skipped_unsupported` (unknown ext, or
  non-CR3 RAW without `--allow-untested-raw`), `skipped_existing` (output exists, no
  `--force`), `decode_failed` (incl. failed dimension/3-channel sanity guard and
  failed/malformed EXIF-orientation apply), `mark_doesnt_fit`, `errored`.
  `total_failures = decode_failed + mark_doesnt_fit + errored`.
- **`RenameStats`**: `matched`, `renamed`, `sidecar_copied`, `sidecar_absent`,
  `sidecar_copy_failed`, `file_missing`, `errored` (the catch-all; its one distinct
  source here is a destination-containment rejection — RT-O).
  `total_failures = sidecar_copy_failed + file_missing + errored`.
- **Exit (RT-F, restated cleanly)**: `skipped_unsupported` and `skipped_existing`
  **never** contribute to the exit code, even under `--strict` (they are deliberate
  gate-skips). The **failure** counters (`decode_failed` / `mark_doesnt_fit` /
  `sidecar_copy_failed` / `file_missing` / `errored`) drive `EX_PARTIAL_FAIL` (2)
  whenever `total_failures > 0`; under `--strict`, the FIRST such failure is
  immediately fatal → `EX_STRICT_FAIL` (1).

### How each deliverable is tested (meaningful assertions only)

- **Dual-mark geometry** (pure fn, unit): EXACT integer `mark_w/mark_h/x/y` for
  landscape, portrait, square, a **wide-logo** asset, and a **tiny pass-through**
  image; the fit-guard returns `Err(GeometryError::MarkDoesNotFit{…})` (→ skip
  under non-strict but still `EX_PARTIAL_FAIL`; fatal under strict) when a mark
  cannot fit; `.max(1)` floors prevent any zero-size mark; assert
  `mark2_y >= H - shadow_band_height` (mark2 sits inside the shadow band).
- **Shadow gradient**: `shadow_alpha_ramp(H)` len == `round(0.30*H)`, monotonic
  with pinned denominator, exact endpoints (`[H-1]==255`, `[H-band]==0`); the row
  above the band is bit-identical to source; the **final demultiplied 3-channel
  buffer** at a mid-band row is partially darkened (catches the premultiply/cliff
  bug); base alpha stays 255; band-zero guard for tiny `H`.
- **Color uniformity**: composite a mid-gray patch via the RAW(`Srgb8`) and raster
  paths; post-composite values match within tolerance.
- **Raster decode**: runtime-generate JPEG + PNG with a known **sentinel pixel
  block**; assert decoded dims + sentinel survival (distinguishes decoded from
  blank); truncated JPEG → `decode_failed`; a **non-3-channel decode → `decode_failed`,
  not rendered** (RT-D); a portrait JPEG `Orientation=6` → mark at the visual
  top-right; a **malformed orientation tag → the defined outcome** (RT-J).
- **Zero distortion / downscale-only**: `out_w/out_h ≈ in_w/in_h` within ±1px;
  long edge == max for larger inputs; a **sub-limit** image emitted at native size
  (NOT upscaled).
- **Extraction regression (RT-C)**: `render_to_jpeg(rgb,w,h,&RenderOptions::default())`
  on a known input is decoded back and asserted to (a) NOT upscale, (b) have NO
  shadow band, (c) match a sample pixel — guarding export's fast-path bypass and
  defaults against silent drift (the existing `exists()+len>0` export test cannot).
  Add `test_watermark_position_calculation` (`lib.rs:792-798`) to the regression set.
- **Non-destructive / idempotency**: source entry-set + bytes/mtime unchanged;
  `--output` rejected when nested in `--source`; run `watermark` twice → stable
  output count + assert `skipped-existing: N` on the 2nd run; a symlink in
  `--source` → `--output` does not cause re-processing (RT-G).
- **Counters & summary line (RT-I)**: assert the FULL summary-line string on an
  empty-source `watermark` run and an empty-catalog `rename` run (every counter
  label present); `sidecar_absent: 1` + exit 0 for a no-`.xmp` row; `errored: 0`
  on a happy path.
- **Best-effort RAW dispatch**: corrupt RAW → `decode_failed`, exit 2; `--strict`
  → exit 1; non-CR3 RAW without `--allow-untested-raw` → `skipped_unsupported`,
  exit 0; `--max-long-edge < 16` → clap rejection (exit 2); non-PNG/unreadable
  mark → fatal up-front, **zero output files** (RT-O).
- **Rename construction** (pure `RenamedFilename` builder, unit): exact
  `Cluster-{X}_Cull-{Y}-{stem}.{ext}` incl. named `None`-cluster/`None`-score
  sentinels (assert sort order), `{:05.2}` width, cluster id **≥1000 / negative
  asserted on the builder directly** (the DB `CHECK(cluster_id>=0)` blocks seeding
  negatives — RT-O), NaN/inf score filtered; **stem sanitization** (separators,
  NUL, control, overlong → composed-name cap); **two distinct stems that
  sanitize-and-truncate identically produce two distinct outputs** and
  `output_file_count == input_row_count` (no clobber — RT-B); case-insensitive
  collision (darwin); RAW + sidecar get the SAME suffix.
- **Rename integration**: seed a catalog with known cull+cluster; exact new RAW
  names, each `.xmp` copied + renamed to `new_raw.with_extension("xmp")`, source
  untouched; deleted-source row → `file_missing`; **sidecar-copy failure → NO final
  renamed RAW exists** (not a half-write) + `sidecar_copy_failed` distinct from
  `sidecar_absent`.

### Checkpoints that fire

1. **Plan-review** (in progress): R1 ✓ → R2 ✓ → remediate (this v3) → **Round 3**.
2. **Sub-component review** at the D1.0 extraction + `photohelper-export`
   geometry/shadow boundary (re-pointed `export` must stay green).
3. **Sub-component review** at the `rename` subcommand boundary.
4. **Session-end review**: Round 1 → Round 2 → CLEAN, then ship PR to `main`.

### Unresolved prior-session item surfaced (NOT planned on top of)

- **Session-14 session-end review** (`session-14-implementation-round3.md`) retained
  **15 verified findings** (2 CRITICAL + 9 HIGH + 4 MEDIUM + 2 LOW raw; 3
  hallucinations discarded, `discard_rate 0.16`) in `photohelper-sidecar`, with **no
  Round-4 CLEAN artifact**, yet PR #15 (merge `d4575fca`,
  `session-14/xmp-library-upgrade`) merged. Note: two "Merge pull request #15"
  commits exist (the other is `17da1eb9`, session-12) — the SHA disambiguates.
  Session 15 does not touch that code (D-Q7: catalog metrics + verbatim `.xmp`
  copy; `rename` is its own driver, never calling `read_xmp`/`write_xmp`/`merge_and_write`).
- **Ledger desync** corrected at session-start.

### Open ambiguities — resolved

- **O1**: shadow is **full-bleed** (no horizontal margin, flush to bottom); the
  4.6% margin applies to the **marks only**.
- **O2**: `image_b781b9.jpg` not provided; follow the textual spec; the named
  geometry constants are the single adjustment point.
- **O3**: resolved as **D-Q7** (catalog only; sidecars copied verbatim).

---

## Deliverables

### D0 — Housekeeping (first chore commit)

- `rg`-confirm nothing references the untracked scratch, then remove
  `crates/photohelper-sidecar/test_quick_xml.rs`,
  `crates/photohelper-sidecar/test_quick_xml/`, `diff.txt`.

### D1.0 — Extract shared rendering primitives (FIRST code commit; regression-guarded)

`export_photo` (`crates/photohelper-export/src/lib.rs:178-347`) is a private
monolith; the "reuse" the rest of D1 needs does not exist yet.

- Extract, in `photohelper-export`:
  - `pub fn resize_rgb(rgb, w, h, long_edge: Option<u32>, downscale_only: bool) -> Result<Pixmap, ExportError>` (lifts `lib.rs:235-297`; `downscale_only` adds `scale = min(1.0, …)`).
  - `pub fn render_to_jpeg(rgb, w, h, opts: &RenderOptions) -> Result<Vec<u8>, ExportError>`. **`RenderOptions` is a decode-output-only subset** (RT-L) — `long_edge`, `downscale_only`, `quality`, `shadow: Option<ShadowSpec>`, and the generalized mark list with `BadgeSizeBasis` + per-axis margins; it **excludes** `output_path`/`force` (caller concerns). It ships its FINAL field set at D1.0 (shadow/height-marks impls land in D1c/D1d behind already-present fields — no cross-step signature churn, no dead-code allow).
  - **`render_to_jpeg` MUST preserve export's three paths** (RT-C): (a) fast-path bypass — direct `compress_jpeg` from the input buffer when no resize/shadow/marks (`lib.rs:222-232`); (b) resize branch (`lib.rs:256-284`); (c) no-resize-but-composited branch (`lib.rs:285-296`). Defaults reproduce export's current behavior exactly (long-edge badge basis, no shadow, upscale-allowed).
  - `pub fn pixmap_to_rgb(pixmap) -> Vec<u8>` (lifts demultiply `lib.rs:311-335`).
  - Make `compress_jpeg` (`:664`), `draw_image_watermark` (`:478`), `calculate_watermark_position` (`:349`) `pub`.
- **Re-point `export_photo`** at `render_to_jpeg` (after its existing
  `decode_image(Linear16)` + `ToneMappingLut` step — export keeps its filmic look).
- Move `TempFileGuard` (`export.rs:186-219`) + a generic `resolve_collisions<F: Fn(&str)->String>` (from `export.rs:271-314`, keeping the macOS/Windows case-fold) into `cli/util.rs`.
- **Regression guard**: existing `export` integration tests stay green; ADD the
  stronger `RenderOptions::default()` decode-back assertion (RT-C/RT-I) +
  `test_watermark_position_calculation`. Keep `test_pixel_demultiplication`
  (`lib.rs:722-781`) with its hand-computed expected values, now calling the
  extracted `pixmap_to_rgb` (do NOT replace the asserts with a self-call). Sub-component review fires here.

### D1 — Raster loader, geometry, shadow (Feature 1 core lib)

- **D1a — Source loader**: add `image` as a **new dependency of `photohelper-export`**
  (`default-features=false, features=["jpeg","png"]`; currently only a dev-dep of
  `photohelper-cli`), verify MSRV-1.88 + `cargo audit`.
  `pub fn load_source_image(path, allow_untested_raw) -> Result<RgbImage, ExportError>`:
  - dispatch via an exhaustive **`enum SourceKind`** with `classify(path) -> Option<SourceKind>` (`eq_ignore_ascii_case`); `None` = unsupported extension, a **distinct** outcome from decode-failure (drives `skipped_unsupported` vs `decode_failed`). It is an enum (3 arms: `Raster`, `Cr3`, `UntestedRaw`) **not** an inline 2-arm match, so the `--allow-untested-raw` gate is decided once, before decode (RT-O).
  - raster: `image` decode → `to_rgb8`; **apply EXIF orientation**, and a
    malformed/unsupported orientation tag is a **defined outcome** — `decode_failed`
    (treat as undecodable) OR a documented default-to-Identity with `tracing::warn!`
    (pick one, test it — RT-J).
  - RAW: `decode_image(ProcessOptions::Srgb8)`; CR3 always; `UntestedRaw` only if
    `allow_untested_raw`, then the **post-decode sanity guard**: reject absurd dims
    AND assert **3 channels** (`len == w*h*3`; the FFI does not assert `colors==3`,
    `ffi.rs:758-764`) — route through `RgbImage::new` (`model.rs:670-689`) so the
    channel guard isn't dropped (RT-D). Failures → `decode_failed`.
- **D1b — Geometry module**: named validated consts `MARK1_HEIGHT_FRAC=0.14`,
  `MARK2_HEIGHT_FRAC=0.13`, `MARK_MARGIN_FRAC=0.046`, `SHADOW_BAND_FRAC=0.30`.
  `MarkPlacement` has **private `u32` fields** + accessors; sole constructor
  `MarkPlacement::fit(target,(mw,mh),height_frac,margin_frac) -> Result<MarkPlacement, GeometryError>` with `mark_h=round(H*f).max(1)`, `scale=mark_h/mh`,
  `mark_w=round(mw*scale).max(1)`, margins `round(W*0.046)`/`round(H*0.046)`,
  origins via **`checked_sub`** so an underflow maps to the error (negatives
  unrepresentable; the blit can then drop its bounds clip — RT-M).
  **`enum GeometryError { MarkDoesNotFit { which: MarkSlot, mark_dims: (u32,u32), target_dims: (u32,u32) } }`** (defined, not stringly — RT-H).
- **D1c — Shadow gradient**: `shadow_alpha_ramp(H) -> Vec<u8>` (len `round(0.30*H)`,
  pinned denominator, monotonic 255→0; `None`/empty for tiny `H`). Composite as a
  color op keeping destination alpha 255: `out_rgb = base_rgb*(1 - t)`,
  `t = ramp[row]/255` — NOT a pixmap-alpha write. Full-bleed.
- **D1d — Composite**: parametrize the existing draw path with
  `BadgeSizeBasis { LongEdge(Scale) | Height(HeightFrac) }` — the `Height` arm
  carries a **validated `HeightFrac`** newtype (finite, `0<f<=1`), not a bare f32
  (RT-M) — plus **per-axis `(margin_x, margin_y)`** (RT-E). `calculate_watermark_position`
  becomes 2-axis; export's re-point passes `margin_x==margin_y==(long_edge*0.015).round().max(8.0)`
  to preserve its placement exactly. Enforce resize→shadow→mark1→mark2; a
  `MarkDoesNotFit` is propagated (D-Q5), not clipped.
- **D1e — Unit tests** per the geometry + shadow rows above.
- **Sub-component review** (folded with D1.0).

### D2 — `watermark` subcommand (Feature 1 wiring)

- **D2a — `WatermarkArgs`**: `--source <DIR>`, `--mark1 <FILE>` (PNG), `--mark2 <FILE>`
  (PNG), `--max-long-edge <u32>` (reuse `validate_long_edge`, ≥16), `--output <DIR>`,
  `--allow-untested-raw`, `--force`, `--strict`. Fixed `const WATERMARK_JPEG_QUALITY: u8`
  (no `--quality` flag).
- **D2b — Setup**: canonicalize `--source`/`--output`; **reject `--output` ==/nested-in
  `--source`**; up-front `--output` writability probe (reuse `export.rs:228-244`);
  **preload both PNG marks fatally up-front** (D-Q6); build a deterministic sorted
  walked-file list, **canonically pruning the `--output` subtree** with
  `follow_links(false)` (RT-G).
- **D2c — Pipeline** (rayon + heartbeat, `"watermark: …"` label, with the
  `heartbeat_handle.is_finished()` pre-stop check per `export.rs:512-518`): per file
  `load_source_image` → `render_to_jpeg` (downscale-only, shadow + mark1 + mark2)
  → temp-then-atomic-rename into `--output`. Per-file fail-open per the taxonomy;
  `--strict` fatal on first failure; the per-file rename error is logged + counted
  `errored` (never silently `let _ =`).
- **D2d — Integration tests** per the test list above.

### D3 — `rename` subcommand (Feature 2)

- **D3a — `RenameArgs`**: `--source <DIR>`, `--output <DIR>`, `--force`, `--strict`;
  catalog via the global flag.
- **D3b — Selection + names**: query `all_photos_with_cull_scores(MODEL_SLUG, CLIP_MODEL_SLUG)`
  → `DevelopRow`; **sort rows by `(ingested_at, photo_id)`** (or add `, p.id` to the
  query) so collision-suffix assignment is deterministic (the query's `ORDER BY` has
  no tie-breaker, `catalog.rs:881` — RT-B). Canonicalize `--source`; filter rows by
  **canonical path-component prefix**; per row, **existence precheck** (`file_missing`
  if gone — Theme N). Build the name via a shared, sanitizing **`RenamedFilename`**
  **`Result`-returning constructor** (RT-M) that OWNS: prefix shape (via the shared
  formatter, `{:05.2}` score + named `None` sentinels), stem sanitization (reject
  separators/NUL/control), and a **composed-name** length cap (truncate the STEM so
  prefix + `_N` suffix + ext always survive `NAME_MAX` — RT-B). Pipeline order:
  sanitize → compose → cap-stem → `resolve_collisions` (key on final bytes; same
  suffix for RAW + sidecar). Destination validated by **lexical containment** under
  canonical `--output` (RT-A); a containment rejection → `errored`.
  **`rename` is an independent driver — it must NOT call `read_xmp`** (D-Q7/RT-O).
- **D3c — Atomic unit copy (numbered, RT-N)**: (1) `fs::copy` RAW → `raw.tmp` under
  `--output`; (2) if a `<stem>.xmp` sidecar exists, copy → `xmp.tmp` under `--output`;
  (3) **only after both temps exist**, `rename(raw.tmp→final)` then `rename(xmp.tmp→final)`;
  (4) both temps under `TempFileGuard` until their `commit()`. Output sidecar =
  `new_raw.with_extension("xmp")` (extension-replaced only). Normalize temp mode to
  writable (don't propagate a read-only source mode that breaks `--force` re-runs).
  Counters `sidecar_copied`/`sidecar_absent`/`sidecar_copy_failed` distinct; on
  sidecar failure, NO final RAW is committed.
- **D3d — Tests** per the rename rows above.
- **Sub-component review** fires here.

### D4 — Docs, scripts, ledgers

- README quickstart for `watermark` + `rename`; `just watermark` / `just rename`
  recipes + wrapper scripts (mirror `scripts/photohelper-*.sh`).
- `docs/decisions/NNNN-rename-filename-scheme.md`: the capitalized
  `Cluster-…_Cull-…` divergence from `export`'s lowercase scheme + the shared
  formatter; ALSO record that `develop.rs`'s collision key uses NFC+lowercase
  (`develop.rs:240-264`) while the shared `resolve_collisions` is lowercase-only
  (`export.rs:300-303`) — intentional; `develop` is out of scope (RT-O).
- Ledgers: SESSION-STATE (component progress), HANDOFF checkpoint. **discovery-notes**:
  FIRST reconcile the pre-existing duplicate IDs (DN-029 @`:241`/`:329`; DN-033
  @`:284`/`:305` — renumber the later collisions; current highest distinct is
  **DN-037**, so the new ids are DN-038, DN-039, then the new untested-RAW DN as
  DN-040), THEN file the new untested-RAW DN (distinct from DN-014's *ingest*
  `RAW_EXTS`; neither trigger subsumes the other) including the residual
  color-corruption note (RT-K); reconcile DN-036 **with a note** that acknowledges
  its trigger *fired* this session (D1.0 touches `export_photo`) — capability
  delivered in `watermark`; `export`-pipeline integration of dynamic badges remains
  deferred (do NOT hard-close); cross-ref DN-002. **TECH-DEBT**: file the NEF/ARW
  fixtures+verification TD with an in-source `TD-N` label + binding trigger (next
  free TD id at filing; ledger tail ≈ TD-040 — confirm, don't assume).
