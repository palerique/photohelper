# Session 08 — Export Pipeline Integration, Review Round 1

```yaml
session_config:
  schema_version: 1
  model_claimed: "Gemini 3.5 Flash (High)"
  model_observed: "unverifiable"
  effort_claimed: "MAX"
  effort_observed: "unverifiable"
  ask_user_question_id: null
  user_response: "option-1"
  gate_state: "pass"
  cache_used: true
```

```yaml
plugin_availability:
  schema_version: 1
  agents_requested: ["general-purpose", "code-architect", "code-reviewer", "type-design-analyzer", "silent-failure-hunter", "comment-analyzer", "pr-test-analyzer", "code-simplifier"]
  agents_unavailable: []
  fallback_used: false
  fallback_agents: []
```

## Triage summary

| Theme | Description | Severity | Target File / Area |
|---|---|---|---|
| **Theme A** | Effective Rating Evaluation & Non-Destructive Culling | **CRITICAL** | `docs/plans/session-08.md:39, 79` |
| **Theme B** | Color Space, Buffer Layout, and FFI Safety | **CRITICAL** | `docs/plans/session-08.md:114, 116` |
| **Theme C** | Performance, Thread-Safety, and Database Contention | **HIGH** | `docs/plans/session-08.md:43, 112` |
| **Theme D** | Font Fallbacks & Headless CI Environment Portability | **HIGH** | `docs/plans/session-08.md:27` |
| **Theme E** | File Operations, Directory Creation & Safety Invariants | **HIGH** | `docs/plans/session-08.md:35, 47` |
| **Theme F** | Pipeline Concurrency, Cancellation, and Error Dispatch | **HIGH** | `docs/plans/session-08.md:40, 47` |
| **Theme G** | Mathematical Bounds & Safe Coordinate Calculation | **HIGH** | `docs/plans/session-08.md:35, 84` |

---

## Theme A — Effective Rating Evaluation & Non-Destructive Culling

### [CRITICAL] Finding A.1 — Effective Rating Logic (D1) overrides "Rejected" (-1) and "Unrated" (0) status
* **Source**: General Purpose, Silent Failure Hunter, Comment Analyzer, Type Design Analyzer
* **Location**: [docs/plans/session-08.md:79](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L79)
* **Problem**: The proposed formula $\text{rating}(P) = \max(\text{Rating}(P_{\text{xmp}}), \text{Rating}_{\text{nima}}(P_{\text{catalog}}), 0)$ uses a mathematical `max`. If a user manually down-rates or rejects a photo (e.g. `Rating::Rejected` which maps to `-1` or `1` star), but the automated AI NIMA score was high (e.g. maps to `4`), the `max` formula evaluates to `4`. This completely ignores the user's manual selection, violating non-destructive manual culling priority.
* **Remediation**: Replace the `max` formula in D1 with clear hierarchical piecewise resolution logic:
  $$\text{rating}(P) = \begin{cases}
  \text{Rating}(P_{\text{xmp}}) & \text{if } P_{\text{xmp}} \text{ exists and has a rating} \\
  \text{Rating}_{\text{nima}}(P_{\text{catalog}}) & \text{else if NIMA score exists} \\
  0 & \text{otherwise}
  \end{cases}$$
  Explicitly state that if `rating(P) == -1` (Rejected), the exporter skips the image immediately.

### [HIGH] Finding A.2 — CLI `--min-rating` range of `1..=5` prevents exporting unrated photos
* **Source**: General Purpose, Type Design Analyzer, Code Simplifier
* **Location**: [docs/plans/session-08.md:39](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L39)
* **Problem**: Restricting `--min-rating` range to `1..=5` makes it impossible to export unrated photos (which default to 0). A user is blocked from exporting their entire catalog or just unrated photos.
* **Remediation**: Expand `--min-rating` range to `0..=5` (default 3), where `0` is documented as exporting all photos, including unrated ones (while still excluding explicitly `Rejected` photos).

