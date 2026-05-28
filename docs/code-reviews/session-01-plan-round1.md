# session-01 plan-review Round 1

> Per `docs/quality-assurance.md § Plan-review protocol`. Cadence A → Tier 5
> (plan review), full 8-agent suite fired in parallel against
> `docs/plans/session-01.md` (revision committed in `1e636ec`).
>
> Findings grouped by **theme**, not by agent (per
> `docs/quality-assurance.md § Consolidation discipline`). When multiple agents
> flagged the same theme, that overlap is the priority signal — agents cited in
> brackets after each theme heading.

## Summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 6 | Block plan acceptance until addressed in remediation. |
| **HIGH** | 4 | Must address before Round 2 reviews (failing to address risks Round-2 regressions). |
| **MEDIUM** | 3 | Address during remediation; deferral to TECH-DEBT only with a binding trigger. |
| **LOW** | 1 | Convenience polish; OK to fold into remediation. |
| **NOTES** (strengths) | 6 | Preserve through remediation; the Round-2 sweep should confirm. |

Agent suite: `general-purpose` (gp), `feature-dev:code-architect` (arch),
`feature-dev:code-reviewer` (rev), `pr-review-toolkit:type-design-analyzer`
(type), `pr-review-toolkit:silent-failure-hunter` (sfh),
`pr-review-toolkit:comment-analyzer` (com),
`pr-review-toolkit:pr-test-analyzer` (test),
`pr-review-toolkit:code-simplifier` (simp).

---

## Findings (by theme)

### Theme 1 — `PhotoId` design: input space, encoding, endianness, constructor safety [CRITICAL]

**Agents**: rev (CRITICAL), type (CRITICAL), arch (HIGH), gp (HIGH), com (HIGH), arch (LOW)

Six independent reads converge on `PhotoId` as the highest-leverage design hole.

- **Input space too narrow** — `BLAKE3(file_size || first_64KB)` is plausibly collidable for Canon R8 burst frames where size + initial container bytes are near-identical (arch HIGH). The catalog primary key being collidable defeats the content-addressing claim everything else relies on.
- **Endianness unspecified** — without `u64::to_le_bytes()` (or `to_be_bytes()`, pick one) for `file_size`, hashes drift across platforms (rev CRITICAL).
- **Render-width lie** — plan says "32-char base32." BLAKE3 32 bytes → 52 chars base32 (or 43 chars base64url). Documentation falsity that becomes a failing unit test (gp HIGH, com HIGH, arch LOW).
- **Constructor surface undefined** — newtype `PhotoId(pub [u8; 32])` would let any caller forge IDs, breaking the content-address invariant; the plan never says the inner field is private (type CRITICAL).
- **Edge-case behavior unspecified** — files < 64KB hash window: does `read_exact` panic, return short hash, or pad? Zero-byte files? Two files with identical first-64KB but different sizes (must the `file_size ||` prefix actually distinguish them)? (test HIGH)

**Remediation direction**: Lock the derivation spec: `BLAKE3(file_size.to_le_bytes() || mtime_unix_seconds.to_le_bytes() || first_64KB || last_64KB)` (adds entropy for bursts, survives FAT32 2s mtime per Theme 7); private inner field; constructors are `PhotoId::derive(path) -> Result<Self, Error>` (canonical) and `PhotoId::from_db_bytes([u8; 32]) -> Self` (catalog reconstruction); render as `base64url-nopad` (43 chars). Document the read-policy for short files (read up to min(64KB, len) for the prefix slot; last_64KB = empty if len ≤ 64KB). Update Test plan row 1 to reflect the actual encoding length and add collision/edge tests.

---

### Theme 2 — Speculative abstractions: `Pipeline` trait + `PipelineCtx` + `Sidecar` enum + `CancellationToken` [CRITICAL]

**Agents**: simp (CRITICAL), type (HIGH ×2), sfh NOTE (caveated positive), arch (MEDIUM)

The plan introduces a trait with one implementer, a context struct that bundles three params for that one implementer, a placeholder enum with one variant explicitly slated for replacement, and a cancellation token that nothing actually flips this session. Type-design analyzer pointed out the Sidecar return-type will be wrong by session 03 (culling returns scores, not sidecars); simplifier called it YAGNI; sfh (a positive note) liked the cancellation scaffold but the simplifier counter-argument is stronger because nothing exercises it.

