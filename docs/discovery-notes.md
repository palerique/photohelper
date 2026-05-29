# photohelper — Discovery Notes

> Append-only log of design gaps, surprising findings, and questions surfaced
> during implementation that belong to a *different* owner (an upstream design
> doc, another team, a future session). Don't fix the gap here — record it so
> the right owner can reconcile it. Each entry gets a stable `DN-NNN` id.
>
> Append-only: corrections are added as new lines/notes, not by rewriting prior
> entries. The git log of this file is the audit trail.

## Entry format

```markdown
### DN-NNN — <short title> (YYYY-MM-DD, session N)

- **Observed**: <the concrete symptom / gap, with file or doc references>
- **Why it matters**: <impact if unreconciled>
- **Owner**: <who should reconcile — upstream doc, other team, future session>
- **Status**: open | reconciled (YYYY-MM-DD, how)
```

---

### DN-001 — LibRaw LGPL static-link distribution mechanics (2026-05-27, session 0; LGPL clause corrected 2026-05-28, session 2)

- **Observed**: The plan locks LibRaw 0.21+ as the RAW decoder (CR3 support for Canon R8). LibRaw is dual-licensed LGPL-2.1 / CDDL-1.0; we plan to statically link the LGPL build. LGPL static linking requires the distributor to offer "the means to relink" — typically a tarball of object files or build inputs alongside each release binary.
- **Why it matters**: We need to know what artifact (e.g. `vendor/libraw-X.Y.Z.tar.gz` per release) ships in GitHub Releases to satisfy **LGPL-2.1 §6(a)** (the "complete corresponding machine-readable source code … so that the user can relink to produce a modified executable" clause). Affects the release workflow and the release notes template. *Originally cited §6(b) in error; §6(b) is the alternative shared-library mechanism, which we are NOT taking. Corrected per session-02 plan-review `PR1-T17`.*
- **Owner**: session that introduces `photohelper-raw` LibRaw FFI (session 02 owns the decision-doc + build-mechanism choice; the release-engineering session owns the GitHub Release workflow that actually ships the tarball alongside binaries).
- **Status**: open (decision-doc 0002 will record the §6(a) artifact shape this session; release-workflow wiring deferred).

### DN-002 — Watermark configuration scope (CLI flags vs `photohelper.toml` vs sidecar) (2026-05-27, session 0)

- **Observed**: Three plausible locations for watermark configuration (top-right + bottom-left text/image, font, color, opacity, margin): one-off CLI flags, project-level `photohelper.toml`, or per-photo `ph:` sidecar entries. The plan recommends a 3-tier merge (CLI overrides toml overrides sidecar) with the resolved config snapshotted into `ph:WatermarkTopRight` / `ph:WatermarkBottomLeft` for reproducibility.
- **Why it matters**: Must be decided before `photohelper-export` lands so users don't have to migrate configuration shape later. Touches CLI surface, sidecar schema, and config loading.
- **Owner**: session that lands the `export` subcommand (planned for session 04+).
- **Status**: open

### DN-003 — In-process vs subprocess ONNX inference for crash isolation (2026-05-27, session 0)

- **Observed**: v0.1 wires `ort` v2.0 (ONNX Runtime) directly inside `photohelper-cli`. A model crash on photo N takes down the run, losing progress on photos N+1…M. Subprocess sandbox (a tiny helper binary per inference) would be more robust at the cost of IPC overhead. The plan defers this to v0.5 reassessment.
- **Why it matters**: Large-batch users (thousands of photos) are exactly the audience that benefits from crash isolation, but they're also exactly the audience that pays the IPC overhead per photo. Need real-world crash-rate data before committing.
- **Owner**: future session if crash reports surface from real users.
- **Status**: open

### DN-004 — Sidecar conflict UX when user edited in Lightroom after photohelper processed (2026-05-27, session 0)

- **Observed**: When both `crs:` (Lightroom-written) and `ph:` (photohelper-written) settings exist and disagree, the planned resolution is timestamp-based: if `ph:LastProcessedAt >= xmp:MetadataDate` trust `ph:`, else trust `crs:`. We never delete `crs:` tags we don't understand. Open question: silent reconciliation vs explicit summary log line per photo, with a `--strict` flag that escalates conflicts to errors.
- **Why it matters**: Wrong choice silently destroys user intent when both editors touch the same photo. Must be locked before `develop` lands.
- **Owner**: session that lands the `develop` subcommand (planned for session 03+).
- **Status**: open

### DN-005 — Catalog storage shape (SQLite confirmed; schema TBD) (2026-05-27, session 0)