### [HIGH] Finding A.3 — Missing Validation for Non-Finite / NaN NIMA Scores
* **Source**: Silent Failure Hunter
* **Location**: [docs/plans/session-08.md:41-46](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L41-L46)
* **Problem**: If NIMA score is `NaN` or non-finite, naive inequality chains (`score < 4.0`) can fall through to the final fallback (e.g., `else { Rating::Five }`), mapping corrupt records to the highest rating.
* **Remediation**: Explicitly validate NIMA scores to be finite before mapping. If `NaN` or infinite, log a warning to `stderr` and treat it as `0` (Unrated).

---

## Theme B — Color Space, Buffer Layout, and FFI Safety

### [CRITICAL] Finding B.1 — Pixel Format Mismatch and Missing Conversion between `tiny-skia` and `mozjpeg`
* **Source**: Code Reviewer, Type Design Analyzer, Code Architect
* **Location**: [docs/plans/session-08.md:116](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L116)
* **Problem**: `tiny-skia::Pixmap` represents image buffers in **premultiplied RGBA** (4 bytes per pixel). However, `mozjpeg`'s compression API expects **un-premultiplied RGB** (3 bytes per pixel). Feeding raw `tiny-skia` bytes directly to `mozjpeg` will cause out-of-bounds reads, memory corruption, or completely garbled outputs.
* **Remediation**: Explicitly specify the pixel conversion in the pipeline steps:
  1. Loop over each pixel of the scaled `tiny-skia::Pixmap`.
  2. Demultiply the alpha channel by calling `.demultiply()` on each pixel (recovering `ColorU8`).
  3. Extract R, G, and B bytes, discarding the A channel.
  4. Pack these bytes into a contiguous `Vec<u8>` of size $3 \times W \times H$ before passing to `mozjpeg`.

### [MEDIUM] Finding B.2 — Inaccurate Outlining Assumptions with `cosmic-text`
* **Source**: Code Simplifier, Comment Analyzer
* **Location**: [docs/plans/session-08.md:114](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L114)
* **Problem**: `cosmic-text`'s rasterization produces pixel glyph fills and does not natively support vector stroking. Implementing vector path outline stroking in `tiny-skia` requires writing extensive path-conversion code.
* **Remediation**: Recommend a multi-pass offset rendering approach (rendering dark text 4-way or 8-way with a 1.5px offset behind the white text) to provide clean legibility outlines without high design complexity.

---

## Theme C — Performance, Thread-Safety, and Database Contention

### [HIGH] Finding C.1 — SQLite Connection Contention & Sequential I/O Bottlenecks
* **Source**: Code Architect, Code Simplifier
* **Location**: [docs/plans/session-08.md:43](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L43)
* **Problem**: Evaluating ratings on-demand per photo inside the parallel loop will cause SQLite lock contention or force transaction synchronization. Conversely, evaluating them sequentially on the main thread creates an $O(N)$ disk check/XML parsing bottleneck.
* **Remediation**:
  1. **Batch query catalog upfront**: Perform a single SQLite query on the main thread to fetch IDs, paths, and NIMA scores of all qualifying non-superseded photos into an in-memory map.
  2. **Parallelize XMP check**: Let worker threads perform XMP checks and XML parsing concurrently in Rayon since file-system reads are thread-safe and highly concurrent.

### [HIGH] Finding C.2 — Heavy `FontSystem` Instantiated Inside Parallel Loop
* **Source**: Code Architect, Comment Analyzer
* **Location**: [docs/plans/session-08.md:112](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L112)
* **Problem**: Initializing `cosmic-text::FontSystem` is very slow because it scans system directories. Doing this per-photo inside the parallel loop will degrade performance.
* **Remediation**: Instantiate `FontSystem` once outside the loop, or use thread-local storage (`thread_local!`) so it is initialized at most once per physical thread.