Specific issues:
- `Pipeline` trait with one impl + speculative return shape — when CullStage arrives in session 03 it'll need a different return type anyway (arch MEDIUM, type HIGH).
- `PipelineCtx { catalog: &Catalog, tracing_span: &tracing::Span, cancel: &CancellationToken }` — `&tracing::Span` is non-idiomatic (owned + `.enter()` is the pattern), `&Catalog` Send/Sync story is unstated for rayon (`rusqlite::Connection` is `!Sync`), lifetime parameter is implicit (type HIGH).
- `Sidecar::Ingested { photo_id }` is a single-variant placeholder typed-redundant with returning `PhotoId` directly (simp MEDIUM).
- `CancellationToken` not exercised by any test or SIGINT handler this session — a dead invariant baked into the type signature (simp HIGH).

**Remediation direction**: Defer the trait. Session 01 ships `fn ingest_one(photo: &Photo, catalog: &CatalogWriter) -> Result<PhotoId, Error>` as a plain function. Drop `Sidecar` placeholder and `CancellationToken` from this session entirely — both land when the session that genuinely needs them arrives (Sidecar with session 04 XMP; cancellation with whichever session wires SIGINT). The `Pipeline` trait emerges in session 02 when `CullStage` arrives — the abstraction's shape will be constrained by two real implementers. Type-design HIGH on `&tracing::Span` and Send/Sync are absorbed by deletion.

---

### Theme 3 — Catalog architecture: placement, single-writer plumbing, panic isolation, lock contention [CRITICAL]

**Agents**: arch (CRITICAL placement), arch (HIGH plumbing), sfh (CRITICAL writer panic), sfh (HIGH lock contention), rev (CRITICAL TOCTOU), gp (HIGH internal contradiction), simp (HIGH unjustified complexity), simp (HIGH migration framework precedes need), arch (NOTE single-writer right), simp (NOTE bundled rusqlite right)

Multi-axis disagreement among the agents — needs explicit decision in remediation:

- **Placement** (arch CRITICAL): `Catalog` inside `photohelper-core` couples storage to the domain crate. Future swap (kv-store, remote DB) requires rewriting `core`. Recommendation: split to its own crate `photohelper-catalog` OR introduce a `trait CatalogBackend` in `core` with the SQLite impl as `core::catalog::sqlite`.
- **Single-writer thread vs `Mutex<Connection>`** (arch favors plumbing-with-fix, simp favors mutex): for v0.1's 2-file test, `Mutex<Connection>` in WAL mode is correct and 10× less code. The dedicated-writer pattern is justified only by profiling evidence we don't have.
- **Response channel semantics** (arch HIGH): if we keep the writer-thread pattern, the plan doesn't specify how a worker waits for the "row was new / pre-existing" reply — needs `oneshot::Sender<bool>` per request, OR the simpler mutex path.
- **Reads-through-write-channel contradiction** (gp HIGH): plan says reads go through the same channel for determinism; this serializes the whole walk and defeats `par_bridge`. UPSERT subsumes the pre-existence read anyway.
- **TOCTOU on `.photohelper/catalog.db` creation** (rev CRITICAL): two concurrent processes both see missing DB → both try `CREATE TABLE` → corruption. Needs file lock (`fs2::FileExt::lock_exclusive` on a sibling `.lock` file) OR SQLite `PRAGMA locking_mode=EXCLUSIVE`.
- **Writer panic = silent deadlock** (sfh CRITICAL): if writer thread panics, rayon producers block forever on a full channel. Needs `JoinHandle.join()` + a `Error::CatalogWriterPanic` surfaced. (Moot if we adopt the mutex path.)
- **Concurrent ingest lock contention** (sfh HIGH): SQLite default `SQLITE_BUSY` behavior with no explicit `busy_timeout` → unbounded hang. Needs an explicit timeout + clear error: "another photohelper process is writing to <catalog.db>; aborting."
- **Migration framework precedes need** (simp HIGH): one migration (0→1) doesn't justify a `migrate(conn: &mut Connection)` framework + `schema_version` table. Ship `CREATE TABLE IF NOT EXISTS` + `PRAGMA user_version = 1`; introduce the framework when migration 1→2 lands.

