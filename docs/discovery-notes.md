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

### DN-001 — LibRaw LGPL static-link distribution mechanics (2026-05-27, session 0)

- **Observed**: The plan locks LibRaw 0.21+ as the RAW decoder (CR3 support for Canon R8). LibRaw is dual-licensed LGPL-2.1 / CDDL-1.0; we plan to statically link the LGPL build. LGPL static linking requires the distributor to offer "the means to relink" — typically a tarball of object files or build inputs alongside each release binary.
- **Why it matters**: We need to know what artifact (e.g. `vendor/libraw-X.Y.Z.tar.gz` per release) ships in GitHub Releases to satisfy LGPL §6(b). Affects the release workflow and the release notes template.
- **Owner**: session that introduces `photohelper-raw` LibRaw FFI (likely session 02) + the eventual release-engineering session.
- **Status**: open

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

### DN-006 — kamadak-exif cannot parse synthetic CR3 ISO-BMFF containers (2026-05-28, session 1)

- **Observed**: Session 01 implementation smoke test on `/tmp/ph_demo` and integration test row 32 confirm: `kamadak-exif 0.6` returns "Unknown image format" when fed raw `0xCC`-byte CR3 fixtures, then yields zero `Fields` on subsequent parse. The synthetic-CR3 path was the only one available pre-real-fixtures (plan v5 §Out of scope item 7 defers real CR3 fixtures to session 02 via git-lfs). What's still UNVERIFIED: whether kamadak-exif can parse a *real* Canon R8 CR3 (ISO-BMFF container with EXIF inside a `uuid` box). The synthetic test is weak evidence — the bytes aren't a real CR3 at all.
- **Why it matters**: If kamadak-exif also fails on real CR3, the v0.1 fallback (NULL EXIF columns for every CR3) is the de-facto behavior until session 02 wires LibRaw as the alternate EXIF source. The `CameraRegistry::for_exif` lookup needs `make` + `model` to succeed; without them every CR3 row's `camera_slug` stays NULL even if the user has a Canon R8.
- **Owner**: session 02 — when real CR3 fixtures land via `git-lfs`, re-run the pre-flight against a real Canon R8 CR3 and (a) confirm parse failure → wire LibRaw EXIF, OR (b) confirm parse success → flip integration test row 32's assertions from `is_none()` to `Some("canon-r8")`.
- **Status**: open

### DN-007 — `rusqlite` pinned at 0.32 instead of plan-v5's 0.40 target (2026-05-28, session 1)

- **Observed**: Plan v5 §Dependencies committed `rusqlite 0.40`. Session-01 implementation shipped `rusqlite 0.32` (the version that lets the `bundled` SQLite amalgamation compile cleanly under the rest of the dep graph on 2026-05-28). The 0.32 bundle is ~14 months stale and per R2.T1 (plan-review) "will trip SQLite CVE advisories." `cargo audit --deny warnings` is currently clean on 0.32, but the staleness is a real future-risk.
- **Why it matters**: Each SQLite CVE released after the 0.32 bundle's cutoff is a candidate for `cargo audit` failure. The longer we sit on 0.32, the more likely a CI break + emergency-bump scramble.
- **Owner**: TD-002 (filed; binding trigger = "bump by 2026-08-01 OR before session 02 adds new schema columns, whichever first"). Cross-reference this DN.
- **Status**: open

### DN-008 — Missing `cfg(test)` knobs for plan-row tests 6, 12, 13, 14, 18, 19, 34, 39, 42 partial, 43 partial, 49 (2026-05-28, session 1)

- **Observed**: Plan v5 §Test infrastructure committed FOUR `cfg(test)` knobs (`LOCK_RETRY_DELAY_MS`, `HEARTBEAT_INTERVAL_MS`, `poison_for_testing`, `fail_init_after_create_table`). Session 01 landed TWO: `Catalog::open_with_retry_delay` (one-shot constructor overload) + `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` env-var override (closes test row 48 in a weakly-deterministic way). The remaining two (`poison_for_testing`, `fail_init_after_create_table`) plus the `trybuild` compile-fail test for plan row 6 plus 9 other plan rows ship without coverage. Behavioral coverage is ~36/50 plan rows = 72% (per session-end Round 1 test-analyzer finding).
- **Why it matters**: Each uncovered plan row is a load-bearing claim with no regression guard. The mutex-poison ROLLBACK path (R3.T5 fix), the schema-init transactional path, the cross-process file-lock test, and the per-photo `.with_context()` boundary all rely on convention — a future refactor that drops the ROLLBACK or the with_context will not fail any test.
- **Owner**: session 02 (alongside real CR3 fixtures + LibRaw integration the deferred rows benefit from). Binding trigger: "session 02 lands `poison_for_testing` + tests 6/12/13/14/18/19/34/39/42/43/49 OR files explicit DN cross-references for each row deferred further."
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