### [MEDIUM] Finding C.3 — Performance Overhead of Rayon `par_bridge`
* **Source**: Code Simplifier, Code Architect
* **Location**: [docs/plans/session-08.md:47](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L47)
* **Problem**: Using `par_bridge` converts sequential iterators into parallel ones but introduces internal mutex locking overhead.
* **Remediation**: Collect resolved target rows into a `Vec` first and use standard `.par_iter()` or `.into_par_iter()`, maximizing CPU core scaling.

---

## Theme D — Font Fallbacks & Headless CI Environment Portability

### [HIGH] Finding D.1 — Headless CI Font Dependencies & Silent Watermark Failures
* **Source**: Comment Analyzer, Silent Failure Hunter, Code Architect
* **Location**: [docs/plans/session-08.md:27](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L27)
* **Problem**: Relying on OS system fonts for `cosmic-text` means headless environments (like GitHub Actions runners) with no fonts installed will fail to render the watermark or produce empty boxes in tests.
* **Remediation**: Bundle a small open-source sans-serif TrueType font (under 100KB) as a byte array (`include_bytes!`) directly inside the export binary to guarantee a reproducible fallback across all machines and CI.

---

## Theme E — File Operations, Directory Creation & Safety Invariants

### [HIGH] Finding E.1 — Output Filename Collisions and Silent Data Loss
* **Source**: Code Architect, Code Simplifier
* **Location**: [docs/plans/session-08.md:35](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L35)
* **Problem**: If multiple RAW photos share the same base name (e.g. `dir1/IMG_0001.CR3` and `dir2/IMG_0001.CR3`), Rayon threads will write to `<output-dir>/IMG_0001.jpg` concurrently, causing corruption or silent overwrites.
* **Remediation**: Scan the export queue upfront on the main thread. If filename collisions are detected, append a short unique suffix derived from the photo's ID or a sequential suffix to prevent overwrite data loss.

### [HIGH] Finding E.2 — Lack of Directory Creation Pre-Validation
* **Source**: Silent Failure Hunter, Code Architect
* **Location**: [docs/plans/session-08.md:35](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L35)
* **Problem**: Performing directory creation inside worker threads introduces concurrent write races on `create_dir_all`. If creation fails due to permissions, threads will still process raw images before failing on write.
* **Remediation**: Create the output folder recursively on the main thread *before* entering the Rayon parallel loop. Abort immediately if the folder cannot be created.

---

## Theme F — Pipeline Concurrency, Cancellation, and Error Dispatch

### [HIGH] Finding F.1 — Inefficient Concurrency and Lack of Cancellation under `--strict`
* **Source**: Code Architect, PR Test Analyzer
* **Location**: [docs/plans/session-08.md:40](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L40)
* **Problem**: Under `--strict`, any single error is fatal for the run. Continuing to decode and process remaining massive RAW files in Rayon after a failure has occurred is a massive waste of resources.
* **Remediation**: Introduce a shared `AtomicBool` cancellation token (`is_cancelled`) across worker threads. If `--strict` is enabled, threads must check this token at key boundaries and abort processing early if it is set.

### [HIGH] Finding F.2 — Missing Heartbeat Thread in Export Subcommand
* **Source**: Code Architect
* **Location**: [docs/plans/session-08.md:47](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L47)
* **Problem**: Exporting is a slow, heavy process. Not having a stderr heartbeat thread violates the codebase liveness patterns (`CLAUDE.md`) established in ingest, cull, and develop commands.
* **Remediation**: Spawn the standard stderr heartbeat loop thread (printing stats every 10s) and shut it down cleanly on completion.

### [HIGH] Finding F.3 — Incomplete/Corrupt Output Files on Export Failure
* **Source**: Silent Failure Hunter
* **Location**: [docs/plans/session-08.md:47](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L47)
* **Problem**: If a thread fails midway through exporting (e.g. compression or write error), a partial, corrupt, or zero-byte JPEG could be left in the output folder.
* **Remediation**: Implement atomic file writes. Write the compressed bytes to a `.jpg.tmp` file in the destination folder, and then atomically rename it to the target `.jpg`. Clean up any temporary files if a step fails.

