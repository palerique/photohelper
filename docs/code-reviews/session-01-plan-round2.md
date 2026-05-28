# session-01 plan-review Round 2

> Per `docs/quality-assurance.md § Plan-review protocol`. Cadence A → Tier 5
> (plan review), full 8-agent suite re-fired in parallel against the v2 plan
> (`docs/plans/session-01.md` revision `364011d`).
>
> **R2 focus**: regressions introduced by Round 1 remediation. Most R1
> CRITICAL/HIGH findings were closed cleanly; the items below are new issues
> the remediation surfaced, plus 2 explicit REGRESSIONs (R1 items not fully
> addressed).
>
> Findings grouped by theme. Agents cited in brackets.

## Summary

| Severity | Themes | Notes |
|----------|-------:|-------|
| **CRITICAL** | 4 | Block Round-2 acceptance; remediate before any code lands. |
| **REGRESSION** | 2 | R1 findings not fully addressed; same severity discipline as CRITICAL. |
| **HIGH** | 6 | Must address before session-end review. |
| **MEDIUM** | 4 | Address during remediation; LOWs may be deferred only with TD + trigger. |
| **LOW** | 2 | Polish — fold into remediation. |
| **CLEAN** themes (R1 fixes preserved) | 12 | Round 2 confirms remediation held. Preserve through Round 3 if it fires. |

