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

### DN-006 — kamadak-exif cannot parse Canon R8 CR3 (both synthetic AND real CR3 fixtures) (2026-05-28, session 1; upgraded 2026-05-28 by DN-011)

- **Observed**: Session 01 implementation smoke test on `/tmp/ph_demo` and integration test row 32 confirmed: `kamadak-exif 0.6` returns "Unknown image format" when fed raw `0xCC`-byte synthetic CR3 fixtures, then yields zero `Fields` on subsequent parse. **DN-011 escalates**: the user's production run on 371 real Canon R8 CR3s (`/Users/ph/Pictures/tests`, 2026-05-28 15:32:52) reproduced the same failure on every single file. kamadak-exif is non-functional for CR3 in v0.1, full stop — not just for synthetic fixtures.
- **Why it matters**: The v0.1 fallback (NULL EXIF columns + `no-exif` counter bump + camera_slug NULL) is no longer a "best-effort fallback" — it IS the production behavior for all CR3 files. The `CameraRegistry::for_exif` lookup never succeeds; the `--strict` flag (per R2-T12) now correctly fails on no_exif > 0, making strict mode effectively unusable for CR3 in v0.1. Session 02's LibRaw integration is the only remediation path.
- **Owner**: session 02 — LibRaw EXIF extraction is no longer optional (no longer "if kamadak-exif fails, fall back to LibRaw"; it IS the primary path for CR3). Confirm with: (a) LibRaw can extract Make/Model/Orientation/CaptureTime from a real Canon R8 CR3, then (b) flip integration test row 32's assertions from `is_none()` to `Some("canon-r8")` AND make the strict-mode tests pass on real CR3 fixtures.
- **Status**: open; severity upgraded to "v0.1 known limitation, session-02 critical-path dependency."

### DN-011 — DN-006 extends to ALL real Canon R8 CR3s (not just synthetic fixtures) (2026-05-28, session 1)

- **Observed**: User executed `photohelper ingest /Users/ph/Pictures/tests --strict` against 371 real Canon R8 CR3 files (2026-05-28 15:32:52). Result: every single CR3 emitted `WARN photohelper::commands::ingest: EXIF parse failed error=EXIF parse error at <path>: Unknown image format`. Summary: `walked: 371, no-exif: 370, ingested: 0, already-catalogued: 370, skipped (non-RAW): 1`. The synthetic-CR3 limitation from DN-006 is NOT a fixture-quality artifact; kamadak-exif genuinely cannot parse the Canon CR3 ISO-BMFF container format in v0.1.
- **Why it matters**: Three downstream consequences. (1) Plan v5's "DN-006 fallback" language baked into ~23 places quietly stops being a "best-effort fallback" and becomes "the actual production behavior." (2) Session 02's LibRaw work is elevated from "RAW pixel decode" to "EXIF read + RAW pixel decode" — LibRaw must extract Make/Model/Orientation/CaptureTime, not just pixel data. (3) Session 01's `--strict` mode is effectively useless for CR3 in v0.1 (per R2-T12 fix: strict now fails on no_exif > 0, which means every CR3 ingest in strict mode fails). This is intentional escalation — strict means strict — and the only fix is LibRaw EXIF.
- **Owner**: session 02 (LibRaw EXIF). Also surfaces a v0.1 user-facing limitation: any session-01 binary released to users will be `--strict`-unusable for CR3.
- **Binding trigger**: session 02's first plan commit MUST include "LibRaw EXIF extraction (Make/Model/Orientation/CaptureTime)" as a §Deliverables item; if it doesn't, the session-02 plan-review must reject.
- **Status**: open (informational; ties DN-006 to the user's production evidence).

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