### [MEDIUM] Finding F.4 — Missing RAW File Edge Case Handling
* **Source**: Comment Analyzer, PR Test Analyzer
* **Location**: [docs/plans/session-08.md:47](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L47)
* **Problem**: If a RAW file is missing from disk, it must be skipped. If `--strict` is active, this must count as a fatal run failure.
* **Remediation**: Explicitly check file existence. Print the error to stderr, skip the file, and under `--strict`, flag it as fatal.

---

## Theme G — Mathematical Bounds & Safe Coordinate Calculation

### [HIGH] Finding G.1 — Unsigned Integer Underflow Panic in Text Position Calculation
* **Source**: Silent Failure Hunter, Code Reviewer, Comment Analyzer
* **Location**: [docs/plans/session-08.md:84](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L84)
* **Problem**: Coordinating math such as $y_{pos} = H - D - H_{text}$ using unsigned `u32` variables can cause a subtraction underflow panic on very small canvases or thumbnail scales.
* **Remediation**: Perform all layout coordinate calculations using signed floating-point math (`f32`) or signed `i32`, clamping coordinates to safe ranges ($\ge 0$) before rendering.

### [HIGH] Finding G.2 — Potential Panic/Crash on Zero/Invalid Image Dimensions
* **Source**: Code Reviewer, Comment Analyzer
* **Location**: [docs/plans/session-08.md:35](file:///Users/ph/area-de-trabalho/pessoal/photohelper/docs/plans/session-08.md#L35)
* **Problem**: If `--long-edge` is specified as `0` or computed scaled dimensions evaluate to `0`, constructing the `Pixmap::new(w, h)` will return `None`. Unwrapping this returns a panic, violating the zero-panic mandate.
* **Remediation**: Enforce a minimum bound on `--long-edge` at CLI parse time (e.g. $\ge 16$ pixels), and clamp computed proportional dimensions to at least `1` pixel.

---

## Disposition summary

| Finding | Severity | Triage Disposition / Action | Remediated in Plan? |
|---|---|---|---|
| **A.1 (Rating Priority)** | **CRITICAL** | Piecewise conditional priority logic instead of `max` | Planned for v2 |
| **A.2 (CLI Bounds)** | **HIGH** | Change `--min-rating` range to `0..=5` | Planned for v2 |
| **A.3 (Finite NIMA)** | **HIGH** | Check NIMA score finiteness before rating mapping | Planned for v2 |
| **B.1 (Color Format)** | **CRITICAL** | Demultiply and convert RGBA 4-channel to RGB 3-channel | Planned for v2 |
| **B.2 (Text Outline)** | **MEDIUM** | Suggest a robust 4-way offset text-blend drop-shadow | Planned for v2 |
| **C.1 (DB Bottleneck)** | **HIGH** | Batch query SQLite metadata upfront on main thread | Planned for v2 |
| **C.2 (Font System)** | **HIGH** | thread_local! cache for FontSystem to initialize once per thread | Planned for v2 |
| **C.3 (par_bridge)** | **MEDIUM** | Use direct par_iter() on Collected Vec | Planned for v2 |
| **D.1 (Headless Font)** | **HIGH** | Embed fallback TTF font using include_bytes! | Planned for v2 |
| **E.1 (Collisions)** | **HIGH** | Append ID-derived unique suffix on name collision | Planned for v2 |
| **E.2 (Pre-Dir check)**| **HIGH** | Recursive create output folder on main thread first | Planned for v2 |
| **F.1 (Cancellation)**| **HIGH** | AtomicBool cancellation flag for fast exit under strict | Planned for v2 |
| **F.2 (Heartbeat)** | **HIGH** | Spawn standard 10s progress log thread | Planned for v2 |
| **F.3 (Atomic Write)**| **HIGH** | Write to `.tmp` file and rename on success | Planned for v2 |
| **F.4 (Missing RAW)** | **MEDIUM** | Skip missing raw files and report as fatal under strict | Planned for v2 |
| **G.1 (Underflow)** | **HIGH** | Perform coordinate arithmetic in signed space (f32/i32) | Planned for v2 |
| **G.2 (Zero Canvas)** | **HIGH** | Validate bounds on CLI and clamp dimensions to >= 1 | Planned for v2 |

---

## Verification

```yaml
verification:
  schema_version: 1
  parent_gate_state: pass
  total_findings: 12
  verified: 12
  drifted: 0
  hallucinated: 0
  unreadable: 0
  compromised: 0
  discard_rate: 0.00
  details:
    - finding_id: 1a33ce1756027bd77a9a6b7914d27741206af8f2
      file: docs/plans/session-08.md
      line: 79
      present: yes
      retain: yes
      reason: "Effective rating evaluation logic contradicts manual culling overrides"
      evidence_snippet: |
        ## Design decisions locked by this plan

        ### D1 — Effective Rating Evaluation Logic
        To decide whether photo $P$ gets exported:
        $$\text{rating}(P) = \max(\text{Rating}(P_{\text{xmp}}), \text{Rating}_{\text{nima}}(P_{\text{catalog}}), 0)$$
        If $\text{rating}(P) \ge \text{min\_rating}$, $P$ is exported.
    - finding_id: bf999ac733a42286e23fcfcb1bfb21f63867c95b
      file: docs/plans/session-08.md
      line: 39
      present: yes
      retain: yes
      reason: "CLI --min-rating bounds restriction prevents exporting unrated photos (0)"
      evidence_snippet: |
           - **Clap Options**:
             - `--output <DIR>` (required): Output directory for compiled JPEGs. Created recursively if missing.
             - `--long-edge <PX>` (optional): Long-edge resize limit in pixels.
             - `--quality <QUALITY>` (optional, default 80): JPEG quality level (1 to 100).
             - `--watermark <TEXT>` (optional): Watermark text.
             - `--min-rating <RATING>` (optional, default 3): Minimum rating to export (range `1..=5`).
    - finding_id: b8d78179423d3a450353219570223b9dacf5c3ba
      file: docs/plans/session-08.md
      line: 116
      present: yes
      retain: yes
      reason: "tiny-skia uses premultiplied RGBA while mozjpeg expects un-premultiplied RGB"
      evidence_snippet: |
        Implement pure-Rust text layout and rasterization:
        - Load default system sans-serif font using `cosmic-text::FontSystem` and `Buffer`.
        - Compute dynamic font size and padding.
        - Render text into mask/outline, blend using `tiny-skia` over the scaled image canvas.

        ### Step 4: MozJPEG Encoding wrapper
        Wrap MozJPEG's write loop:
        - Convert scaled sRGB pixel buffer to progressive-scan JPEG bytes.
    - finding_id: 32c8d3fff1194da9c64786654e92ad2785ca52ff
      file: docs/plans/session-08.md
      line: 114
      present: yes
      retain: yes
      reason: "Outlining is not natively supported in cosmic-text's basic layout/rasterization buffer"
      evidence_snippet: |
        ### Step 3: Cosmic-Text Watermark Composition
        Implement pure-Rust text layout and rasterization:
        - Load default system sans-serif font using `cosmic-text::FontSystem` and `Buffer`.
        - Compute dynamic font size and padding.
        - Render text into mask/outline, blend using `tiny-skia` over the scaled image canvas.
    - finding_id: c6320a3dacf39d95fe68813cedcc289d87091f7d
      file: docs/plans/session-08.md
      line: 43
      present: yes
      retain: yes
      reason: "Performing rating resolution sequentially causes disk I/O bottlenecks or SQLite lock contention"
      evidence_snippet: |
             - `--watermark <TEXT>` (optional): Watermark text.
             - `--min-rating <RATING>` (optional, default 3): Minimum rating to export (range `1..=5`).
             - `--strict` (optional): Treat any single-photo export failure as fatal for the overall run.
           - **Effective Rating Evaluation**:
             - Resolves each photo's rating non-destructively:
               1. **XMP Sidecar first**: Checks if a corresponding `.xmp` file exists next to the raw photo. If it has `xmp:Rating`, use that.
               2. **NIMA score second**: If no XMP rating exists, look up the NIMA score in the catalog and map it to a rating (Score $< 4.0 \rightarrow 1$; $[4.0, 5.5) \rightarrow 2$; $[5.5, 7.0) \rightarrow 3$; $[7.0, 8.5) \rightarrow 4$; $\ge 8.5 \rightarrow 5$).
    - finding_id: 1a42af4890a86a4029d0ea1d78b156dc3e10273c
      file: docs/plans/session-08.md
      line: 112
      present: yes
      retain: yes
      reason: "Heavy FontSystem instantiation inside parallel loop creates performance/memory bottlenecks"
      evidence_snippet: |
        ### Step 2: Resizing Pipeline
        Implement aspect-ratio preserving scaling. Convert sRGB `RgbImage` to `tiny-skia::Pixmap`, compute optimal target dimensions based on `--long-edge`, apply matrix scale transform, and render into a new `Pixmap`.

        ### Step 3: Cosmic-Text Watermark Composition
        Implement pure-Rust text layout and rasterization:
        - Load default system sans-serif font using `cosmic-text::FontSystem` and `Buffer`.
    - finding_id: 790eab4a2522981ffb6b93f30ec34c1f93679437
      file: docs/plans/session-08.md
      line: 27
      present: yes
      retain: yes
      reason: "Headless environments lack system fonts, causing blank/failed watermarks"
      evidence_snippet: |
           - **Proportional Text Watermarking**:
             - Combines `cosmic-text` (pure-Rust glyph layout/rasterization) and `tiny-skia` (2D hardware-accelerated vector canvas) to compose beautiful, crisp watermarks.
             - Portrait/landscape aware placements: supports bottom-left or top-right positioning.
             - **Proportional Sizing & Padding**: Font size and padding are calculated dynamically relative to the scaled image's long edge (e.g. font size is 2% of the long edge, padding is 1.5% of the respective edge), ensuring identical relative positioning and size across mixed-resolution exports.
             - **Legibility Design**: Semi-transparent white text (70% opacity, `rgba(255, 255, 255, 0.7)`) with a subtle dark drop shadow or stroke (30% opacity, `rgba(0, 0, 0, 0.3)`) to guarantee perfect readability on both extremely light (snow/sky) and extremely dark (night) backgrounds.
             - **Font Fallbacks**: Standard, widely-available system font collections are automatically loaded/matched via `cosmic-text`'s system fallback mechanism.
    - finding_id: 118b035edf51f28a9ee52461d84cc6908efb30c0
      file: docs/plans/session-08.md
      line: 35
      present: yes
      retain: yes
      reason: "--output is a flat directory, causing filename collisions and silent data loss"
      evidence_snippet: |
        2. **CLI `export` Subcommand**:
           - **Clap Options**:
             - `--output <DIR>` (required): Output directory for compiled JPEGs. Created recursively if missing.
             - `--long-edge <PX>` (optional): Long-edge resize limit in pixels.
             - `--quality <QUALITY>` (optional, default 80): JPEG quality level (1 to 100).
             - `--watermark <TEXT>` (optional): Watermark text.
             - `--min-rating <RATING>` (optional, default 3): Minimum rating to export (range `1..=5`).
             - `--strict` (optional): Treat any single-photo export failure as fatal for the overall run.
    - finding_id: cf917ed69b0a4a556a6d95a7dc9dde9ffa0db6e0
      file: docs/plans/session-08.md
      line: 35
      present: yes
      retain: yes
      reason: "Creating output directory in worker threads causes race conditions or late write failures"
      evidence_snippet: |
        2. **CLI `export` Subcommand**:
           - **Clap Options**:
             - `--output <DIR>` (required): Output directory for compiled JPEGs. Created recursively if missing.
             - `--long-edge <PX>` (optional): Long-edge resize limit in pixels.
             - `--quality <QUALITY>` (optional, default 80): JPEG quality level (1 to 100).
             - `--watermark <TEXT>` (optional): Watermark text.
             - `--min-rating <RATING>` (optional, default 3): Minimum rating to export (range `1..=5`).
             - `--strict` (optional): Treat any single-photo export failure as fatal for the overall run.
    - finding_id: 0c8273e74ab538fcc072d640a490fbcd85eb1276
      file: docs/plans/session-08.md
      line: 40
      present: yes
      retain: yes
      reason: "Lack of pipeline cancellation under --strict wastes CPU and I/O cycles on failure"
      evidence_snippet: |
             - `--output <DIR>` (required): Output directory for compiled JPEGs. Created recursively if missing.
             - `--long-edge <PX>` (optional): Long-edge resize limit in pixels.
             - `--quality <QUALITY>` (optional, default 80): JPEG quality level (1 to 100).
             - `--watermark <TEXT>` (optional): Watermark text.
             - `--min-rating <RATING>` (optional, default 3): Minimum rating to export (range `1..=5`).
             - `--strict` (optional): Treat any single-photo export failure as fatal for the overall run.
    - finding_id: 09f343805e25055596394671b08fdfb4685ed910
      file: docs/plans/session-08.md
      line: 47
      present: yes
      retain: yes
      reason: "Missing RAW file edge case is not handled under --strict"
      evidence_snippet: |
           - **Effective Rating Evaluation**:
             - Resolves each photo's rating non-destructively:
               1. **XMP Sidecar first**: Checks if a corresponding `.xmp` file exists next to the raw photo. If it has `xmp:Rating`, use that.
               2. **NIMA score second**: If no XMP rating exists, look up the NIMA score in the catalog and map it to a rating (Score $< 4.0 \rightarrow 1$; $[4.0, 5.5) \rightarrow 2$; $[5.5, 7.0) \rightarrow 3$; $[7.0, 8.5) \rightarrow 4$; $\ge 8.5 \rightarrow 5$).
               3. **Fallback**: Default to Unrated (0) if neither is found.
             - Filters out all photos whose effective rating is strictly less than `--min-rating`.
           - **Rayon Parallel Processing**:
             - Distributes RAW decoding, resizing, watermarking, and JPEG encoding across all physical CPU cores using `rayon::par_bridge`.
             - Maintains complete resilience: print per-photo errors to stderr and continue processing remaining photos. Exit non-zero if `--strict` is active and errors occurred.
    - finding_id: 2bbb161cc0e398825d872386ace8025f760eb602
      file: docs/plans/session-08.md
      line: 84
      present: yes
      retain: yes
      reason: "Coordinate math using u32 can cause subtraction underflow panics on tiny canvases"
      evidence_snippet: |
        - Let $L$ be the long edge length of the final output image.
        - Font size $F = \max(12, \text{round}(L \times 0.02))$ pixels.
        - Padding $D = \max(8, \text{round}(L \times 0.015))$ pixels.
        - Watermark position is aligned relative to bottom-right or bottom-left (configurable or hardcoded default bottom-right/bottom-left). For v0.1, we hardcode bottom-left as standard, portrait/landscape aware.
        - Text uses a black outline (stroke width = 1.5px, 30% opacity) surrounding semi-transparent white text (70% opacity) to remain legible across any background.
```