**Remediation direction**:
1. **Placement**: introduce `photohelper-catalog` as an 8th workspace crate; `core` keeps the domain types, `catalog` owns the persistence layer. (This is a *new file*, not a reshuffle — minimal blast radius.)
2. **Concurrency**: `Mutex<rusqlite::Connection>` in WAL mode, shared across rayon workers. Single dedicated writer thread is deferred to a later session with a TD if profiling ever proves the need.
3. **Migration framework**: defer. v0.1 ships `CREATE TABLE IF NOT EXISTS` + `PRAGMA user_version = 1` only.
4. **TOCTOU + lock contention**: file-lock the `.photohelper/catalog.db.lock` sibling at catalog open; set `busy_timeout = 5000`; reject second concurrent process with a typed error.
5. **Update §Deliverables 5 and 4** accordingly; preserve the **NOTE** strengths (bundled rusqlite, SQLite for v0.1) — those remain correct.

---

### Theme 4 — Path safety: canonicalization, NUL bytes, `--catalog` directory collisions [CRITICAL]

**Agents**: rev (CRITICAL), sfh (HIGH)

- `source_path` accepted as-is from `walkdir` → can contain `..`, NUL bytes (`\0`), or be a symlink target that escapes the ingestion root (rev CRITICAL).
- `--catalog <path>` semantics undefined for: missing parent dir; path exists as directory; path exists as non-DB file (sfh HIGH).

**Remediation direction**: Canonicalize every `source_path` with `std::fs::canonicalize` before catalog insert; reject NUL bytes; reject paths whose canonical form escapes the ingestion root (logged WARN + skip). For `--catalog`: create missing parent dirs (logged INFO); reject existing-as-directory with `Error::CatalogPathIsDirectory { path }`; magic-byte check existing files to reject non-SQLite blobs. Add error-path integration tests for each.

---

### Theme 5 — Observability: silent skips, missing summaries, fail-open stubs, tracing levels, anyhow context [CRITICAL]

**Agents**: sfh (CRITICAL ×2), sfh (MEDIUM ×2), sfh (HIGH ×2)

A photographer running `photohelper ingest /some/path/` MUST be able to tell what happened. The plan as written allows:
- `ingest /jpegs/` → exit 0, empty catalog, no signal (sfh CRITICAL).
- 5,000 photos from an unsupported body → 5,000 `CameraId::Unknown` rows, exit 0, no signal (sfh CRITICAL).
- Stub subcommands (`cull`, `develop`, …) exit 0 → scripted pipelines see them as success (sfh MEDIUM, gp MEDIUM separately).
- EXIF parse failures silent at default verbosity because `tracing` levels aren't pinned (sfh HIGH).
- Errors `?`-bubble to `main` without `.with_context(...)` — user sees "io error: No such file or directory" with no photo path (sfh HIGH).
- `mtime` ∈ {0, future, pre-1970} silently stored → broken ORDER BY queries downstream (sfh HIGH).

**Remediation direction**: Add a §Observability subsection to the plan committing to:
- End-of-run summary line: `walked: N, ingested: M, skipped (non-RAW): K, unknown-camera: U, errored: E`.
- Non-zero exit (e.g. `64 EX_USAGE`) when `walked > 0 && ingested == 0`.
- Stub subcommands exit `64 EX_USAGE`, not 0.
- Tracing-level table: EXIF parse failure → `WARN`; unknown camera first-seen → `WARN`, subsequent same body → `INFO`; skipped non-RAW → `INFO`; ingest-success → `DEBUG`. Default `-v` count surfaces `WARN` to stderr.
- Mandate `.with_context(|| format!("ingesting {}", path.display()))` at the per-photo work loop and `.with_context(|| format!("opening catalog at {}", catalog_path.display()))` at the catalog-open boundary; both are explicit deliverable bullets.
- `--strict` flag escalates `unknown-camera > 0` and `errored > 0` to non-zero exit.
- `mtime` validation: clamp to `[1995-01-01, now() + 1 day]`; out-of-band → log WARN + store NULL (requires a small schema tweak the §4 spec must reflect).

---

### Theme 6 — Scope expansions vs the bootstrap plan are undeclared [CRITICAL]

**Agents**: gp (CRITICAL)