Decision on Round 3: 4 CRITICAL + 2 REGRESSION clear the Round-3 trigger per
`docs/quality-assurance.md § Double-review protocol` ("If Round 2 surfaces
CRITICAL-class regressions needing another cycle, add Round 3"). Round 3 fires
after this remediation.

---

## Findings (by theme)

### R2.T1 — Dependency-health regressions: `fs2` unmaintained; `rusqlite 0.32` two majors stale; `kamadak-exif` CR3 capability unverified [CRITICAL]

**Agents**: gp (HIGH ×2), arch (HIGH), sfh (HIGH overlap), com (verified versions exist)

The plan v2 dependency table was added in remediation but the chosen versions weren't sanity-checked:
- **`fs2 0.4`** — last published 2018-01-06 (8 years stale). Author's own successor is `fs4 1.x` (current). `cargo audit --deny warnings` (a gate in `CLAUDE.md § Quality gates`) typically flags `fs2` via `RUSTSEC-*-unmaintained-*` advisories. This would fail `just ci` on day one. Direct CI breakage = CRITICAL.
- **`rusqlite 0.32`** — current is `0.40`. The bundled SQLite amalgamation in 0.32 is ~14 months old and will trip SQLite CVE advisories. Defeats the "bundling closes version-skew risk" rationale.
- **`kamadak-exif 0.6`** — primary use case is JPEG/TIFF/HEIF EXIF; CR3 stores EXIF inside an ISOBMFF `uuid` box. The plan defers the choice to DN-006 but ships `kamadak-exif` as default without confirming CR3 support. If kamadak silently returns empty `Fields`, every CR3 ingests "successfully" with all-NULL metadata (covered also under R2.T8 silent-failure).

**Remediation**:
1. Swap `fs2 0.4` → `fs4 1` (drop-in `FileExt::lock_exclusive` / `try_lock_exclusive` API).
2. Bump `rusqlite` to the latest 0.3x or 0.40 line; verify the `bundled` feature still vendors the latest SQLite.
3. Pre-flight kamadak-exif on a synthesized CR3 fixture before session-01 implementation finalizes. If it can't parse CR3, document the fallback plan in DN-006 (likely: defer CR3-EXIF source to session 02 with LibRaw, use synthesized EXIF fixtures for session-01 tests).

---

### R2.T2 — `PhotoId::from_db_bytes` is a public forgery bypass + mtime-in-hash trade-off unacknowledged [CRITICAL]

**Agents**: type (CRITICAL forgery), arch (CRITICAL mtime trade-off)

Two intertwined PhotoId issues:
- **Forgery**: `PhotoId::from_db_bytes([u8; 32]) -> Self` is `pub` with "no validation — bytes already trusted from our DB." Any caller in `cli`, `cameras`, or a future crate can mint a `PhotoId` from arbitrary bytes, defeating the content-address invariant. The only legitimate callers are catalog row-reconstruction + tests.
- **mtime trade-off**: hashing `mtime_unix_seconds` alongside content means two byte-identical copies via different tools (one preserving mtime, one stamping `now`) produce different PhotoIds. For a photo-management tool where users re-import from SD card and `rsync`/`cp`/`rclone` archives between machines, this breaks the intuitive "same file → same PhotoId" contract. The plan didn't acknowledge the trade-off.

**Remediation**:
1. Make `PhotoId::from_db_bytes` `pub(crate)` to `photohelper-core`; expose to `photohelper-catalog` via a sealed trait `FromCatalogBytes` (or simpler: place `photohelper-catalog`'s row-reconstruction inside `photohelper-core::model` behind a `pub(crate)` constructor and have catalog call through a small `pub fn` that takes a `PhotoRow`).
2. Add an explicit "PhotoId stability across copies" note in the §PhotoId derivation section: hashing mtime is **intentional** — we want photographer-relevant identity (same shot at same moment), not pure content-bytes identity. Document the supersede semantics handle the "user re-saved the file" case. Same-bytes-different-mtime = different PhotoId by design.

---

### R2.T3 — `fs2::lock_exclusive` blocks forever + plan internally contradicts blocking vs try-lock [CRITICAL + REGRESSION]

**Agents**: sfh (CRITICAL), arch (MEDIUM + REGRESSION), rev (MEDIUM)

Multi-axis issue around the catalog file-lock:
- **Stale-lock hang on networked filesystems**: `fs2::lock_exclusive` is *blocking*; on NFS/SMB/some FUSE filesystems, advisory locks aren't always released cleanly when the prior process crashes. `photohelper ingest /mnt/photos` then hangs silently with the spinner ticking forever.
- **Plan self-contradicts**: §Deliverables 4 says "acquires an exclusive `lock_exclusive`" (blocking) but immediately says "second concurrent process exits with `Error::CatalogLockHeld`" (which is try-lock semantics).
- **File-lock sequence**: plan doesn't say the lock is acquired *before* opening the `.db` file. The magic-byte check on existing `.db` content (a separate `File::open`) creates a small TOCTOU window if it runs first.

**Remediation**:
1. Switch to `fs4::try_lock_exclusive()` (or whatever the equivalent is on the new `fs4` crate per R2.T1) with an explicit retry loop: 5 attempts at 500ms each, WARN per retry, then `Error::CatalogLockHeld`. Bounded wait, user-visible.
2. Acquire the lock *first* — before any `Connection::open` or magic-byte check — and hold it for the whole ingest run.
3. Update §Deliverables 4 to specify the lock-first sequence explicitly.

---

### R2.T4 — `parking_lot::Mutex<Connection>` doesn't poison + no explicit `BEGIN IMMEDIATE` per insert [CRITICAL]

**Agents**: sfh (CRITICAL + HIGH BEGIN IMMEDIATE), arch (CLEAN with caveat)

`parking_lot::Mutex` deliberately does NOT poison on panic (unlike `std::sync::Mutex`). A worker that panics mid-insert (OOM during parameter bind, a `panic = "warn"` clippy slip, a malformed EXIF parse) releases the mutex cleanly, leaving the connection possibly mid-`BEGIN`. The next worker picks up a connection that silently lands inserts in the wrong transaction or hits `SQLITE_MISUSE`. The summary line lies (`ingested: N`, actually M < N committed).

Additionally: the plan doesn't specify per-insert transactions. Default rusqlite auto-commits, but a power-loss / SIGKILL mid-write corrupts WAL state. SQLite recovers correctly on next open but doesn't tell the user a frame was discarded.

**Remediation**: pick one of two paths, justify in the plan:
- **Path A (defensive)**: switch to `std::sync::Mutex<Connection>` — poisons on panic, every subsequent caller sees `PoisonError` and we surface `Error::CatalogPoisoned`. Slight performance cost vs parking_lot, but mechanically correct.
- **Path B (transactional)**: keep `parking_lot::Mutex`, but wrap every insert in `Connection::execute("BEGIN IMMEDIATE")` + commit-or-rollback, and use `std::panic::catch_unwind` around insert calls to force ROLLBACK on panic.

In both cases: at `Catalog::open`, run `PRAGMA wal_checkpoint(TRUNCATE)` and log `WARN` if it reports recovered frames (signals an unclean prior shutdown).

---

### R2.T5 — Path-traversal escape check dropped during remediation [REGRESSION]

**Agents**: rev (REGRESSION)

Round 1 Theme 4 explicitly required rejecting paths whose canonical form
escapes the ingestion root (a malicious symlink at `/tmp/photos/evil ->
/etc/passwd` would otherwise canonicalize to outside the input dir and be
cataloged). Plan v2 §Path safety addresses NUL bytes + canonicalization
but dropped the escape check.

**Remediation**: add a constraint after canonicalize: `if !canonical.starts_with(&ingestion_root_canonical) { return Err(Error::PathEscapesRoot { path, root }) }`. Add a test row to §Test plan: tempdir with a symlink pointing outside → ingest skips (or fails per `--strict`) with that error variant.

---

### R2.T6 — `Catalog` Send+Sync claim unverified; no compile-time assertion [CRITICAL → addressable as MEDIUM with test]

**Agents**: rev (CRITICAL), type (HIGH), gp (MEDIUM — claim verified mechanically)

Plan claims `Catalog` is `Send + Sync` because it wraps `parking_lot::Mutex<rusqlite::Connection>`. The claim is mechanically correct *if* `Catalog`'s only field is the mutex. But the plan defers the full struct definition to "implementation notes (after Round 2)," so any added field (lock-file handle `File`, `PathBuf`, etc.) must also be `Send + Sync` and that won't be reviewer-verifiable until code lands.

**Remediation**: (a) explicitly list `Catalog`'s fields in §Deliverables 4 — at minimum the `Mutex<Connection>`, the lock-file `File`, the canonical path; (b) add a unit test that names a compile-time assertion: `const _: fn() = || { fn assert_send_sync<T: Send + Sync>() {} assert_send_sync::<Arc<Catalog>>(); };`. Zero runtime cost, fails the build if a future change adds an `Rc`/`Cell`.

---

### R2.T7 — `ingest_one` placed in `photohelper-core` re-couples core → catalog [HIGH]

**Agents**: arch (HIGH)

Plan v2 §Deliverables 5 defines `ingest_one(path: &Path, catalog: &Catalog) -> Result<IngestOutcome, Error>` in `photohelper-core::ingest`. But `Catalog` lives in the new `photohelper-catalog` crate (created by Round 1 Theme 3 remediation precisely to keep `core` storage-agnostic). The function signature in `core` therefore introduces a `core → catalog` dependency that defeats the separation.

**Remediation**: move `ingest_one` to `photohelper-cli::commands::ingest` (composition happens at the binary boundary — `cli` already depends on both `core` and `catalog`). `core` exports just the domain types. The `IngestOutcome` enum can stay in `core::model` since it's a pure domain type. Update §Deliverables 5 + the deps DAG accordingly.

---

### R2.T8 — Observability gaps: silent quiet, no mtime-anomalous summary, no worker heartbeat, no no-EXIF outcome [HIGH]

**Agents**: sfh (HIGH ×3, MEDIUM), gp (MEDIUM exit-code collision)

A cluster of small observability holes that together let a wrong-feeling run pass for a successful one:
- **`-q` mutes both WARN and the summary line** (summary is `tracing::info!`). User running `photohelper -q ingest ... | tee` sees zero diagnostic output AND zero summary.
- **`mtime_anomalous` flag stored per row, absent from summary line**: 5000 anomalous-mtime photos produce 5000 WARN lines (scroll off) + summary that says nothing.
- **No stuck-worker heartbeat**: when one worker holds the Mutex on a slow EXIF parse, all others `park`. Spinner ticks from the main thread. User can't tell "still working" from "stuck."
- **kamadak silent empty-fields**: linked to R2.T1 — even if the lib doesn't fail, an "EXIF parse succeeded with zero fields" outcome is invisible. Plan only logs "EXIF parse failure," not "EXIF parse empty."
- **Exit-code 2 collision**: `clap` parse-failures exit 2 by default. Plan v2 reuses exit 2 for "fatal catalog/IO errors." A test asserting "exit 2 = fatal catalog error" collides with "exit 2 = clap rejected flag."

**Remediation**:
1. `-q` contract: the summary line **always** prints to stderr regardless of tracing level (use a direct `eprintln!` for the summary, or downgrade the summary to a `println!` to stdout that survives the tracing filter).
2. Add `mtime-anomalous: <X>` slot to the summary-line format.
3. Add a periodic heartbeat: spawn a thread that fires `tracing::info!("walked {N}, ingested {M}, in-flight {P}")` every 10s.
4. Add `IngestOutcome::NoExifFields` variant + `no-exif: <X>` summary slot.
5. Pick distinct fatal exit code: change "fatal" from `2` to `74 (EX_IOERR)` or `78 (EX_CONFIG)` per sysexits.h. Document the choice in §Observability.

---

### R2.T9 — Type-design refinements: encoding ambiguity, ExifOrientation variant order, AbsPath ergonomics, KnownCamera slug, Error #[from] ambiguity, IngestOutcome non_exhaustive [HIGH/MEDIUM]

**Agents**: type (HIGH ×3, MEDIUM ×4)

- **`camera_id TEXT` ambiguous encoding** (HIGH): `'unknown:<make>:<model>'` is lossy when make/model contains `:` (real EXIF strings: `"IQ4 150MP / XF"`, etc.). Three options: JSON-encode unknown payload; drop the synthetic key and just store `make`/`model` columns + a `camera_known BOOLEAN`; or length-prefix.
- **`ExifOrientation` variant order is WRONG** (HIGH): plan v2 lists `Normal, MirrorH, Rotate180, MirrorV, MirrorHRotate270, Rotate90Cw, MirrorHRotate90, Rotate90Ccw`. EXIF canonical mapping is `1=Normal, 2=MirrorH, 3=Rotate180, 4=MirrorV, 5=MirrorH+Rotate90CW (transpose), 6=Rotate90CW, 7=MirrorH+Rotate90CCW (transverse), 8=Rotate90CCW`. The plan's variant at slot 5 (`MirrorHRotate270`) is the wrong rotation direction. Direct correctness bug.
- **`ExifOrientation` int round-trip unspecified** (HIGH): need `from_int(i64) -> Result<Self, Error>` + `to_int(&self) -> i64`; unit test asserting `from_int(N).unwrap().to_int() == N` for N ∈ 1..=8.
- **`AbsPath` ergonomics** (MEDIUM): plan doesn't say `impl AsRef<Path>`. Every consumer doing `.0` is a paper cut. Spec: private field; `impl AsRef<Path>`; `pub fn as_path(&self) -> &Path`.
- **`KnownCamera::slug()`** (MEDIUM): catalog stores `'canon-r8'` text but derivation isn't specified. `Debug` is `"CanonR8"`, `Display` is unspecified. Spec: `pub fn slug(&self) -> &'static str` with explicit `CanonR8 => "canon-r8"`; `pub fn from_slug(&str) -> Option<Self>`.
- **`Error` enum `#[from]` ambiguity** (MEDIUM): 3 variants take `io::Error` (`Io`, `Canonicalize`, implicitly others); 2 take `rusqlite::Error`. `#[from]` on any of them would route `?` to that variant unconditionally — a footgun for contributors. Spec: no `#[from]` derives; every site uses explicit `.map_err(|e| Error::Io { path, op: "...", source: e })`.
- **`IngestOutcome` should be `#[non_exhaustive]`** (MEDIUM): the summary-counter matching has to be wildcard-safe so adding a 6th variant (NoExifFields per R2.T8; SkippedDuplicateHash later) doesn't silently break tallies.

**Remediation**: batched edits to §Deliverables 2 and 4. Choose option (b) for camera_id (drop synthetic key; use existing `make`/`model` columns + `camera_known BOOLEAN`). Fix the ExifOrientation variant names and order to match EXIF canonical. Add the `slug`/`from_slug`/`from_int`/`to_int` specs. Add the no-`#[from]` discipline note. Mark IngestOutcome `#[non_exhaustive]`.

---

### R2.T10 — Stub subcommand exit code `64 EX_USAGE` semantically wrong; should be `69 EX_UNAVAILABLE` [MEDIUM]

**Agents**: rev (MEDIUM)

`EX_USAGE = 64` is for "command line usage error" (invalid flags, missing args). Plan v2 reuses it for "feature not yet implemented." `EX_UNAVAILABLE = 69` ("service unavailable") is more semantically accurate; `EX_SOFTWARE = 70` ("internal software error") also defensible. The reason matters because R2.T8 already needs to deconflict exit-code 2 with `clap` usage errors — picking the wrong codes here makes the exit-code surface confusing.

**Remediation**: switch stub subcommand exit to `69 EX_UNAVAILABLE`. Update §Deliverables 1, §Observability exit-code table, and the test rows.

---

### R2.T11 — Test gaps: mtime clamp, per-event tracing level, file-lock cross-process, .with_context boundary, exit-code discrimination, IngestOutcome end-to-end, hardlink dedup, walked=0, exact count [HIGH]

**Agents**: test (HIGH ×3, MEDIUM ×4, LOW ×3)

After v2 added the §Observability contract + supersede semantics + Catalog file lock, the test plan didn't expand to cover the new commitments:
- **`mtime_anomalous` clamp** untested — function unit test (under-1995 → clamped to 1995 + anomalous=1; future > now+1d → clamped + anomalous=1; in-range → unchanged + anomalous=0) + integration assertion via synthesized mtime=0.
- **Per-event tracing-level mapping** untested — only `-v` count is. A regression where EXIF-failure drops from WARN to DEBUG silently suppresses. Need 3+ rows: EXIF failure → stderr contains WARN at default `-v=0`; unknown-camera first-seen vs subsequent → one WARN + one INFO; mtime clamp → WARN.
- **File-lock test must be cross-process**: `fcntl` advisory locks are per-process on Linux/macOS; two threads in the same process can both acquire. Spec: spawn a second process via `std::process::Command` (or `assert_cmd`) to verify cross-process exclusion.
- **`.with_context()` boundary** untested — force an unreadable RAW, assert stderr contains `"ingesting <path>"`.
- **Exit-code 2 vs 64 (now 64 vs 74/78 per R2.T8) discrimination** — explicit `.code(N)` assertions per fatal scenario.
- **`SkippedHashWindowTooSmall` summary tally** untested end-to-end — drop a 0-byte `.cr3`, assert `skipped (too-small): 1`.
- **Hardlink / cross-path INSERT OR IGNORE** untested end-to-end — hardlink the same CR3 to two paths, assert one row + INFO log.
- **`walked=0`** untested — truly empty input dir → exit 0, summary `walked: 0`.
- **Exact test count** — "approximately 28" should be exact (actual ~30 currently; will be ~38 after this expansion).
- **proptest for PhotoId collision** — defer as DN-009.

**Remediation**: add 8 test rows to §Test plan, update the count to exact.

---

### R2.T12 — Plan hygiene: cross-ref heading text, decision-doc deliverable, schema init transactional, tense [MEDIUM/LOW]

**Agents**: com (MEDIUM ×2, LOW ×3), arch (MEDIUM), rev (LOW)

- **Cross-ref heading mismatch** (MEDIUM): body cites `§Observability` and `§PhotoId derivation`, but real headings are `### Observability contract (per Round 1 Theme 5)` and `### PhotoId derivation (locked spec per Round 1 Theme 1)`. Drop the parentheticals from headings.
- **Decision doc not a top-level deliverable bullet** (MEDIUM): `docs/decisions/0001-catalog-schema-v1.md` referenced inline in deliverable 4 + §Expected discovery items, but not listed as a deliverable artifact. Easy miss at session-end.
- **Schema init transactional** (MEDIUM): plan doesn't say `BEGIN IMMEDIATE; CREATE TABLE ...; PRAGMA user_version = 1; COMMIT`. Without that, power loss between table-create and `user_version` write leaves the gate ambiguous. (Currently survives by accident since `CREATE TABLE IF NOT EXISTS` is idempotent, but document it.)
- **Tense drift** (LOW): §Deliverables mixes present-tense and infinitive bullets. Standardize on "will" + present.
- **Conventional-commit examples** (LOW): deferred to implementation; OK as-is.

**Remediation**: batched hygiene edits; explicit "decision artifact" bullet in §Deliverables; transactional init spec.

---

### R2.T13 — Simplicity creep callouts (smaller speculative shapes returning) [MEDIUM]

**Agents**: simp (MEDIUM ×4)

Simplifier's call-outs — to weigh, not to mechanically apply:
- **`AbsPath` newtype**: one-consumer abstraction. Could inline canonicalization in `Photo::from_filesystem`. *Weighted decision: keep AbsPath — the canonicalize boundary is real load-bearing for both `Photo::from_filesystem` AND `Catalog::open` (two consumers, not one — simplifier missed the catalog consumer). Plus the `ingestion_root` check from R2.T5 will become a third consumer.*
- **`IngestOutcome` enum**: simplifier suggests collapsing to `IngestStats { atomics... }` updated via shared ref. *Weighted decision: keep enum + add `#[non_exhaustive]` per R2.T9. The variants carry semantics that an integer counter would lose (`SupersededPrevious` triggers an INFO log; `SkippedHashWindowTooSmall` triggers a WARN). The driver still updates an `IngestStats` from the enum match — both layers are load-bearing.*
- **`Error` variant overlap** (`Canonicalize` is subset of `Io`): *agreed*. Collapse `Canonicalize` and `NulByteInPath` into `Io { op: "canonicalize" }` and `Io { op: "canonicalize-nul-check" }`. Drops 2 variants. Aligns with the no-`#[from]` discipline from R2.T9.
- **`--strict` flag pre-emptive**: *partial agreement*. Keep `--strict` because v0.1's "AI-first batch processing" use case is precisely the audience that needs scripted error semantics. But the exit-code-1 tier (only used by `--strict`) is acceptable.

**Remediation**: collapse `Canonicalize`/`NulByteInPath` into `Io { op }`. Keep AbsPath, IngestOutcome, --strict. Document why each was kept.

---

## CLEAN themes (R1 fixes preserved — confirm in Round 3)

Round 2 confirmed these remediations held cleanly across all relevant lenses:

1. **R1 Theme 1 PhotoId derivation spec** — endianness pinned, 43-char base64url-nopad render, edge-case behavior fully specified. (Note: the *forgery bypass* in R2.T2 is a new issue, not a regression of the derivation spec.)
2. **R1 Theme 2 speculative abstractions deletion** — Pipeline, PipelineCtx, Sidecar, CancellationToken all gone.
3. **R1 Theme 3 catalog placement** — 8th crate created; Mutex<Connection> chosen; migration framework dropped. (Note: R2.T7 catches that `ingest_one`'s placement re-couples; small adjustment needed.)
4. **R1 Theme 4 path safety** — canonicalize + NUL rejection in place. (Note: R2.T5 catches the missing escape check.)
5. **R1 Theme 5 observability core** — summary line, exit-code table, --strict flag, tracing-level table, .with_context mandates. (R2.T8 adds the gaps the v2 contract left.)
6. **R1 Theme 6 scope expansions** — explicit table with 7 additions and 4 deletions named.
7. **R1 Theme 7 mtime + supersede** — 2s-floor, anomalous flag, supersede-with-both-rows-retained, content-change integration test.
8. **R1 Theme 8 type design baseline** — Photo private fields + AbsPath; CameraId { Known, Unknown }; KnownCamera #[non_exhaustive]; ExifOrientation full 8-variant; structured Error variants; PhotoRow boundary; CameraProfile typed-Err stubs.
9. **R1 Theme 9 test coverage core** — 28+ rows enumerated; assertion-quality per testing-standards.
10. **R1 Theme 10 hygiene core** — out-of-scope count fixed (8); testing-standards.md created; DN-005 "partially resolves"; session-start agent named; tier numbers cited; `---` divider; dependencies subsection. Crate versions verified on crates.io (com confirmed).
11. **R1 Theme 11 Send/Sync after Pipeline deletion** — mechanically correct via Mutex<Connection>. (R2.T6 catches that it's not *asserted*.)
12. **R1 Theme 12 indicatif/clap mechanics** — spinner-not-progress-bar + explicit clap subcommand handlers.

---

## Round 3 watch-list

After R2 remediation lands, Round 3 should specifically re-check:

1. **Dep versions actually compile + `cargo audit` clean** (R2.T1) — `fs4` API surface matches our use; `rusqlite 0.40` doesn't break the `bundled` feature; kamadak-exif pre-flight verdict is documented.
2. **`PhotoId::from_db_bytes` visibility actually `pub(crate)`** (R2.T2) and no caller outside `photohelper-core` / `photohelper-catalog` constructs PhotoIds.
3. **File-lock try-lock with retry budget** (R2.T3) — explicit timeout numbers; sequence (lock before any DB op).
4. **`std::sync::Mutex` or `BEGIN IMMEDIATE`** (R2.T4) — path picked and consistently applied; `PRAGMA wal_checkpoint(TRUNCATE)` recovery log.
5. **Path-escape check restored** (R2.T5) — `canonical.starts_with(&ingestion_root_canonical)` + new test row.
6. **Compile-time Send+Sync assertion on Catalog** (R2.T6) — actually present in the test plan.
7. **`ingest_one` moved to `photohelper-cli::commands::ingest`** (R2.T7) — DAG shows `cli → core, catalog` only, not `core → catalog`.
8. **Observability gaps closed** (R2.T8) — `-q` summary contract; `mtime-anomalous` summary slot; heartbeat thread; `NoExifFields` outcome; fatal exit code distinct from clap's `2`.
9. **ExifOrientation variant order matches EXIF canonical** (R2.T9) — `MirrorHRotate90Cw` at slot 5 (NOT `MirrorHRotate270`); round-trip test covers 1..=8.
10. **`camera_id` encoding fixed** (R2.T9) — drop synthetic-text key; use `make`/`model` + `camera_known BOOLEAN`.
11. **Error enum collapsed `Canonicalize`/`NulByteInPath` into `Io { op }`** (R2.T13) — total variant count drops from 12 to 10.
12. **IngestOutcome `#[non_exhaustive]`** (R2.T9).
13. **Test plan grows to exact count** (R2.T11) — 8 new rows from R2.T11; ~38 total; count is exact, not approximate.
14. **Cross-ref heading text matches actual headings** (R2.T12).
15. **Decision-doc artifact explicitly in §Deliverables** (R2.T12).
16. **Plan total length** — v2 was 462 lines; R3 should aim to *not* grow past ~480 despite adding fixes (use the simplification opportunities in R2.T13 to offset additions).