- **Observed**: SQLite (via `rusqlite`) chosen over sled / flat JSON for the catalog at `<root>/.photohelper/catalog.db`. Schema (tables, indices, migration story) is undefined. Lightroom's `.lrcat` is the prior-art precedent but is not open-spec.
- **Why it matters**: First session that writes catalog rows (session 01 for the `ingest` slice) needs at least a `photos` table; a half-baked schema becomes a migration headache.
- **Owner**: session 01 (minimal schema) + session 02 (full schema once `cull` adds dup-group and culling-score tables).
- **Status**: partially resolved 2026-05-28 — session 01 lands v1 single-table `photos` schema; authoritative spec at `docs/decisions/0001-catalog-schema-v1.md`. Session 02 still owes cull-score + dup-group tables + the migration framework v1 → v2 (per the decision doc's "Migration policy" section).
  - **Update (2026-05-28, session 03 plan-review R1 remediation — PR1-T36)**: session 03 owns the v1→v2 migration + `cull_scores` table per decision-doc 0001 § Amendments 2026-05-28. `dup_groups` table deferred to session 04+ (DN-024). "Session 02" owner above crossed out; session 03 is the authoritative owner. DN-005 will be fully closed when session 03's PR merges.

### DN-006 — kamadak-exif cannot parse Canon R8 CR3 (both synthetic AND real CR3 fixtures) (2026-05-28, session 1; upgraded 2026-05-28 by DN-011)

- **Observed**: Session 01 implementation smoke test on `/tmp/ph_demo` and integration test row 32 confirmed: `kamadak-exif 0.6` returns "Unknown image format" when fed raw `0xCC`-byte synthetic CR3 fixtures, then yields zero `Fields` on subsequent parse. **DN-011 escalates**: the user's production run on 371 real Canon R8 CR3s (`/Users/ph/Pictures/tests`, 2026-05-28 15:32:52) reproduced the same failure on every single file. kamadak-exif is non-functional for CR3 in v0.1, full stop — not just for synthetic fixtures.
- **Why it matters**: The v0.1 fallback (NULL EXIF columns + `no-exif` counter bump + camera_slug NULL) is no longer a "best-effort fallback" — it IS the production behavior for all CR3 files. The `CameraRegistry::for_exif` lookup never succeeds; the `--strict` flag (per R2-T12) now correctly fails on no_exif > 0, making strict mode effectively unusable for CR3 in v0.1. Session 02's LibRaw integration is the only remediation path.
- **Owner**: session 02 — LibRaw EXIF extraction is no longer optional (no longer "if kamadak-exif fails, fall back to LibRaw"; it IS the primary path for CR3). Confirm with: (a) LibRaw can extract Make/Model/Orientation/CaptureTime from a real Canon R8 CR3, then (b) flip integration test row 32's assertions from `is_none()` to `Some("canon-r8")` AND make the strict-mode tests pass on real CR3 fixtures.
- **Status**: closed 2026-05-28 (session 2, Deliverable 4 atomic kamadak-exif removal). `photohelper-cli::commands::ingest` no longer calls kamadak-exif at all — the `parse_cr3_exif` function in `crates/photohelper-cli/src/commands/ingest.rs` delegates to `photohelper_raw::exif::read_cr3` (LibRaw 0.22.1, vendored). Acceptance criterion 2b smoke test against `/Users/ph/Pictures/tests` (370 CR3 + 1 `.photohelper` dir): `walked: 371, ingested: 370, unknown-camera: 0, no-exif: 0, errored: 0, --strict exit 0`. Was `walked: 371, ingested: 0, no-exif: 370, errored: 0` with kamadak.

### DN-011 — DN-006 extends to ALL real Canon R8 CR3s (not just synthetic fixtures) (2026-05-28, session 1)

- **Observed**: User executed `photohelper ingest /Users/ph/Pictures/tests --strict` against 371 real Canon R8 CR3 files (2026-05-28 15:32:52). Result: every single CR3 emitted `WARN photohelper::commands::ingest: EXIF parse failed error=EXIF parse error at <path>: Unknown image format`. Summary: `walked: 371, no-exif: 370, ingested: 0, already-catalogued: 370, skipped (non-RAW): 1`. The synthetic-CR3 limitation from DN-006 is NOT a fixture-quality artifact; kamadak-exif genuinely cannot parse the Canon CR3 ISO-BMFF container format in v0.1.
- **Why it matters**: Three downstream consequences. (1) Plan v5's "DN-006 fallback" language baked into ~23 places quietly stops being a "best-effort fallback" and becomes "the actual production behavior." (2) Session 02's LibRaw work is elevated from "RAW pixel decode" to "EXIF read + RAW pixel decode" — LibRaw must extract Make/Model/Orientation/CaptureTime, not just pixel data. (3) Session 01's `--strict` mode is effectively useless for CR3 in v0.1 (per R2-T12 fix: strict now fails on no_exif > 0, which means every CR3 ingest in strict mode fails). This is intentional escalation — strict means strict — and the only fix is LibRaw EXIF.
- **Owner**: session 02 (LibRaw EXIF). Also surfaces a v0.1 user-facing limitation: any session-01 binary released to users will be `--strict`-unusable for CR3.
- **Binding trigger**: session 02's first plan commit MUST include "LibRaw EXIF extraction (Make/Model/Orientation/CaptureTime)" as a §Deliverables item; if it doesn't, the session-02 plan-review must reject.
- **Status**: closed 2026-05-28 (session 2, Deliverable 4 ingest rewire). The user's `walked: 371, no-exif: 370, ingested: 0` failure mode now produces `walked: 371, ingested: 370, no-exif: 0, errored: 0, --strict exit 0` (verified via end-to-end smoke against the same fixture set). DN-006 captures the kamadak-exif specifics; this DN flips to closed alongside.

### DN-012 — T15 minor-polish items deferred to session 02 (2026-05-28, session 1)

- **Observed**: R1.T15 surfaced minor type/polish items that were noted as "kept for now" but landed in SESSION-STATE without DN or TD entries (`KnownCamera` Display impl, workspace clippy allow-list per-line rationale comments, Windows case-sensitivity in walker filter, `UpsertOutcome` `#[non_exhaustive]` for uniformity). R2-T17 flagged the missing ledger entries as a `No Acceptable Trade-offs Policy` violation.
- **Why it matters**: Each item is small (≤5 LoC + maybe 1 comment), but the cumulative "minor polish drift" pattern is the textbook tech-debt accretion the policy is designed to catch.
- **Owner**: session 02 (or any session that touches `model.rs` / `Cargo.toml` / `cameras::registry.rs` for other reasons — fold these in).
- **Binding trigger**: next session that touches any of: `crates/photohelper-core/src/model.rs::KnownCamera`, `Cargo.toml` `[workspace.lints]`, `crates/photohelper-cli/src/commands/ingest.rs::WalkBuilder` filter, or `crates/photohelper-catalog/src/catalog.rs::UpsertOutcome`. OR by `2026-08-01`.
- **Status**: open

### DN-007 — `rusqlite` pinned at 0.32 instead of plan-v5's 0.40 target (2026-05-28, session 1)

- **Observed**: Plan v5 §Dependencies committed `rusqlite 0.40`. Session-01 implementation shipped `rusqlite 0.32` (the version that lets the `bundled` SQLite amalgamation compile cleanly under the rest of the dep graph on 2026-05-28). The 0.32 bundle is ~14 months stale and per R2.T1 (plan-review) "will trip SQLite CVE advisories." `cargo audit --deny warnings` is currently clean on 0.32, but the staleness is a real future-risk.
- **Why it matters**: Each SQLite CVE released after the 0.32 bundle's cutoff is a candidate for `cargo audit` failure. The longer we sit on 0.32, the more likely a CI break + emergency-bump scramble.
- **Owner**: TD-002 (filed; binding trigger = "bump by 2026-08-01 OR before session 02 adds new schema columns, whichever first"). Cross-reference this DN.
- **Status**: open

### DN-008 — Missing `cfg(test)` knobs for plan-row tests 6, 12, 13, 14, 17, 18, 19, 34, 39, 42 partial, 43 partial, 49 (2026-05-28, session 1; rewritten 2026-05-28 per R2-T16)

- **Observed**: Plan v5 §Test infrastructure committed FOUR `cfg(test)` knobs (`LOCK_RETRY_DELAY_MS`, `HEARTBEAT_INTERVAL_MS`, `poison_for_testing`, `fail_init_after_create_table`). Session 01 landed ONE genuinely-usable knob (`PHOTOHELPER_HEARTBEAT_INTERVAL_MS` env-var override, deterministic post-R2-T6 rewrite with the new heartbeat granularity fix at R2-T4) plus ONE dead-public-API placeholder (`Catalog::open_with_retry_delay` — `pub fn` with `#[doc(hidden)]`, but NO test calls it; per R2-T15 it sits as dead code awaiting the row-13 cross-process file-lock test). The remaining two (`poison_for_testing`, `fail_init_after_create_table`) plus the `trybuild` compile-fail test for plan row 6 plus the rows listed in the title ship without coverage. Behavioral coverage is ~36/50 plan rows = 72%.
- **Why it matters**: Each uncovered plan row is a load-bearing claim with no regression guard. The mutex-poison ROLLBACK path (R3.T5 fix), the schema-init transactional path, the cross-process file-lock test, and per-photo error context (now via `Error::Io { path }` + `Error::CatalogInsert { photo_id }` structured variants — NOT via `.with_context`, which R1.T10 deleted along with the no-op `ContextForPath` trait) all rely on convention. A future refactor that drops the ROLLBACK or the structured-error discipline will not fail any test.
- **Owner**: session 02 (alongside real CR3 fixtures + LibRaw integration the deferred rows benefit from). **Binding trigger** (rewritten): session 02 lands `poison_for_testing` + tests `{6, 12, 13, 14, 17, 18, 19, 34, 39, 42, 43, 49}` (12 rows; row 48 is closed by the R2-T6 deterministic heartbeat test; row 17 hardlink was missing from the prior list and is restored). Session 02's first plan commit MUST enumerate which of these rows it intends to cover and explicit DN cross-references for any deferred further.
- **R2 update**: row 48 (`heartbeat_fires_during_ingest_when_interval_is_short`) is now genuinely closed per the R2-T6 rewrite; the prior "weakly-deterministic" claim was incorrect because the test asserted on the unconditional summary line `walked: 1`.
- **Status**: open

### DN-009 — `scripts/verify-review-artifact.sh` (bash port of fox's mjs enforcer) (2026-05-28, session 1)

- **Observed**: The harness sync at `02d43d1` upgraded our `eight-agent-review` SKILL.md to require three YAML blocks at the top of every review artifact (`session_config`, `plugin_availability`, `verification`). Fox's upstream has a `scripts/verify-review-artifact.mjs` enforcer wired into `verify_project.sh` + `.husky/pre-commit` that parses these blocks and validates schema invariants. Photohelper's SKILL.md notes the YAML blocks but has no automated enforcement — the discipline is currently advisory.
- **Why it matters**: Without enforcement, the YAML markers will drift (missing fields, wrong schema_version, parse failures) and the verification machinery loses its audit value. Per the §0 precondition gate intent, a downgraded review with `gate_state: downgraded-no-prompt` should be visually distinguishable in the artifact AND CI-detectable.
- **Owner**: future session that ships a `scripts/verify-review-artifact.sh` bash equivalent, wired into `just ci` (after `verify-state.sh`) and `.pre-commit-config.yaml`'s pre-push stage. Binding trigger: "before the first review artifact lands on `main` post-this-session" OR "by 2026-09-01," whichever first.
- **Status**: open

### DN-010 — session-pause skill assumes `HANDOFF_REPORT.md` at repo root (2026-05-28, session 1)

- **Observed**: The `session-pause` SKILL.md (ported from fox at `02d43d1`) references `HANDOFF_REPORT.md` at the repo root — which matches our layout (we put it at `./HANDOFF_REPORT.md`, not `./docs/HANDOFF_REPORT.md` like fox). The other ported references (`SESSION-STATE.md`, `docs/discovery-notes.md`, `TECH-DEBT.md`) match our layout. Verified during the harness sync; flagging here so a future audit that compares the two harness families' file-placement choices has the breadcrumb.
- **Why it matters**: If a future harness sync from fox lands a `session-pause` upgrade, the path translation has to be re-applied. Document the divergence point.
- **Owner**: future harness sync.
- **Status**: open (informational; no action required this session).

### DN-013 — Windows LibRaw cross-compile audit deferred to v0.2 (2026-05-28, session 2)

- **Observed**: Session 02 lands LibRaw FFI for Canon R8 CR3 on Linux x86_64 + macOS arm64. Windows cross-compile from macOS arm64 → `x86_64-pc-windows-msvc` is non-trivial: LibRaw uses C++ idioms that depend on the MSVC C++ standard library (`libcxx` vs MSVC STL ABI differences), and the LibRaw build system (autoconf + custom Makefile alternatives) is GNU-toolchain-shaped, not MSBuild-shaped. The plan's §Out of scope row defers Windows to v0.2; this DN records the binding trigger so the work isn't lost.
- **Why it matters**: v0.1 ships Linux + macOS binaries only. Windows users have no path until v0.2. Documenting the deferral with a binding trigger prevents "v0.2 silently slips because nobody owns it" failure mode the No-Acceptable-Trade-offs Policy is designed to catch.
- **Owner**: v0.2 release-planning session OR first Windows-using contributor.
- **Binding trigger**: by v0.2 cut OR first PR from a contributor with `target = x86_64-pc-windows-msvc` in their CI matrix, whichever first. The audit MUST cover: (a) does LibRaw 0.21+ cross-compile cleanly from macOS arm64 with `cargo build --target x86_64-pc-windows-msvc` + the chosen build mechanism (vendored cmake vs system pkg-config), (b) does the resulting binary statically link LibRaw (verifiable via `objdump`), (c) Windows path-encoding boundary: `open_file_w` with `\\?\` prefix for paths >MAX_PATH (260 chars) (cross-ref to PR1-T20's FFI path encoding finding from `docs/code-reviews/session-02-plan-round1.md`).
- **Status**: open

### DN-014 — Other RAW formats (CR2 / NEF / ARW / RAF / ORF / RW2 / DNG) deferred to first non-Canon camera profile (2026-05-28, session 2)

- **Observed**: `crates/photohelper-cli/src/commands/ingest.rs:27` declares `RAW_EXTS = &["cr3", "cr2", "arw", "nef", "raf", "orf", "rw2", "dng"]` — the walker admits 8 RAW extensions but v0.1 supports only Canon R8 CR3. Session-02 plan-review `PR1-T1` flagged that the original `parse_exif_for(path, extension)` dispatch silently routes non-CR3 RAW to kamadak-exif (which can't parse any of them on DN-006/DN-011's evidence). Plan v2 narrows `RAW_EXTS` to `["cr3"]` for v0.1 (OR routes all through LibRaw — TBD per PR1-T1 remediation); this DN records the binding trigger for re-expanding `RAW_EXTS` when the next camera profile lands.
- **Why it matters**: If the dispatch decision is "narrow RAW_EXTS to `["cr3"]`," then adding a Sony camera profile (which uses ARW) requires re-expanding `RAW_EXTS` to `["cr3", "arw"]` AND verifying LibRaw extracts EXIF for the fixture set AND writing the camera-registry entry AND adding integration tests. Without a tracked binding trigger, the re-expansion gets forgotten and the new camera silently routes through whatever placeholder branch the dispatcher has.
- **Owner**: session that adds the second `CameraProfile` (likely a Sony / Nikon / Fuji body in v0.3 or v0.4).
- **Binding trigger**: first session whose plan includes a `CameraProfile` implementation other than `CanonR8`. The plan-review for that session MUST include "expand `RAW_EXTS` to include `<new format ext>` AND verify LibRaw EXIF on fixtures of that format AND add integration test for the new dispatch path" as a §Deliverables item.
- **Status**: open

### DN-015 — Heartbeat-thread `.join()` cleanup binding trigger fired by session 02 plan-review (2026-05-28, session 2)

- **Observed**: TD-003 (heartbeat thread not `.join()`-ed at end of `run_ingest`) has a binding trigger including "test-flake surfaces on CI from stderr-ordering instability." Session-02 plan-review `PR1-T4` surfaced a related concern (R2-T18's heartbeat-death WARN path lacks regression-test coverage because the `panic_for_testing` knob was deferred). The session-02 plan v2 commits the `panic_for_testing` knob (closing R2-T18 fully) but TD-003's `.join()` cleanup itself is NOT fired by this session — we're not touching `run_ingest`'s post-walk teardown. Recording for cross-reference clarity so a future audit doesn't conflate the two concerns.
- **Why it matters**: Distinguishes the `panic_for_testing` knob (session 02 ships this — tests the heartbeat-death WARN path) from TD-003's `.join()` cleanup (session 04+ — eliminates the zombie-output race). Both touch the heartbeat thread but at different abstraction levels.
- **Owner**: session 04+ per TD-003's existing binding trigger; this DN is informational.
- **Status**: open (informational; no action required this session; cross-references TD-003).

### DN-016 — Canon CR3 EXIF timezone recovery deferred to v0.2 develop pipeline (2026-05-28, session 2)

- **Observed**: Canon CR3 (and most EXIF-bearing formats) store `DateTimeOriginal` as a naïve wall-clock string with no timezone offset; LibRaw's `imgdata.other.timestamp` is `time_t` interpreted as wall-clock-as-UTC by default. A photo taken in Tokyo (UTC+9) is stamped 9 hours earlier in the catalog than wall-clock truth. Session 02's `RawExif::capture_time_unix_seconds()` field is `Option<i64>` documented inline as "UTC assumption" but the actual local-time recovery (via EXIF tags `OffsetTimeOriginal` + `SubSecTimeOriginal` if present, OR via GPS `GPSDateStamp` + `GPSTimeStamp` if present) is deferred.
- **Why it matters**: Catalogs ingested by v0.1 have systematically wrong `capture_time_unix_seconds` for photos taken outside the UTC zone the user's machine is configured for. A user reorganizing photos by capture time sees ordering that doesn't match wall-clock. Future v0.2 develop pipeline must offer timezone recovery; v0.1 ingest behavior is the eventual migration target via `photohelper ingest --reindex` or similar.
- **Owner**: session that lands the develop pipeline (likely session 04+) — owns adding timezone-aware capture-time recovery + the catalog column shape change (likely `capture_time_offset_seconds: Option<i32>` or `capture_time_utc: i64 + capture_time_offset_seconds: Option<i32>`). Session 03's cull pipeline does NOT touch this surface so does not need to address.
- **Binding trigger**: session 04+'s first plan commit MUST include "EXIF timezone-aware capture-time recovery" as a §Deliverables item IF the develop pipeline exposes any time-zone-sensitive feature (e.g. "show photos taken on date X local time"); if not, deferral rolls to v0.2.
- **Status**: open (informational; v0.1 limitation documented inline in `photohelper-raw::RawExif::capture_time_unix_seconds()` rustdoc + cross-ref to this DN).

### DN-017 — WhiteBalance rebalance + per-illuminant color-matrix recovery deferred to develop pipeline (2026-05-28, session 2)

- **Observed**: Session 02 ships `photohelper-raw::RawImage` with `as_shot_white_balance: WhiteBalance` (the at-capture WB the camera computed) AND `color_matrix: ColorMatrix` (the camera's CamRGB→XYZ_D65 matrix interpolated at the as-shot illuminant). This is sufficient for "render the photo as the camera intended" but NOT for the Lightroom-equivalent develop pipeline's WB-edit feature (drag the temperature slider — recompute with a different WB) or per-illuminant color-recovery (Adobe DNG ColorMatrix1 + ColorMatrix2 interpolated by CCT). LibRaw exposes the additional matrices (`imgdata.color.WB_Coeffs[256][4]` for EXIF-preset WBs; `imgdata.color.WBCT_Coeffs[64][5]` for color-temperature curves; `imgdata.color.cmatrix` for the cross-illuminant matrices) but session 02 only consumes the as-shot pair.
- **Why it matters**: A v0.1 photohelper user editing WB in the develop pipeline gets "as-shot WB only" — they cannot recompute white-balance from a custom illuminant. This is acceptable for v0.1 (no develop pipeline yet) but binds the eventual session 04+ develop pipeline to extending `RawImage` with WB-preset + WB-curve fields.
- **Owner**: session 04+ (develop pipeline). Owns extending `RawImage` with: `wb_presets: HashMap<WbTag, WhiteBalance>`, `wb_color_temperature_curves: ColorTemperatureCurves`, `color_matrices: HashMap<Illuminant, ColorMatrix>` (or similar — exact shape decided at session-04 plan-review).
- **Binding trigger**: session 04+'s first plan commit MUST include "WhiteBalance + ColorMatrix surface extension for develop pipeline WB-edit" as a §Deliverables item; the v0.1 `RawImage` shape is `#[non_exhaustive]` to allow the extension without a breaking change.
- **Status**: open (informational; v0.1 limitation documented inline in `photohelper-raw::RawImage` rustdoc + cross-ref to this DN).

### DN-019 — TD-003 manifests empirically: `heartbeat_fires_during_ingest_when_interval_is_short` fails 5/5 on apple-silicon (2026-05-28, session 2, session-pause gate run)

- **Observed**: `just session-end` (= `just ci`) ran at session-02 plan-review pause time, on apple-silicon (the author's M-series Mac). The single test `crates/photohelper-cli/tests/cli.rs::heartbeat_fires_during_ingest_when_interval_is_short` fails consistently 5/5 runs. Ingest of 80 stub CR3 files completes in ~0.28s; the spawned heartbeat thread doesn't flush its first `eprintln!("[heartbeat] ...")` before the parent process exits.
- **Why it matters**: this is the **empirical manifestation of TD-003** ("heartbeat thread is not `.join()`-ed at end of `run_ingest`; leaks past summary"; binding trigger includes "test-flake surfaces on CI from stderr-ordering instability"). At session-01 R2 remediation commit time, the test passed (`63 tests pass` per SESSION-STATE.md); on apple-silicon today, ingest is fast enough that the race fires deterministically against the heartbeat thread. The test was always borderline; this machine pushed it over.
- **Owner**: TD-003 already owns the fundamental fix (join the heartbeat thread on `JoinHandle` with a `Condvar` wake-up so the join completes within one `granularity` cycle). DN-019 records the empirical evidence so TD-003's binding trigger ("test-flake surfaces on CI") is now demonstrably FIRED — TD-003 moves from "deferred" to "binding trigger fired, fix required for session 02 implementation green CI."
- **Implication for session 02**: the LibRaw FFI work doesn't touch `run_ingest`'s teardown, but `just ci` green is Acceptance criterion 1. Session 02 implementation MUST either (a) fix TD-003 (small: ~15 LoC per the existing TD-003 fundamental-fix spec) OR (b) flake-tolerate the test (mark `#[ignore]` with explicit TD-003 cross-ref + DN-019 cross-ref + commit-message note that the test surface is being deferred until TD-003 closes). Path (a) recommended — it's small AND closes TD-003 in lockstep with session 02's LibRaw work.
- **Status**: closed 2026-05-28 (session 2). Path (a) shipped: `HeartbeatStop` (`Mutex<bool>` + `Condvar`) replaces the bare `AtomicBool`; `thread::Builder::new().name("ph-heartbeat").spawn(...)` carries a named handle; `run_ingest` calls `stop.signal()` then `heartbeat_handle.join()` so every `[heartbeat]` line flushes BEFORE the summary line; `heartbeat_loop` is now tick-first-wait-after so a fast ingest still emits a liveness signal when `stop.signal()` lands inside the OS thread-startup-latency window (the empirical race surfaced here). Test now passes 10/10 on the same apple-silicon machine that produced this DN. `just ci` green. TD-003 marked Closed.

### DN-018 — LibRaw vendored-tarball SHA-256 + CVE-posture-as-of-pin audit owner (2026-05-28, session 2)

- **Observed**: Session 02 commits to vendoring LibRaw `=0.21.4` at `crates/photohelper-raw/vendor/libraw-0.21.4.tar.gz` with SHA-256 verified at build-time per Acceptance criterion 7. The decision-doc-0002 records the pin. But the CVE-posture-at-pin-time check (was the vendored version CVE-clean as of the pin date?) is a one-shot human-eyes-on-the-MITRE-feed step that the plan doesn't assign an owner OR a record-keeping artifact. TD-004 covers the ongoing monitoring; this DN covers the up-front pin-time check.
- **Why it matters**: Pinning a LibRaw version that already has open CVEs against it (without the author knowing) ships a known-vulnerable binary. The asymmetry is: TD-004's binding trigger is "ongoing CVE monitoring" which assumes a clean starting state; if the starting state is dirty, TD-004 won't catch it because no NEW disclosure happens.
- **Owner**: Deliverable 0 (pre-flight feasibility probe) owner. During pre-flight, the author runs the LibRaw EXIF extraction probe AND checks the MITRE CVE feed + LibRaw GitHub Security Advisories for any open CVE affecting `=0.21.4` (or the chosen version if the pre-flight chooses a different patch). Result recorded in `docs/analysis/ANL-001-libraw-cr3-preflight.md § CVE-posture-as-of-pin` subsection.
- **Binding trigger**: Deliverable 0's pre-flight commit. ABORT trigger: if any open CVE affects the chosen version, escalate to plan-review v4 to either (a) pin a different version, (b) backport the fix via vendored-source patch, or (c) defer the LibRaw landing session.
- **Status**: closed 2026-05-28 (session 2, Deliverable 0). Pre-flight artifact landed at `docs/analysis/ANL-001-libraw-cr3-preflight.md`. CVE-posture clean per MITRE NVD (`services.nvd.nist.gov/rest/json/cves/2.0?keywordSearch=libraw`: 0 CVEs published since 2023-01-01) AND per LibRaw GitHub Security Advisories ("There aren't any published security advisories"). Pin escalated from plan-default `=0.21.4` to `=0.22.1` because LibRaw 0.22.1 carries six TALOS-2026-* fixes + two CR3-parser-specific hardenings ("zero all buffers before fread"; 64-bit unsigned file offsets) that did NOT backport to 0.21.5b — 0.21.x is effectively EOL. User consulted under No-Acceptable-Trade-offs Policy; approved 0.22.1 pin. Plan amended to v3.2 in lockstep.

### DN-020 — Stub-subcommand `planned for session 02` messages are stale post-merge (2026-05-28, session 02.5)

- **Observed**: After session 02 merged with LibRaw FFI for the `ingest` subcommand, the other 6 stubs (`camera`, `cull`, `develop`, `export`, `run`, `models`) still emit `"<subcommand>: not yet implemented (planned for session 02)"`. Session 02 NEVER scoped any of these — they were stubbed in session 01 with the placeholder text and the message stayed.
- **Why it matters**: Operators run these to see what's available and get a stale promise. Trust erodes.
- **Owner**: session 03's first chore commit. One-line edit per stub source file — drop "planned for session 02" or replace with "not yet implemented; see SESSION-STATE.md for the current roadmap".
- **Binding trigger**: session 03 session-start sweep — the SESSION-STATE.md Goal block surfaces this as a quick-win first-commit candidate.
- **Status**: **CLOSED** — session 03 D6 commit (`chore(cli): refresh stub-subcommand messages (closes DN-020)`). `stub()` rewritten to emit `"not yet implemented in v0.1 (ingest + cull only); see README.md for the current scope."`. `planned_in` parameter removed; 6 call sites updated; test assertions tightened to the new wording; negative test added confirming `cull --help` does not show the stub message.

### DN-021 — Two-shell PATH drift footgun: scripts merged on `main` but invisible to a separate terminal session that hasn't pulled (2026-05-28, session 02.5)

- **Observed**: Within the session 02.5 sub-sessions (cleanup-catalog-script + list-catalog-script), the user tried to invoke a freshly-merged script from their own zsh terminal and got `zsh: no such file or directory` despite the file being on `main`. Diagnosis: their interactive terminal session was a separate working checkout (or hadn't `git pull`-ed) from the Claude Code session's working copy.
- **Why it matters**: Two-shell workflows (Claude Code in one window, the user's daily terminal in another) silently diverge unless the user remembers to `git pull --ff-only origin main` after every merge. The current Quickstart in `README.md` does not call this out.
- **Owner**: future Quickstart-section refinement (session 03 docs-pass or whenever the README is touched next). One-paragraph addition under § Development would suffice.
- **Binding trigger**: next README touch OR next operator-reported "no such file or directory" repro (informational tracking only).
- **Status**: open (informational; no immediate harm beyond the user's confusion in this one session).

### DN-022 — LibRaw demosaic algorithm selection for NIMA preprocessing (2026-05-28, session 03)

- **Observed**: Session 03 adds `photohelper-raw::read_raw_rgb(path) -> Result<RgbImage>` via LibRaw's `dcraw_process` + `dcraw_make_mem_image` pipeline. LibRaw's default demosaic algorithm is AHD (Adaptive Homogeneity-Directed). NIMA was trained on consumer JPEGs produced by camera-native demosaic (typically a high-quality algorithm); the choice of demosaic algorithm may shift NIMA's score distribution slightly. LibRaw exposes alternative algorithms: AMaZE, AAHD, VNG4, DCB, and others via `imgdata.params.user_qual` (0=linear, 1=VNG, 2=PPG, 3=AHD, 11=AMaZE, 12=AAHD).
- **Why it matters**: v0.1 uses the LibRaw default (AHD). If a future session's NIMA score distribution comparison vs. camera-native output shows systematic bias, the demosaic choice is the most likely cause. The develop pipeline (session 04+) may also want explicit algorithm selection for quality rendering.
- **Owner**: session 04+ develop pipeline OR any session whose plan-review surfaces NIMA score bias as a quality regression. Cross-reference TD-012 (stop-gap for AHD default).
- **Binding trigger**: session 04+'s first plan commit MUST include "demosaic algorithm selection for develop pipeline" as a §Deliverables item; if not, deferral rolls. Alternatively fires if NIMA score regression is documented in user feedback before session 04.
- **Status**: open (informational; v0.1 uses LibRaw default AHD; TD-012 is the stop-gap tracker).

### DN-023 — `cull_scores.photo_id` ON DELETE CASCADE absent from v2 schema (2026-05-28, session 03)

- **Observed**: `cull_scores.photo_id REFERENCES photos(id)` with no `ON DELETE CASCADE`. In v0.1 there is no delete path for `photos` rows, so this is deliberately absent. If a future session adds photo deletion (`photohelper purge` or similar), orphan `cull_scores` rows would accumulate silently unless a delete path is also added to `cull_scores`. The FK *without* CASCADE means deletion of a `photos` row would fail (FK violation), which is actually a safer default for v0.1 — it prevents accidental orphaning.
- **Why it matters**: If a delete path for photos is added, the `cull_scores` FK behavior must be explicitly decided: (a) `ON DELETE CASCADE` (scores auto-deleted with the photo), (b) `ON DELETE SET NULL` (scores orphaned for audit), or (c) keep the violating FK (the delete path must also clean `cull_scores` first). No correct answer exists without the delete use-case defined.
- **Owner**: session that adds a delete path for `photos` rows. Decision-doc 0002 records this as a known open choice.
- **Binding trigger**: first session whose plan includes `photohelper purge` or any `DELETE FROM photos` path. The plan-review for that session MUST include a resolution for `cull_scores.photo_id` FK behavior.
- **Status**: open (informational; no v0.1 delete path exists; documented in decision-doc 0002).

### DN-024 — MobileCLIP dup-detection compute deferred to session 04+ (2026-05-28, session 03)

- **Observed**: Session 02's plan originally scoped `dup_groups` as a v2 schema table. Session 03 plan-review Round 1 (PR1-T30) found the table was under-specified (no dimension, no float-format, no model-identity column) and would ship with no writer — a schema-only stop-gap with no current value. Per decision-doc 0001:129 ("A single-statement migration doesn't justify framework overhead"), shipping a table with zero consumers also violates the spirit of the v2 migration: every table in v2 should have at least one producer session 03 can point to.
- **Why it matters**: When MobileCLIP (or an alternative embedding model) arrives in session 04+, the schema design can be done correctly with the consumer's actual shape known. A premature schema must be migrated again (v3+), wasting a migration slot. Session 03 ships only `cull_scores` in v2.
- **Owner**: session 04+ that adds the MobileCLIP producer. That session's plan MUST include: embedding table schema (`model_slug TEXT`, `dim INTEGER`, `quantization TEXT`, `embedding BLOB`), dimension validation at insert time, `dup_clusters` table for group assignment (separate from per-photo embeddings), and `v2→v3` migration.
- **Binding trigger**: session 04+'s first plan commit IF MobileCLIP dup-detection is in scope for that session. If not, defers until the session that introduces the first embedding producer.
- **Status**: open (dup_groups deferred from session 03 per PR1-T30 remediation; schema will be defined when the compute arrives).

### DN-025 — NIMA cross-platform score tolerance (apple-silicon vs Linux x86_64) (2026-05-28, session 03)

- **Observed**: ort CPU inference is deterministic per binary (same arch, same model, same ort version → same f32 output). However, f32 arithmetic is NOT bit-identical across CPU architectures: apple-silicon (arm64) and Linux x86_64 may produce NIMA scores differing by ~1e-3 due to FMA instruction presence/absence, SIMD lane ordering, and compiler vectorization differences. The golden-vector fixture committed on apple-silicon will not match the x86_64 CI score exactly.
- **Why it matters**: If the Linux x86_64 CI runner asserts `score == golden` (exact), the test flakes non-deterministically across ort RC updates that change instruction scheduling. Session 03 mitigates with: (a) `±1e-3` tolerance on apple-silicon golden, (b) band assertion `score ∈ [3.0, 9.0]` on Linux x86_64 CI (based on actual D0 fixture scores ± safety margin). This is a known limitation of cross-arch f32 inference.
- **Owner**: session that adds a second target architecture to CI (e.g., Linux arm64 or Windows x86_64). That session must extend the golden-vector fixture or switch to distribution-based assertions (e.g., assert that the score percentile within the NIMA training distribution is within ±5th percentile across architectures).
- **Binding trigger**: first session adding a second native CI runner OR a user bug report of NIMA test flaking on a specific arch. OR by 2027-01-01 if the band assertion on Linux x86_64 CI fails 3+ consecutive runs (likely means the band is too tight for the chosen model's score range).
- **Status**: open (informational; session 03 mitigates with tolerance + band; cross-arch full convergence deferred).

### DN-026 — No NIMA ONNX model with explicit permissive license found (2026-05-28, session 03 D0 ABORT)

- **Observed**: Session 03 D0 pre-flight (ANL-002) searched for a NIMA ONNX model with an explicitly stated MIT, Apache-2.0, or CC-BY-4.0 license to use as the photohelper-ai aesthetic scorer. Platforms searched: HuggingFace (ONNX+aesthetic filter, ONNX+nima filter), GitHub (nima+onnx query), PINTO0309 model zoo, ONNX Model Zoo. Only one candidate found: `cromsc/nima-mobilenet-aesthetic` on HuggingFace (file: `nima_mobilenet_aesthetic.onnx`, uploaded 2026-03-31). This model has **no license file, no license tag, and no model card** — provenance to the weight source is unconfirmed. Under copyright law, absent an explicit license, the work is all-rights-reserved.
- **Why it matters**: `docs/plans/session-03.md § D0` requires "a NIMA ONNX model with a clear license + provenance + reproducible SHA-256" and ABORTs if the license is not in {MIT, Apache-2.0, CC-BY-4.0}. Without a clear license, the ABORT condition fires: session 03 cannot wire ort, commit the model binary, implement D1–D4 (AI culling pipeline), or implement D2 (catalog v1→v2 migration with cull_scores). See ANL-002 for full analysis.
- **Owner**: session 04+ that adds the AI culling pipeline. That session MUST resolve this blocker before any ort dep is wired. Two resolution paths:
  - **Path A (recommended)**: Export `idealo/image-quality-assessment` (Apache-2.0) MobileNet aesthetic Keras weights to ONNX via `tf2onnx` (also Apache-2.0). Record derivation script + SHA-256 sidecar + explicit Apache-2.0 license alongside the model binary. This path is self-sufficient and reproducible.
  - **Path B**: Contact `cromsc` on HuggingFace requesting an explicit Apache-2.0 or MIT license declaration. Proceed only after the LICENSE file is confirmed live in the repository.
- **Binding trigger**: session 04+ plan-review Round 1 MUST include a §D0 resolution with either (A) the `tf2onnx` export commit SHA + SHA-256 verified or (B) the `cromsc` license-confirmed commit SHA. No ort dep may land until this fires.
- **Status**: **BLOCKER** — D0 ABORT triggered. AI culling pipeline (D1–D4) halted until resolved. ANL-002 records the full pre-flight findings (CVE-posture = clean, threading-semantics = `&mut self` confirmed).