The bootstrap plan Phase B specifies session 01's subcommand surface as `ingest, cull, develop, export, run, models, camera`. The session-01 plan adds capabilities not in the bootstrap-plan Phase B: `--catalog` override flag, `tracing-subscriber` env-filter, `Orientation` enum, `schema_version` table + migration helper, UPSERT-based idempotency, single-writer concurrency pattern, `CancellationToken`. Several of these are sound (and Themes 2/3 will trim some of them anyway), but adding them silently inside session 01 erodes the "plan is a contract" discipline.

**Remediation direction**: Add a §"Scope expansions vs the bootstrap plan" subsection naming every addition with a one-line justification. After Themes 2 & 3 remediation, the actual expansions remaining are: `--catalog` override (justified — operational ergonomics), `Orientation` enum (now reshaped per Theme 5), magic-byte check on existing catalog file (Theme 4), `--strict` flag (Theme 5), `busy_timeout` (Theme 3). Each is named + justified or removed.

---

### Theme 7 — Filesystem realities: mtime resolution, content-change at same path, edge files [HIGH]

**Agents**: rev (HIGH FAT32 mtime), sfh (CRITICAL UPSERT masks content change), sfh (HIGH mtime validation — covered by Theme 5), test (HIGH PhotoId small files — covered by Theme 1), test (LOW walker edges)

After Theme 1 absorbs `mtime` into `PhotoId` (for entropy), the remaining issues are:
- **FAT32/exFAT 2s mtime granularity**: Canon SD cards are FAT32/exFAT — `mtime_unix_ns` precision is a documentation lie on those filesystems. Theme 1 derivation should hash `mtime_unix_seconds` (truncated to 2s), not `_ns`, to avoid hash drift when copies preserve subsecond precision differently across tools.
- **Same path, different content** (sfh CRITICAL): same `source_path` + different `PhotoId` (file replaced) — plan doesn't say what UPSERT does. After Theme 3 redesign with catalog keyed by `PhotoId` primary key + `source_path` as a secondary indexed column, the resolution is: insert the new row, mark the old one `superseded`. Needs an integration test.
- **Walker edges** (test LOW): hidden files, symlink loops, non-UTF-8 paths, empty input dir. Document the policy in the plan + add a single integration test covering all four in one tempdir.

**Remediation direction**: Hash `mtime_unix_seconds` (2s-floor) in `PhotoId` derivation (Theme 1 update); add an integration test that mutates content at the same path between two ingests and asserts both rows exist with one marked superseded; document walker edge-case policy in §Deliverables 1 + add one consolidated edge-case test.

---

### Theme 8 — Type design: `Photo` invariants, `CameraId` shape, `Orientation` lossiness, `Error` enum, `PhotoRow`, `CameraProfile` stubs [HIGH]

**Agents**: type (HIGH ×4, MEDIUM ×2), arch (MEDIUM CameraId overlap), sfh (MEDIUM `todo!()` panic surface), rev (HIGH `todo!()` production-path question)

Cluster of related but smaller type-design fixes:

- **Photo**: fields private; single fallible constructor `Photo::ingest(source_path: PathBuf, ...) -> Result<Self, Error>` that canonicalizes + validates; consider an `AbsPath` newtype if "canonical absolute" is load-bearing across stages.
- **CameraId**: refactor to `enum CameraId { Known(KnownCamera), Unknown { make: String, model: String } }` where `KnownCamera` is `#[non_exhaustive]` and holds the recognized-body variants (CanonR8 today, R5/R6 II in session 02). Eliminates dead `match` arms.
- **Orientation**: do NOT collapse EXIF orientation tag (8 values: rotations + mirrors) to Landscape/Portrait. Store full `#[non_exhaustive] enum ExifOrientation { Normal, MirrorH, Rotate180, Rotate90Cw, Rotate90Ccw, ... }` (8 variants). Derive Landscape/Portrait as a *method* on `Photo` from `(width, height, orientation)`. Information lost here is unrecoverable without re-reading the file (which session 05's export will want).
- **Error enum**: `#[non_exhaustive]`; per-operation variants with structured context (`Io { path: PathBuf, op: &'static str, source: io::Error }`); distinguish catalog-open (fatal) from catalog-insert (per-photo skip).
- **PhotoRow**: name an explicit struct in `photohelper-catalog`; single `from_row(&Row) -> Result<Self, Error>` + `to_params(&self) -> impl Params` boundary; column-name knowledge confined to the schema-definition module.
- **`CameraProfile` stub methods**: do NOT use `todo!()` / `unimplemented!()` (both panic — fail the `panic = "warn"` clippy lint under `-D warnings` AND violate `CLAUDE.md § Rust-specific gates` "no panics on production paths"). Use `Err(Error::CameraProfileNotImplemented { method: &'static str, camera_id })` — typed, recoverable, lint-clean, and session 02 can search-and-replace.

**Remediation direction**: rewrite §Deliverables 2 and 3 with the above type shapes; add `photohelper-catalog::PhotoRow` to §Deliverables 4 (the new catalog crate from Theme 3); replace every `todo!()`/`unimplemented!()` mention with the `Error::*NotImplemented` pattern. Adds ~30 lines to the plan, removes ~10 lines of confusion.

---

### Theme 9 — Test coverage gaps [HIGH]

**Agents**: test (HIGH ×3, MEDIUM ×3, LOW)

Some of these are absorbed by remediation of other themes (Theme 1 mandates the PhotoId edge tests; Theme 3 mandates concurrent-process and migration tests; Theme 5 mandates summary-line and exit-code tests; Theme 7 mandates content-change-at-same-path test). What remains:

- **CLI surface** (test HIGH): every stub subcommand needs exit-code + stderr-substring tests via `assert_cmd`; `-v` / `-vv` / `-vvv` → tracing level mapping; `--threads 0`, `--threads <huge>` boundaries; `--catalog` override actually used.
- **CameraRegistry normalization** (test MEDIUM): real EXIF strings carry trailing NULs, whitespace, case variations. Either implement normalization + test, OR document the assumption and add a `normalize_exif_string` helper with tests.

**Remediation direction**: expand §Deliverables 6 and §Deliverables 7 to enumerate every test added in this remediation; preserve the global testing-standard NOTE (every test asserts a concrete observable — no `expect(true)`).

---

### Theme 10 — Cross-cutting plan hygiene: counts, opinions, link rot, cadence cites, deps section [MEDIUM]

**Agents**: com (HIGH ×3, MEDIUM ×3, LOW ×2), gp (MEDIUM ×2, LOW), arch (MEDIUM dep cliff)

- **Count drift**: plan cover text says "8 explicit out-of-scope items" — actual count is 7 (com HIGH). Either add one or correct the count.
- **`~/.claude/CLAUDE.md` link rot** (com HIGH): the only carrier of the assertion-quality rule is a user-private file path. Inline the rule into `docs/quality-assurance.md` or add a repo-local `docs/testing-standards.md` and cite that.
- **DN-005 closure overstatement**: discovery-notes lists owners as session 01 AND session 02; the plan says "Closes DN-005." Replace with "Partially resolves DN-005 — lands v1 minimal schema slice; session 02 still owes dup-group + culling-score tables" (gp HIGH, com MEDIUM, arch LOW, rev LOW). Add `docs/decisions/0001-catalog-schema-v1.md` to §Deliverables as an explicit artifact.
- **Unmarked opinion**: `kamadak-exif` "the obvious pick" — back with a citation (download count, last-commit-date) or drop the qualifier (com MEDIUM).
- **Cadence A session-start agent unnamed** (com MEDIUM): plan says "1 (alignment) — implicit" but the QA doc Tier 1 specifies `general-purpose, haiku`. Name it.
- **Dependencies introduced this session** (gp MEDIUM, arch MEDIUM): add a "Dependencies introduced this session" subsection — crate + version range + features for each. Bootstrap-plan §A.4 set this convention; session-01 plan doesn't honor it.
- **`indicatif` ETA with `par_bridge`** (gp MEDIUM): `par_bridge` consumes lazily; total count is unknown without a pre-pass; ETA is structurally impossible. Either commit to a discovery pre-pass (and document the I/O cost) OR downgrade to a spinner with throughput, not an ETA.
- **`clap` stub subcommand exit-0 mechanics** (gp MEDIUM): `clap` derive stubs don't print-and-exit-0 by default; the plan needs an explicit handler arm per subcommand. (Theme 5 now changes these to exit 64.)
- **`indexing_slicing = warn`** (rev MEDIUM): noted; iteration via `for entry in walkdir` / `.filter_map()`, never index. Low risk — clippy will catch it. Mention in §Deliverables 5.
- **Heading hierarchy** (com LOW): `## (Below this line — implementation notes…)` is a divider, not a section. Convert to `---` + bold.
- **Tense/voice** (com LOW): standardize §Deliverables on future-perfect ("when the PR merges, X will exist").
- **Cadence tier numbers**: cite Tier 1 / Tier 4 / Tier 5 from the QA doc so the mapping is auditable (gp LOW).

**Remediation direction**: batched edit to the plan addressing each bullet; create `docs/testing-standards.md` (small) to break the `~/.claude/` link.

---

### Theme 11 — `Pipeline` trait Sync/Send story (now moot after Theme 2 remediation) [HIGH → resolved]

**Agents**: type (HIGH PipelineCtx Send/Sync), simp (CRITICAL dropping the trait)

Theme 2 remediation deletes the trait entirely, so the Send/Sync story disappears with it. Round 2 should confirm the deletion did not leave a Send/Sync question elsewhere (the `Mutex<Connection>` from Theme 3 still has to be `Send + Sync`; that's the new place to look).

---

### Theme 12 — `indicatif` ETA + `clap` exit codes [MEDIUM → folded into Theme 10]

Already covered above; flagged as Theme 12 only to acknowledge that gp's MEDIUMs on these two were correct call-outs but they're hygiene-class, not architecture.

---

## Strengths to preserve through remediation [NOTES]

These should not regress in Round 2:

- **Out of scope + Non-goals two-axis framing** (gp NOTE): exactly what `docs/quality-assurance.md § Plan-review protocol` asks for. Preserve.
- **DN/TD posture honest** (gp NOTE): plan proactively flags potential new DN-006 / DN-007 and explicitly notes no new TDs anticipated. Preserve.
- **Bundled rusqlite + 7-crate split + `indicatif` adoption** (simp NOTE, arch NOTE): all defensible per the bootstrap plan + product positioning. Preserve. (Theme 3 adds an 8th crate but preserves the split philosophy.)
- **RAW-extension allowlist** (rev NOTE): correct guard against accidentally cataloguing companion JPEGs from the same SD card directory. Preserve.
- **Deferring LibRaw to session 02** (arch NOTE): right scope discipline. Preserve.
- **Global testing-standards compliance addressed** (test NOTE, simp NOTE): every described test asserts a concrete observable. Preserve through Theme 9's expansion.
- **Deferred-pseudocode contract holds** (com NOTE): the §Below-the-line marker correctly defers implementation notes. Preserve, but rename heading per Theme 10.
- **`CancellationToken` is the right scaffold for cooperative shutdown** (sfh NOTE) — caveated by Theme 2's deletion. The *concept* is right; the *placement in session 01* was wrong. When SIGINT lands in a later session, sfh's reasoning will be the spec.
- **Single-writer SQLite encodes a hard invariant** (type NOTE, arch NOTE) — caveated by Theme 3's `Mutex<Connection>` decision. The reasoning is correct in principle; v0.1 just doesn't need the plumbing.

---

## Round 2 watch-list

Specific things to re-check in Round 2 after remediation (regression-prone areas):

1. **PhotoId encoding length** still consistent across §Deliverables 2 + Test plan + the to-be-written `0001-catalog-schema-v1.md` decision doc.
2. **`Mutex<Connection>` Send/Sync** is correct (Theme 11) — the connection wrapped in a Mutex must be `Send + Sync` for sharing across rayon workers.
3. **Stub subcommand exit codes** consistent at `64 EX_USAGE` everywhere (Theme 5 + Theme 10).
4. **Theme 6 expansions list** is exhaustive — every addition the remediation kept must be named.
5. **Cargo.toml [workspace.dependencies] vs per-crate Cargo.toml**: the deps subsection (Theme 10) needs to specify where each dep is centralized.
6. **Plan total length**: after adding the §Observability subsection + §Scope-expansions + §Dependencies + the type-design rewrites, the plan should not exceed ~400 lines without justification. Simplifier discipline.
7. **§Test plan table row count** matches the post-remediation actual count (currently advertised as 6 — likely 10+ after Theme 9).
8. **Theme 7 walker-edges test** doesn't fragment into one test per edge — keep it as one consolidated test with multiple assertions.
