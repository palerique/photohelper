# Session 02 — `libraw-cr3-decode`

> **Branch**: `session-02/libraw-cr3-decode`
> **Started**: 2026-05-28
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v1 (initial; pre plan-review) → **v2 (this revision;
> post plan-review Round 1 — see `docs/code-reviews/session-02-plan-round1.md`)**

> **Note on title slug**: the branch is named `libraw-cr3-decode` but this
> session lands LibRaw EXIF read **AND** RAW pixel decode in one PR — see
> §Scope rationale below for the bundling rationale.

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the LibRaw FFI integration that turns `photohelper-raw` from a one-line
stub into a working RAW pipeline for Canon R8 CR3. Two complementary
deliverables under the same FFI surface:

1. **`photohelper-raw::exif::read_cr3(path) -> Result<RawExif>`** — extract
   `Make`, `Model`, `Orientation`, `CaptureTime`, `Width`, `Height` from a
   Canon R8 CR3 ISO-BMFF container. This is the **DN-011 critical-path
   remediation**: kamadak-exif fails on **370/370** real Canon R8 CR3s that
   reached the parser in DN-011's production trace (1 of the 371 walked
   files was skipped pre-parse as non-RAW), so LibRaw EXIF is the only
   path to a usable `--strict` mode and a non-degraded catalog row for
   CR3 ingest.

2. **`photohelper-raw::decode::read_raw(path) -> Result<RawImage>`** — decode
   the Bayer-pattern sensor data into a `RawImage` ready to feed
   session 04's develop pipeline. RAW pixel decode is the
   originally-planned-for-session-02 deliverable; the EXIF surface
   above was elevated from "nice to have" to "critical path" by DN-011
   between plan and start.

`ingest_one` rewires to call LibRaw for ALL `*.cr3` files. **Per
plan-review PR1-T1 remediation**: v0.1 narrows `RAW_EXTS` to
`["cr3"]` (was 8 extensions including CR2/NEF/ARW/RAF/ORF/RW2/DNG —
kamadak-exif failed on all of them per DN-006/DN-011 evidence and the
camera registry has no non-Canon profiles to route them to anyway).
The 7-format walker behavior moves to DN-014, which binds the
re-expansion to the session that adds the second `CameraProfile`.

Once wired, integration test row 32 flips its assertions from
`is_none()` to `Some("canon-r8")` and the strict-mode test on real CR3
fixtures exits 0 — closing the DN-006 / DN-011 binding triggers.

### Scope rationale (why bundle EXIF + decode + rusqlite bump in one session)

LibRaw is a single C library. Wiring its FFI surface for **only** EXIF
read and then re-wiring it for decode in a later session would mean
doing the FFI safety review, the LGPL static-link plumbing, and the
build-system configuration twice. The EXIF + decode pairing keeps the
FFI surface defined once and reviewed once.

**The rusqlite bump is bundled by calendar trigger, not by schema-touch.**
Per `TECH-DEBT.md:51`, TD-002's binding trigger has two clauses joined
by OR: (a) by **2026-08-01**, OR (b) before session 02 introduces new
schema columns. Clause (b) is NOT fired this session because **NO new
columns are added** — populate-existing-NULLs is DML, not DDL (the
column shape stays v1 per `docs/decisions/0001-catalog-schema-v1.md`
which the same plan-review caught and we amended to defer the v1→v2
migration framework to session 03). Clause (a) IS the operative
trigger; bundling the bump here is voluntary, ahead of the calendar,
because session 02 is already in catalog-crate code populating those
columns and pairing the dep bump with adjacent catalog work minimizes
churn. **Plan-review PR1-T24 corrected the original v1 framing which
incorrectly cited the structural trigger.**

### Deliverables (when the PR merges, the following will exist)

#### Deliverable 0 — Pre-flight feasibility probe (NEW per PR1-T9)

Before any FFI wiring, verify LibRaw can actually extract EXIF from the
user's R8 firmware revision.

- **Sequencing**: fires AFTER Deliverable 1's DI-1/DI-2 decisions
  (we need LibRaw bindings to call) AND BEFORE Deliverable 4's `ingest`
  rewire (we don't write production wiring on top of untested LibRaw
  behavior).
- **Artifact**: `docs/analysis/ANL-001-libraw-cr3-preflight.md` with:
  - LibRaw upstream version + commit SHA (per Deliverable 1 DI-1 lock).
  - For each of the user's 371 CR3 fixtures (`/Users/ph/Pictures/tests`):
    pass/fail + extracted `Make`/`Model`/`Orientation`/`CaptureTime`/
    `Width`/`Height`.
  - Aggregate stats: total passed, total failed, any LibRaw error codes
    encountered.
- **Commit shape**: a dedicated `chore(libraw): pre-flight EXIF
  extraction against user's 371-CR3 set` commit so the result is
  auditable in `git log`.
- **ABORT trigger**: if any field is missing on >5% of files, raise
  plan-review v3 with scope-escalation options (LibRaw alternative,
  custom CR3 parser, narrowed scope to specific firmware revisions).

#### Deliverable 1 — `photohelper-raw` real implementation

Single crate with three sibling modules: `ffi` (the **only** `unsafe`
site), `exif`, `decode`. Per plan-review PR1-T1/T2/T5/T20/T21:

##### 1a — FFI module (`crates/photohelper-raw/src/ffi.rs`)

- **Strategy locked at plan-review v2: hand-rolled minimal FFI shim**
  (DI-1 resolved). Binds only the ~6 LibRaw functions we actually
  call: `libraw_init`, `libraw_open_file`, `libraw_unpack`,
  `libraw_recycle`, `libraw_close`, `libraw_strerror`, plus
  `imgdata.idata.*` / `imgdata.sizes.*` / `imgdata.other.timestamp` /
  `imgdata.color.*` / `imgdata.rawdata.raw_image` field accesses via
  `#[repr(C)]` structs mirroring the LibRaw 0.21 ABI.
  - **Rationale (per PR1-T7b)**: smaller attack surface than adopting
    `libraw-rs` / `libraw-sys` (which wrap the entire LibRaw surface
    including parts we don't use); no third-party maintenance pace
    dependency; matches Acceptance criterion 3 ("`photohelper-raw::ffi`
    is the only crate with `unsafe` blocks") without depending on
    a transitive crate's `unsafe` discipline.
  - **Re-evaluation trigger**: if the hand-rolled shim's FFI function
    count exceeds 10 OR if a LibRaw 0.22+ binary-incompatible ABI
    change ships, escalate to plan-review v3 to reconsider DI-1.
- **`unsafe_code` discipline (per PR1-T21)**:
  - `crates/photohelper-raw/Cargo.toml` gains `[lints.rust] unsafe_code = { level = "allow", priority = 1 }` overriding the workspace `forbid`.
  - `src/ffi.rs` head: `#![deny(unsafe_op_in_unsafe_fn)]` — every `unsafe fn` body still requires inner `unsafe { ... }` with `// SAFETY:` comment.
  - `src/exif.rs`, `src/decode.rs`, `src/lib.rs` heads: `#![deny(unsafe_code)]` — confines `unsafe` to `ffi.rs` only.
  - Workspace `Cargo.toml` `[workspace.lints.clippy]` adds `undocumented_unsafe_blocks = "deny"` — every `// SAFETY:` omission is a compile error, not a convention.
- **Path encoding (per PR1-T20)**:
  - New `pub(crate) struct RawPath` newtype that wraps a `&Path` and validates: NUL-byte interior → `Err(Error::RawPath { reason: "interior-nul-byte" })`; non-UTF-8 path on Unix → typed error (not panic via `unwrap`); Windows long path → automatically `\\?\`-prefixed.
  - Per-OS conversion: Unix uses `OsStr::as_bytes() + CString::new()`; Windows uses `OsStr::encode_wide() + null-terminate + libraw_open_file_w` (separate FFI binding).
  - FFI calls accept `RawPath` only — never raw `&Path`.

##### 1b — `RawExif` type (`crates/photohelper-raw/src/exif.rs`)

Per PR1-T5 (type-design CRITICAL), `RawExif` is a strong type with
private fields, fallible constructor, and typed accessors — NOT a
bag-of-public-fields:

```rust
pub struct RawExif {
    make: String,
    model: String,
    orientation: ExifOrientation,            // strong enum, not u8/i64
    capture_time_unix_seconds: Option<i64>,  // matches catalog schema (per PR1-T35; UTC assumption documented inline)
    width: NonZeroU32,                       // non-zero invariant via type
    height: NonZeroU32,                      // non-zero invariant via type
}

impl RawExif {
    pub(crate) fn from_libraw_data(
        data: &libraw_data_t,
        path: &Path,
    ) -> Result<Self, Error> { ... }

    pub fn make(&self) -> &str { ... }
    pub fn model(&self) -> &str { ... }
    pub fn orientation(&self) -> ExifOrientation { ... }
    pub fn capture_time_unix_seconds(&self) -> Option<i64> { ... }
    pub fn width(&self) -> NonZeroU32 { ... }
    pub fn height(&self) -> NonZeroU32 { ... }
}

// Derives committed (PR1-L7): Clone for cheap tests; Debug;
// PartialEq/Eq for assertion convenience. NO Default (no meaningful
// zero value).
#[derive(Clone, Debug, PartialEq, Eq)]
// Send + Sync: pinned via module-scope static_assertions (NOT
// #[cfg(test)]-only per R2-M2 lesson):
// static_assertions::assert_impl_all!(RawExif: Send, Sync);
```

- `orientation` is `ExifOrientation` (the existing
  `crates/photohelper-core/src/model.rs:354` enum), not `u8`. If
  LibRaw returns an orientation outside 1..=8, `from_libraw_data`
  returns `Error::RawExifUnavailable { path, cause: ExifMalformed { field: "orientation", raw_value: format!("{n}") } }`.
- `capture_time_unix_seconds: Option<i64>` matches the catalog column
  shape at `catalog.rs:373`. UTC assumption is documented inline:
  "LibRaw's `imgdata.other.timestamp` is `time_t` interpreted as
  wall-clock UTC absent EXIF timezone metadata; DN-016 tracks the
  timezone-recovery work for v0.2." If session 02 implementation
  decides to use `time::OffsetDateTime` internally and convert at
  the conversion boundary, that's acceptable so long as the public
  accessor returns `Option<i64>` (catalog wire-shape stays consistent).
- `width`/`height` are `NonZeroU32`. LibRaw value sourced from
  `imgdata.sizes.iwidth` / `imgdata.sizes.iheight` (post-rotation
  visible-area pixels — per PR1-T34). The semantic match with EXIF's
  `PixelXDimension`/`PixelYDimension` is documented in
  `docs/decisions/0001-catalog-schema-v1.md` (amendment to land in
  session 02's catalog touch).
- `from_libraw_data` is `pub(crate)` — sole minting site (R3.T2
  precedent). Tests in the `exif` module exercise the conversion paths.

##### 1c — `RawImage` type (`crates/photohelper-raw/src/decode.rs`)

Per PR1-T5 (type-design CRITICAL):

```rust
pub struct RawImage {
    pixels: BayerPlane,                     // newtype carrying dims
    cfa_pattern: CfaPattern,                // 4-variant enum
    levels: SensorLevels,                   // newtype carrying black < white invariant
    // White-balance and color-matrix fields deferred per PR1-T19:
    // session 02 ships AS-SHOT WB only (no rebalance support); a
    // future session extends RawImage when develop pipeline lands
    // (DN-016 binding trigger when develop work begins).
    as_shot_white_balance: WhiteBalance,    // [f32; 4] RGGB; rejects all-zero (LibRaw "unloaded")
    color_matrix: ColorMatrix,              // 3x3 CamRGB→XYZ_D65; rejects identity-as-unloaded
}

pub struct BayerPlane {
    data: Box<[u16]>,
    width: NonZeroU32,
    height: NonZeroU32,
}
impl BayerPlane {
    pub(crate) fn new(data: Vec<u16>, width: NonZeroU32, height: NonZeroU32) -> Result<Self, Error> {
        if data.len() != width.get() as usize * height.get() as usize {
            return Err(Error::RawImageDimensionMismatch { ... });
        }
        Ok(Self { data: data.into_boxed_slice(), width, height })
    }
    pub fn row(&self, y: usize) -> &[u16] { ... }
    pub fn pixel(&self, x: usize, y: usize) -> u16 { ... }
    pub fn width(&self) -> NonZeroU32 { self.width }
    pub fn height(&self) -> NonZeroU32 { self.height }
    // No `pub data()` accessor — downstream code goes through
    // row()/pixel() so unchecked indexing is impossible.
}

pub enum CfaPattern {
    Rggb,
    Bggr,
    Grbg,
    Gbrg,
}

pub struct SensorLevels {
    black: u16,
    white: u16,
}
impl SensorLevels {
    pub(crate) fn new(black: u16, white: u16) -> Result<Self, Error> {
        if black >= white {
            return Err(Error::RawInvalidLevels { ... });
        }
        Ok(Self { black, white })
    }
}

#[derive(Debug)]
// NO Clone (Vec<u16> is too big; ~50 MB per CR3); use Arc<RawImage>
// for shared-borrow if multiple consumers need it.
// Send + Sync: pinned via module-scope static_assertions.
```

- **Memory pressure SLO (per PR1-T28)**: per-decode peak is bounded by
  `width * height * 2 bytes` for the `BayerPlane` plus LibRaw's
  internal `imgdata` working set (~96 MB for R8 24Mpix). With rayon's
  default 8 workers, transient peak is ~800 MB during decode;
  documented inline so session 04's develop pipeline can plan
  back-pressure. Test plan asserts the per-decode allocation bound.
- **Out-of-scope explicitly noted in §Out of scope**: tile-based decode
  (`read_raw_tile(path, region)`), streaming-to-disk decode
  (`read_raw_into(path, &mut buffer)`), and other-than-Bayer sensors
  (X-Trans, Foveon) — each owned by a later session with a binding
  trigger.

##### 1d — Error enum

Per PR1-T2 (5-way CRITICAL convergence), a single variant with typed
cause enum — NOT two variants per failure mode:

```rust
// in photohelper-raw::Error (NOT photohelper-core; keeps core
// storage-agnostic and free of LibRaw transitive dependency per R2-T26)
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("RAW EXIF unavailable at {path}: {cause}")]
    RawExifUnavailable {
        path: PathBuf,
        cause: RawExifCause,
    },
    #[error("RAW image decode failed at {path}: {cause}")]
    RawDecodeFailed {
        path: PathBuf,
        cause: RawDecodeCause,
    },
    #[error("RAW image dimension mismatch at {path}: declared {declared_pixels}, actual {actual_pixels}")]
    RawImageDimensionMismatch { path: PathBuf, declared_pixels: u64, actual_pixels: u64 },
    #[error("RAW invalid sensor levels at {path}: black={black} >= white={white}")]
    RawInvalidLevels { path: PathBuf, black: u16, white: u16 },
    #[error("RAW path validation failed at {path}: {reason}")]
    RawPath { path: PathBuf, reason: &'static str },
}

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum RawExifCause {
    #[error("LibRaw could not open file (code {libraw_code}: {})", libraw_strerror(*libraw_code))]
    OpenFailed { libraw_code: i32 },
    #[error("LibRaw opened file but EXIF fields are absent (corrupt CR3)")]
    ExifFieldsMissing,
    #[error("LibRaw reports unsupported format / camera (make={libraw_make:?} model={libraw_model:?})")]
    UnsupportedFormat { libraw_make: String, libraw_model: String },
    #[error("LibRaw resource exhausted (code {libraw_code})")]
    ResourceExhausted { libraw_code: i32 },
    #[error("EXIF field {field} malformed: raw_value={raw_value:?}")]
    ExifMalformed { field: &'static str, raw_value: String },
}

// RawDecodeCause similar — Open, Unpack, BufferTooSmall, etc.
```

- **`cause` field is typed** (closes PR1-T2 issue 1). LibRaw numeric
  codes preserved; operators can `match err.cause` to discriminate
  CVE-worthy "corrupt input" from operational "out of memory."
- **`photohelper-core::Error` does NOT gain LibRaw variants** (closes
  PR1-T2 issue 3). The CLI's `parse_exif_for` boundary converts
  `photohelper_raw::Error` → `Error::ExifLibraw { source: BoxedSourceError(Box::new(e)) }` (NEW variant in `photohelper-core::Error`) at the cross-crate boundary. The conversion is explicit `.map_err`, not `?`-bubbled — matches existing workspace discipline.
- **Dispatch site routing** (closes PR1-T2 issue 2 + PR1-T29 sad-path):
  `ingest_one` MUST `match` on the cause variant:
  - `ExifFieldsMissing` → log WARN with distinct `event = "cr3-exif-fields-absent"`; bump new `IngestStats::cr3_exif_absent` counter (NOT `no_exif` which has different semantics post-LibRaw); `--strict` rejects.
  - `OpenFailed` / `UnsupportedFormat` / `ResourceExhausted` → bump `IngestStats::errored`; `--strict` rejects.
  - `ExifMalformed` → bump `IngestStats::errored`; per-field WARN; `--strict` rejects.

#### Deliverable 2 — LibRaw build-system + LGPL §6(a) scaffolding

##### 2a — Build system (DI-2 resolved: vendored source + cmake)

- **Strategy locked at plan-review v2**: vendor LibRaw source under
  `crates/photohelper-raw/vendor/libraw-X.Y.Z/` (X.Y.Z pinned at first
  build.rs commit; recommended: latest 0.21.x patch release as of
  the session-02 implementation date).
- `crates/photohelper-raw/build.rs` invokes `cmake` (via the `cmake`
  crate) to compile the vendored LibRaw as a static library; links
  the result into `photohelper-raw`.
- **SHA-256 verification (per PR1-T10)**: the tarball SHA-256 is
  recorded at `crates/photohelper-raw/vendor/libraw-X.Y.Z.tar.gz.sha256`
  and verified by `build.rs` at the start of the build. Tampered
  tarball → build fails with actionable error.
- **Actionable build errors (per PR1-T36)**: `build.rs` emits
  `cargo:warning=` lines on system-toolchain failures naming the
  exact apt/brew/dnf package needed (e.g. "missing `cmake`; install
  via `brew install cmake` on macOS or `apt install cmake` on
  Debian/Ubuntu").
- **`cargo:rerun-if-changed=`** for vendored headers / source so a
  vendored-source bump triggers rebuild.

##### 2b — LGPL §6(a) compliance (decision-doc 0002)

- **`docs/decisions/0002-libraw-lgpl-static-link-mechanics.md`** records
  the §6(a) artifact shape: per-release `vendor/libraw-X.Y.Z.tar.gz`
  shipping alongside the binary in GitHub Releases, plus relinking
  instructions in the release notes template.
- **§6(a), not §6(b)** — verified via `docs/discovery-notes.md § DN-001`
  (corrected per plan-review PR1-T17). §6(b) is the shared-library
  mechanism we are NOT taking; static-linking-plus-vendored-source
  is §6(a) ("Accompany the work with the complete corresponding
  machine-readable source code … so that the user can relink").
- **Decision-doc 0002 explicitly defers** the actual GitHub Release
  workflow wiring (Authenticode, Homebrew tap, winget, the
  `.github/workflows/release.yml` that uploads the tarball alongside
  binaries) to the dedicated release-engineering session. DN-001's
  ownership is split: this session owns the decision-doc + build
  mechanism; release-engineering session owns the workflow wiring.
- **Legal review caveat**: decision-doc 0002 ships as DRAFT with
  status `"Accepted pending legal review before first GitHub Release
  tag"`. Per PR1-M18 (LGPL is not a plan-review-grade question), the
  release-engineering session re-validates with counsel before tagging
  v0.1.

#### Deliverable 3 — Real CR3 fixtures via Git LFS

- Git LFS initialized in the repo (`.gitattributes` + `.lfsconfig`).
  Standardize on "Git LFS" capitalization in prose (PR1-L11); use
  `git-lfs` only when referencing the CLI binary.
- Fixtures at `tests/fixtures/cr3/`:
  - At least 2 sanitized Canon R8 CR3s (different camera settings,
    different sensor crops if available).
  - Each fixture file: `>1 MB` (so a not-fetched LFS pointer of ~150 B
    fails the sanity helper per PR1-T13).
- **License audit (existing)**: every fixture is CC0 or equivalent
  unencumbered; sources cited in `tests/fixtures/cr3/README.md`.
- **EXIF sanitization gate (NEW per PR1-T11; CRITICAL)**: every fixture
  passes through:
  ```bash
  exiftool -all= -tagsfromfile @ \
    -Make -Model -Orientation -DateTimeOriginal \
    -ExifImageWidth -ExifImageHeight -Software \
    -overwrite_original sanitized.cr3
  ```
  Strips GPS / OwnerName / SerialNumber / Copyright /
  LensSerialNumber / CameraOwnerName / IPTC creator fields / embedded
  preview thumbnails (which themselves carry GPS+owner).
  `tests/fixtures/cr3/README.md` records the exact sanitization
  invocation + an `exiftool -G -a` "after" dump for each fixture.
- **CI sanitization lint (NEW per PR1-T11)**:
  `tests/fixtures/sanitize-check.sh` runs from `just ci` and asserts
  no PII tag (GPS, owner, serial, etc.) appears on any fixture.
  Failure ⇒ CI fails ⇒ unsanitized drop-in caught at PR time.
- **`fixture_is_real_cr3` helper (NEW per PR1-T13)**:
  `tests/common/fixtures.rs::fixture_is_real_cr3(path)` verifies the
  fixture is ≥1 MB AND first 16 bytes do NOT start with the LFS
  pointer magic (`version https://git-lfs`). Tests that depend on the
  fixture MUST call this helper at top; failure ⇒ `panic!()` with
  actionable message ("LFS not resolved; run `git lfs install && git
  lfs fetch && git lfs checkout`"). Silent-skip is explicitly rejected.
- **CI checkout shape**: `.github/workflows/ci.yml` uses
  `actions/checkout@<pinned-SHA>` with `lfs: true` (LFS fetched at
  checkout time). The TD-001 SHA-pinning for `actions/checkout`
  happens naturally as part of this update.
- **Developer onboarding (per PR1-T13)**: `README.md` gains a one-line
  note that `git lfs install` is now a `cargo test` prerequisite.

#### Deliverable 4 — `photohelper-cli::commands::ingest` rewired for LibRaw

Per PR1-T1 (path a: narrow scope) + PR1-T26 + PR1-T27:

- `RAW_EXTS` narrowed from 8 extensions to `["cr3"]` for v0.1. The
  walker's `is_raw_extension` filter now admits only `*.cr3`. Files
  with other RAW extensions (CR2/NEF/ARW/RAF/ORF/RW2/DNG) are walked
  but counted under `skipped (non-RAW)`. **Cross-ref DN-014** for the
  re-expansion binding trigger.
- `parse_exif_for(path, extension)` dispatcher DELETED. The old
  `parse_exif(path)` is replaced with a single function:
  ```rust
  fn parse_cr3_exif(path: &Path) -> Result<ExifMetadata, Error> {
      let raw_exif = photohelper_raw::exif::read_cr3(path)?;
      Ok(ExifMetadata::from(raw_exif))
  }
  ```
  `impl From<RawExif> for ExifMetadata` lives in
  `photohelper-cli::commands::ingest` (closes PR1-M4). Unit test pins
  the field-by-field conversion.
- **`kamadak-exif` workspace dep REMOVED atomically** (closes PR1-T26 +
  PR1-T27): `Cargo.toml` `[workspace.dependencies]` drops
  `kamadak-exif`; `crates/photohelper-cli/Cargo.toml` drops the dep;
  the existing JPEG-path test is deleted. Session 04+ JPEG sidecar
  work re-adds the dep when needed — no dead code shipping per
  R1.T2 lesson. **DN-006 status updates from "kamadak-exif fallback
  active" to "kamadak-exif removed in session 02; replaced by LibRaw
  for the only RAW format in v0.1."**
- **`ExifCompleteness` predicate (NEW per PR1-T14 CRITICAL)**:
  ```rust
  pub enum ExifCompleteness {
      Full,
      Partial { missing: Vec<&'static str> },
      Empty,
  }
  impl ExifMetadata {
      pub fn completeness(&self) -> ExifCompleteness { ... }
  }
  ```
  Replaces `is_empty()` semantics in the WARN gate. `IngestStats`
  gains `partial_exif` counter alongside the existing `no_exif`.
- **`--strict` semantics (per PR1-T14 + PR1-T29)**: strict fails on
  ANY of `unknown_camera > 0 || anomalous > 0 || errored > 0 ||
  no_exif > 0 || partial_exif > 0 || cr3_exif_absent > 0`. Cross-ref
  R2-T12 in the gate's source comment to anchor the discipline.
- **`IngestOutcome` exhaustivity** (closes PR1-T37 + PR1-M17): gains
  variant `InsertedWithPartialExif { photo_id: PhotoId, missing_fields: Vec<&'static str> }` that `apply_outcome` matches
  exhaustively. The compiler then catches drift (new counter without
  matching variant, or vice versa).
- **R2-T5 WARN gate corrected for partial-EXIF**: the gate fires on
  `ExifCompleteness::{Empty, Partial}`, with distinct WARN messages
  per variant. The PR1-T14 partial-EXIF silent-failure mode closes.

#### Deliverable 5 — `photohelper-catalog` touch + rusqlite bump

##### 5a — TD-002 rusqlite 0.32 → 0.40 bump

Per PR1-T12 (rusqlite API-compatibility enumeration):

- Bump `rusqlite` workspace dep from `0.32` to `0.40` (or whatever
  the latest at implementation time).
- **Enumerated API surface changes between 0.32 and 0.40** touching
  the catalog call sites (verified BEFORE the bump):
  - `Connection::open` / `open_with_flags` `OpenFlags` enum stability
    (specifically `SQLITE_OPEN_NO_MUTEX` default).
  - `TransactionBehavior::Immediate` signature stability.
  - `params!` macro variadic / type-coercion behavior.
  - `execute_batch` PRAGMA-row handling (specifically
    `PRAGMA journal_mode = WAL`).
  - `Error` variant additions (the `match
    rusqlite::Error::QueryReturnedNoRows` at `catalog.rs:321,343`
    MUST not silently dispatch to a wildcard arm).
- **Bump-verification tests (NEW per PR1-T12 + PR1-M6)**:
  - `PRAGMA journal_mode = WAL` read-back returns `"wal"` post-init.
  - Roundtrip test: open with bumped rusqlite → write a row → close →
    re-open → read the row back (pins the cross-version persistence
    contract).
  - `rusqlite::version_number() >= 3_045_000` assert (pins bundled
    SQLite version threshold to >= 3.45; a downgrade re-fails).
- **Risk register row 2 (rusqlite fallback)** reworded per PR1-L2:
  "introduces unforeseen API surface" not "API-breaks"; clarify TD-002
  already declared 0.40 API-compat for the ops we use.

##### 5b — Schema column population (no shape change)

- `Catalog::upsert` inputs now carry populated `Make`/`Model`/`CaptureTime`/`Width`/`Height`/`ExifOrientation` for CR3 rows (post-LibRaw). The SQL statements themselves are unchanged. **No
  `PRAGMA user_version` bump** because the schema shape doesn't
  change — populate-existing-NULLs is DML, not DDL.
- **NULL semantics shift (NEW per PR1-M14)**: pre-session-02 ingest
  runs left CR3 EXIF columns NULL ("we didn't try" semantics); post-
  session-02 runs populate them ("LibRaw extracted"). Catalogs
  rebuilt session-02-onwards have different NULL distribution from
  ones carried over from session-01 ingest runs. Plan commits the
  migration intent explicitly: **catalogs created by v0.1 with NULL
  CR3 columns are NOT backfilled by the session-02 binary;
  `ingested_at_unix_seconds < <session-02-merge-timestamp>` discriminates
  the two eras. `photohelper ingest --reindex` (deferred to session 03+)
  provides the explicit rebuild path.**
- **`unused_crate_dependencies` lint added (per PR1-T27 + PR1-AU)**:
  `[workspace.lints.rust] unused_crate_dependencies = "warn"`. With
  `kamadak-exif` workspace dep removed atomically with the dispatch
  removal (Deliverable 4), this lint catches the next inadvertent
  dep-without-purpose. Closes R2-T26's "add unused_crate_dependencies
  to workspace lints" obligation.

##### 5c — Decision-doc 0001 § History update

- Decision-doc 0001 already amended in the cross-doc fix commit
  (preceding plan v2) for the v1→v2 migration framework reschedule
  per PR1-T8. Session-end (not this commit) appends a § History entry
  noting "LibRaw landed in session 02; CR3 EXIF columns now
  populated; schema shape unchanged; NULL semantics as above."

#### Deliverable 6 — Test infrastructure (DN-008 subset + R2-T18 + R2-M8)

##### 6a — `poison_for_testing` knob (test-only)

Per PR1-T15 (CRITICAL):

```rust
#[cfg(test)]
impl Catalog {
    pub(crate) fn poison_for_testing(&self) { ... }
}
```

- `#[cfg(test)]`-gated visibility — not part of public API, not
  reachable from production binaries. Acceptance criterion gains: "No
  `*_for_testing` method exists in any production binary symbol table"
  (verifiable via `nm` on the release binary).
- Three distinct tests for the poison surface (closes PR1-T42; was
  collapsed to one in v1):
  - `poison_propagates_as_catalog_poisoned_error` — panic mid-tx →
    next `upsert` returns `Err(Error::CatalogPoisoned)`.
  - `poison_rollback_discards_panicked_workers_partial_insert` —
    count rows before panic; panic; assert `Err`; count rows after;
    MUST equal pre-panic count.
  - `poison_recovery_admits_subsequent_inserts` — after `Err`, fresh
    `upsert` succeeds AND the new row is queryable. **Note**: per
    `error.rs:111-117`, poison is PERMANENT — every subsequent op
    returns `CatalogPoisoned` until the Catalog is dropped and
    reopened. The test exercises the drop-and-reopen recovery, not
    in-place recovery.

##### 6b — R2-M8 silent rollback fix (per PR1-T32)

`catalog.rs:297` `let _ = conn.execute("ROLLBACK", [])` replaced with:
```rust
match conn.execute("ROLLBACK", []) {
    Ok(_) => {},
    Err(rusqlite::Error::SqliteFailure(_, Some(msg)))
      if msg.contains("cannot rollback - no transaction is active") => {},
    Err(e) => tracing::warn!(error = ?e, op = "poison-recovery-rollback", "ROLLBACK failed during poison recovery"),
}
```
Test row: poison + disk-full mid-rollback → WARN fires.

##### 6c — `panic_for_testing` knob + R2-T18 4/4 closure (per PR1-T4)

Per plan-review PR1-T4 (4-way CRITICAL convergence), R2-T18 closes
fully — not 3 of 4:

```rust
#[cfg(test)]
static HEARTBEAT_PANIC_FOR_TESTING: AtomicBool = AtomicBool::new(false);

// Inside heartbeat_loop (test builds only):
if HEARTBEAT_PANIC_FOR_TESTING.load(Ordering::Relaxed) {
    panic!("heartbeat death triggered by panic_for_testing");
}
```

Regression test asserts `heartbeat death-WARN` fires when the knob is
flipped mid-`run_ingest`.

##### 6d — DN-008 row coverage (per PR1-T3 CRITICAL)

**Covered this session (6 of 12)**: rows `{6, 17, 39, 42, 43, 49}`:
- Row 6: `trybuild` compile-fail test for the `assert_send_sync!(Arc<Catalog>)` invariant.
- Row 17 (RESTORED per PR1-T3): hardlink fixture → one row in `photos`; stderr contains `hardlink-dedup` INFO line at `-v`. (Per PR1-T3: hardlink tests need only a single CR3 fixture + the existing `AlreadyCatalogued` branch — no cull-pipeline dependency.)
- Row 39: `--strict` exit-code on a CR3-only directory (post-LibRaw).
- Row 42: walker edge cases (mtime in the future; nested dirs; broken symlinks).
- Row 43: `mtime_anomalous` flag round-trip.
- Row 49: fatal exit codes (catalog locked; permission denied; disk full mid-write).

**Deferred to session 03+ with explicit cross-ref (6 of 12)**: rows
`{12, 13, 14, 18, 19, 34}` (sum = 6 covered + 6 deferred = 12 DN-008
rows). Deferral rationale per row:
- Rows 12 (cross-process file-lock) + 13 (cross-process serialization): require multi-process test infrastructure; session 03+ when concurrent ingest matters.
- Rows 14 (concurrent `Catalog::open` races): same multi-process infra.
- Rows 18 (cull-score column queries) + 19 (dup-group queries): require cull pipeline + dup-group catalog tables (DN-005 / session 03).
- Row 34 (multi-camera fixture set): require non-Canon `CameraProfile` registry entries (DN-014).

**Row 32 (CR3 ingest happy path)** is NOT a DN-008 row — it's the
session-01 row whose EXIF assertions session-02 flips. Tracked under
Acceptance criterion 2a, not under DN-008.

##### 6e — R2-T18 four WARN regression tests (PR1-T4 + PR1-T44)

Per the §Test plan FFI error-path table below; key entries:
- Run `ingest` twice in-process → `build_global already initialized` WARN fires.
- Kill + reopen catalog → `wal_checkpoint recovered N frames` WARN fires.
- Parent-dir non-writable → `lock-file-create` op-tag WARN fires (R2-T11 sibling) AND `file-lock` op-tag WARN fires (R2-T18 path 4) — **independent tests** per PR1-T4 sub-issue.
- Heartbeat death (via `panic_for_testing` knob from 6c) → heartbeat death-WARN fires.

##### 6f — R2-T19 disposition (closes PR1-T30)

R2-T19's discriminating PhotoId test is **already landed at
`crates/photohelper-core/src/model.rs:770`** (per session-01 R2
remediation commit `681a3a2`; 9th-agent verified in
`docs/code-reviews/session-02-plan-round1.md § PR1-T30`). No
session-02 test work needed; the v1 plan's redundant "replace" wording
is dropped.

#### Deliverable 7 — DN-012 polish items (per PR1-T25 per-item enumeration)

**Folded into this session (binding trigger fires per DN-012's
enumerated surfaces touched)**:

- `KnownCamera::Display` impl — touched in `camera_slug` rendering
  when LibRaw populates EXIF (`format!("{camera}")` shorthand).
- `UpsertOutcome::#[non_exhaustive]` — touched when `Catalog::upsert`
  rewires for populated EXIF columns; uniformity with `Photo` and
  `Error`.
- **Workspace clippy allow-list per-line rationale comments** — touched
  when this plan adds `unused_crate_dependencies = "warn"` and the
  PR1-T21 lints; rationale comments added to each existing allow.

**Deferred with explicit binding trigger**:

- **Windows case-sensitivity walker filter** (DN-012 item 4 — not
  touched this session because session 02 doesn't change
  `ingest.rs::WalkBuilder`). New binding trigger per PR1-T25:
  "next session that touches `crates/photohelper-cli/src/commands/ingest.rs::WalkBuilder`
  filter OR by 2026-08-01." Updated in DN-012 at session-end.

### Out of scope (explicit deferrals)

| Item | Owner | Tracking |
|------|-------|----------|
| AI culling (`cull` subcommand real impl) | session 03 | unchanged |
| AI denoise (`develop` subcommand; model TBD pending session 04 plan-review) | session 04+ | unchanged |
| XMP sidecar I/O (`crs:` / `ph:` namespaces) | session 04+ | unchanged |
| JPEG export + watermarks (`export` subcommand) | session 05 | unchanged |
| `cull-score` + `dup-group` catalog tables + **migration framework v1 → v2** | session 03 | DN-005 + decision-doc 0001 § Amendments (rescheduled per PR1-T8) |
| Release-engineering wiring (musl static, codesign, Authenticode, winget, Homebrew tap, GitHub Release workflow, LGPL §6(a) tarball upload mechanism) | dedicated release session | DN-001 (decision-doc + build mechanism THIS session; workflow wiring LATER) |
| `scripts/verify-review-artifact.sh` (bash port of fox's mjs enforcer) | future session | DN-009 |
| GitHub Actions SHA pinning for non-`actions/checkout` actions | before first external PR / first release tag | TD-001 |
| Heartbeat-thread `.join()` cleanup | session 04 export pipeline (likely first toucher) OR by 2026-08-01 | TD-003 |
| DN-008 rows {12, 13, 14, 18, 19, 34} | session 03+ with cull pipeline / cross-process test infra / multi-camera registry | DN-008 (cross-ref this plan's Deliverable 6d enumeration) |
| DN-012 Windows case-sensitivity walker filter | next session touching `ingest.rs::WalkBuilder` filter OR by 2026-08-01 | DN-012 (binding-trigger update at session-end) |
| Windows build + cross-compile audit for LibRaw | v0.2 cut OR first Windows-using contributor | DN-013 |
| Other RAW formats (CR2, NEF, ARW, RAF, ORF, RW2, DNG) | session adding second `CameraProfile` (likely v0.3 / v0.4) | DN-014 |
| LibRaw C-library CVE monitoring (osv-scanner / similar wired into `just ci`) | first session touching `photohelper-raw` after 2026-08-01 OR any LibRaw CVE disclosure OR before first GitHub Release tag | TD-004 |
| Tile-based / streaming RAW decode (`read_raw_tile`, `read_raw_into`) | session 04 develop pipeline if memory pressure forces refactor | NEW: file as DN if it surfaces |
| Non-Bayer sensor support (X-Trans, Foveon, monochrome) | session adding a non-Bayer `CameraProfile` | NEW: file as DN if it surfaces |
| WB rebalance + per-illuminant color-matrix recovery in `RawImage` | session 04 develop pipeline when WB-edit feature is implemented | NEW: file as DN if it surfaces (per PR1-T19) |

### Plan-review decisions resolved at Round 1 (was "Discovery items")

Per PR1-T7 (CRITICAL): the v1 §Discovery items section used a `DI-N`
prefix not defined in CLAUDE.md conventions. v2 renames to
"Plan-review decisions" and either resolves inline OR moves to §Risk
register / DN-NNN with binding triggers:

- **DI-1 → resolved inline: hand-rolled minimal FFI shim** (Deliverable
  1a above). Rationale documented per PR1-T7b. Re-evaluation trigger:
  if hand-rolled FFI exceeds 10 functions OR LibRaw 0.22+ ABI break,
  escalate to plan-review v3.
- **DI-2 → resolved inline: vendored LibRaw source + cmake build**
  (Deliverable 2a above). Rationale: reproducible builds + LGPL §6(a)
  artifact alignment + version SHA-256 pinning.
- **DI-3 → moved to §Risk register** (Windows cross-compile) + filed
  as `DN-013` unconditionally per PR1-M9. Plan v1's "if it lands"
  qualifier was a No-Acceptable-Trade-offs Policy violation.
- **DI-4 → resolved inline: drop kamadak-exif this session**
  (Deliverable 4 above) AND file `DN-014` per PR1-T7e for the
  re-expansion of RAW_EXTS when the second camera profile lands. The
  v1 plan's contradiction between Deliverable 4 (keep) and DI-4
  (open) is resolved by atomic drop coordinated with the dispatch
  removal.

### Acceptance criteria (Definition of Done)

A session-02 merge candidate must satisfy all of:

1. **`just ci` green** locally and on GitHub Actions (all jobs).

2. **(2a — CI-verifiable)**: `cargo test --workspace` against the
   Git-LFS-committed CR3 fixtures (≥2 sanitized Canon R8 fixtures)
   asserts:
   - Catalog rows have non-NULL `make`, `model`, `camera_slug = 'canon-r8'`, `capture_time_unix_seconds`, `width > 0`, `height > 0`.
   - `--strict` mode exit code = 0 on the fixture set.
   - `stderr` contains `no-exif: 0` AND `cr3_exif_absent: 0` AND `partial_exif: 0` AND `errored: 0`.
   - `parse_cr3_exif` integration test passes.

   **(2b — manual smoke; recorded in PR description)**: pre-merge, the
   author runs (with the clean-catalog precondition):
   ```bash
   rm -rf /Users/ph/Pictures/tests/.photohelper/ && \
   photohelper ingest /Users/ph/Pictures/tests --strict
   ```
   Expected summary line (fresh-catalog, 371 walked files, 1 non-RAW
   skipped — per the corrected 370/370 count): `walked: 371, no-exif:
   0, ingested: 370, already-catalogued: 0, skipped (non-RAW): 1`.
   The author copy-pastes the actual summary into the PR body. **Not
   a CI gate** (the path is not portable per PR1-T6).

3. **`photohelper-raw::ffi` is the only crate with `unsafe` blocks**
   (enforced by lints, not by convention per PR1-T21):
   - Workspace `unsafe_code = "forbid"` applies to all crates.
   - `crates/photohelper-raw/Cargo.toml` overrides to `unsafe_code = "allow"`.
   - Within `photohelper-raw`, only `src/ffi.rs` allows `unsafe` (other modules `#![deny(unsafe_code)]` at head).
   - `#![deny(clippy::undocumented_unsafe_blocks)]` enforces every `// SAFETY:` comment as a compile-time gate.

4. **`cargo audit --deny warnings` clean** on the bumped `rusqlite` +
   the new LibRaw build inputs. **Caveat (per PR1-T10)**: `cargo
   audit` does NOT cover LibRaw C-library CVEs; TD-004 tracks the
   CVE-monitoring gap. Add a one-line caveat to the audit-run output.

5. **`docs/decisions/0002-libraw-lgpl-static-link-mechanics.md` exists**
   and records the §6(a) artifact shape (vendored tarball, relinking
   instructions, release-notes template snippet). Status:
   "Accepted pending legal review before first GitHub Release tag."

6. **Zero CRITICAL findings open at session end** (closed inline OR
   filed as TD/DN with binding triggers per `CLAUDE.md § No Acceptable
   Trade-offs Policy`). MEDIUM findings remediated before session end
   (per `docs/quality-assurance.md § Findings triage`). LOW findings
   ship with TD/DN entries OR accepted explicitly. HIGH carry-forward
   budget ≤ 2 per `docs/quality-assurance.md § Metrics`.

7. **LibRaw upstream pinned to exact `=X.Y.Z`** (per PR1-T10):
   - `crates/photohelper-raw/vendor/libraw-X.Y.Z.tar.gz` exists.
   - `crates/photohelper-raw/vendor/libraw-X.Y.Z.tar.gz.sha256` exists.
   - `build.rs` verifies the SHA-256 at build-time.
   - The exact X.Y.Z is recorded in decision-doc 0002 and in
     `Cargo.toml` metadata.

### Test plan

| Deliverable | Unit | Integration |
|-------------|------|-------------|
| Deliverable 0 — Pre-flight | n/a | Manual: `docs/analysis/ANL-001-libraw-cr3-preflight.md` records per-file pass/fail for the 371-CR3 set + aggregate stats. ABORT trigger: >5% failure. |
| LibRaw build-system (per PR1-L9) | n/a | CI matrix builds clean on `linux-x86_64` + `macos-arm64`; resulting binary statically links LibRaw (verified via `nm` on Linux / `otool -L` on macOS — no dynamic `libraw.dylib` reference). Build.rs SHA-256 verification fires on tampered tarball. Missing-cmake error produces actionable `cargo:warning=`. |
| `photohelper-raw::ffi` path encoding (PR1-T20) | NUL-byte interior → `Error::RawPath { reason: "interior-nul-byte" }`; non-UTF-8 on Unix → typed error; Windows long path → `\\?\` prefix added. | n/a |
| `photohelper-raw::ffi` LibRaw error-path table (PR1-T44) | n/a | Per row: `Error::RawExifUnavailable { cause: OpenFailed }` ← `chmod 000` + read; `... { cause: UnsupportedFormat }` ← CR2 fixture (closes PR1-M7); `... { cause: ExifFieldsMissing }` ← hex-edited CR3 with EXIF box zeros; `... { cause: ResourceExhausted }` ← `ulimit -v` low + decode; truncated CR3 → `RawExifUnavailable { OpenFailed }`; symlink loop → `Error::RawPath`. |
| `photohelper-raw::exif::read_cr3` field conversions (PR1-M4) | LibRaw stub fixture (synthesized `libraw_data_t`): trailing-NUL trim on `make`/`model`; `time_t = 0` / `i64::MAX` boundary on timestamp; each of LibRaw's 1..=8 orientation values round-trips through `ExifOrientation::from_tag`; `imgdata.sizes.iwidth = 0` → `Error::RawExifUnavailable { ExifMalformed }`; UTF-8-invalid bytes in `make` → typed error. | Real Canon R8 fixture: `make() == "Canon"`, `model() == "Canon EOS R8"` (or what LibRaw actually reports — recorded in ANL-001 pre-flight), `orientation() == ExifOrientation::Normal` (for a known-orientation fixture), `capture_time_unix_seconds().is_some()`, `width().get() > 0`, `height().get() > 0`. |
| `photohelper-raw::decode::read_raw` shape + invariants | `BayerPlane::new` rejects `data.len() != w*h` mismatch; `SensorLevels::new` rejects `black >= white`; `CfaPattern` derivable from each of the 4 valid `cdesc[4]` patterns. | Real Canon R8 fixture: `pixel_count == width * height`; `pixels[0..1000]` not all zero / not all `u16::MAX`; `levels.black < levels.white`; `matches!(cfa_pattern, CfaPattern::Rggb)` (R8-specific). |
| `From<RawExif> for ExifMetadata` conversion (PR1-M4) | Field-by-field mapping: `RawExif::orientation()` → `ExifMetadata::orientation: Some(_)`; `RawExif::width()` (`NonZeroU32`) → `ExifMetadata::width: Some(u32)`. | Covered by `parse_cr3_exif` integration test below. |
| `ingest` rewire (PR1-T1 + PR1-T14 + PR1-T29) | Mock-free: `parse_cr3_exif(real_cr3_path)` returns `Ok(metadata)` with `make = "Canon"`. `ExifCompleteness::completeness()` returns `Full` / `Partial { missing }` / `Empty` correctly. | **Happy path**: `photohelper ingest tests/fixtures/cr3 --strict` exits 0; catalog `SELECT COUNT(*) WHERE make IS NOT NULL` equals fixture count; same for `camera_slug = 'canon-r8'`; `stderr` substrings match Acceptance 2a. **Sad paths (PR1-T29)**: `strict_mode_fails_on_unknown_camera_real_cr3` (CR3 with Make recognized but Model not in registry → exit non-zero); `strict_mode_fails_on_libraw_error_real_cr3` (corrupted CR3 → exit non-zero); `strict_mode_fails_on_partial_exif_real_cr3` (hex-edited CR3 with capture-time absent → exit non-zero, `partial_exif: 1` in stderr). |
| Narrowed `RAW_EXTS = ["cr3"]` (PR1-T1) | n/a | Mixed-content directory with CR3 + ARW + NEF: walker walks all 3, ingests only CR3, counts other 2 under `skipped (non-RAW)`. |
| TD-002 rusqlite bump verification (PR1-T12 + PR1-M6) | n/a | (1) `PRAGMA journal_mode = WAL` read-back returns `"wal"` post-init. (2) Roundtrip: open → write row → close → re-open → read same row. (3) `rusqlite::version_number() >= 3_045_000` assert. (4) `match rusqlite::Error::QueryReturnedNoRows` arm exercised against an empty SELECT — confirms no silent wildcard dispatch. |
| `Catalog::poison_for_testing` (PR1-T15 + PR1-T42) | Three tests (split per PR1-T42): `poison_propagates_as_catalog_poisoned_error`; `poison_rollback_discards_panicked_workers_partial_insert`; `poison_recovery_admits_subsequent_inserts` (via drop-and-reopen — poison is permanent per `error.rs:111-117`). | n/a |
| R2-M8 silent ROLLBACK fix (PR1-T32) | n/a | Poison + simulated disk-full during ROLLBACK → WARN fires with `op = "poison-recovery-rollback"` substring. |
| `panic_for_testing` heartbeat knob + R2-T18 path 4 (PR1-T4) | n/a | Knob flipped → heartbeat panics → `run_ingest` summary still completes → stderr contains heartbeat-death-WARN substring. |
| R2-T18 paths 1-3 + R2-T11 sibling (PR1-T44) | n/a | Per the LibRaw error-path table above PLUS: `ingest` run twice in-process → `build_global already initialized` WARN; kill + reopen catalog → `wal_checkpoint recovered N frames` WARN; parent-dir read-only → BOTH `file-lock` AND `lock-file-create` op-tag WARNs (independent test rows per PR1-T4). |
| DN-008 row 6 (Send+Sync trybuild) | `trybuild` compile-fail on a `let _: Box<dyn Send + Sync> = Box::new(Arc::new(catalog))` regression that would unsink the assertion. | n/a |
| DN-008 row 17 (hardlink) | n/a | Hardlink fixture (CR3 file + hardlink to same): `ingest` writes ONE row; stderr contains `hardlink-dedup` INFO at `-v`. |
| DN-008 rows 39, 42, 43, 49 | Per row: see Deliverable 6d enumeration. |
| `git-lfs` fixture sanity (PR1-T13) | `fixture_is_real_cr3` helper unit: returns Err on a synthesized LFS pointer (`"version https://git-lfs..."`); returns Ok on a 1MB+ binary that doesn't start with that pointer. | All real-CR3 integration tests above call `fixture_is_real_cr3` at the top; failure ⇒ panic with actionable message. |
| Fixture EXIF sanitization (PR1-T11) | `tests/fixtures/sanitize-check.sh` lint: `exiftool -G -a` on every `tests/fixtures/cr3/*.cr3` MUST NOT contain GPS / OwnerName / SerialNumber / Copyright tag names. CI gate. | n/a |

### Checkpoints firing this session (Cadence A — per `docs/quality-assurance.md § Review cadence`)

**Always-on per protocol**: plan-review (firing now); session-end review.

**Session-02-specific sub-component reviews** (Tier 4, 3-5 agents):

| Checkpoint | When | Agents | Artifact |
|------------|------|--------|----------|
| Sub-component — `photohelper-raw::ffi` | When `ffi` module first exposes a non-scaffold public API | 3–5 (Tier 4) | `docs/code-reviews/session-02-photohelper-raw-ffi-round{1,2}.md` |
| Sub-component — LibRaw build-system / LGPL | When `build.rs` + decision-doc 0002 land | 3–5 (Tier 4) | `docs/code-reviews/session-02-libraw-build-round{1,2}.md` |

(Plan-review and session-end are mandatory per Cadence A § Review
cadence — Tier 5, full 8-agent suite.)

### Risk register

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| LibRaw fails to extract CR3 EXIF for our specific R8 firmware revision | low | Deliverable 0 pre-flight runs BEFORE FFI wiring; ABORT trigger at >5% failure. |
| Hand-rolled FFI shim grows beyond 10 functions in implementation | medium | DI-1 re-evaluation trigger: escalate to plan-review v3; consider `libraw-sys` adoption. |
| LibRaw vendored-source build (cmake) hits a compiler-flag landmine on macOS arm64 | medium | DI-2 re-evaluation trigger: switch to `pkg-config` against system LibRaw; defer Windows to v0.2 explicitly (DN-013). |
| LibRaw vendored-source build fails on contributor machine | medium | `build.rs` emits `cargo:warning=` lines naming exact toolchain packages needed (PR1-T36); `README.md` documents the prerequisite list. |
| `rusqlite 0.40` introduces unforeseen API surface needing migration | low | TD-002 declares 0.40 API-compatible for ops we use; Deliverable 5a enumerated API surface changes before bump; pre-merge spike validates. If a multi-file rewrite surfaces, fall back to `rusqlite 0.3X` intermediate and amend TD-002. |
| Git LFS bandwidth quota hit on free tier | low | Initial fixture set is ≤30 MB; quota threshold ≈ 1 GB/month free; revisit if total exceeds 500 MB. |
| Windows LibRaw cross-compile blocked (cluster: LibRaw + cmake + pkg-config + MSVC ABI) | medium | DN-013 bounds scope to v0.2; v0.1 ships Linux+macOS only; documented in `README.md`. If two or more LibRaw risks fire concurrently, escalate to a dedicated session for LibRaw FFI alone — split into session 02 (EXIF only) + session 02.5 (decode). |
| LibRaw C-library CVE disclosed during session 02 implementation | low | TD-004 tracks the monitoring gap; session-end check on LibRaw security advisories before merge; pre-merge holds if a CVE lands. |

### Cross-references (verb taxonomy below)

**Verb taxonomy (per PR1-L12)**:
- **closed** = the binding trigger is satisfied; the DN/TD/finding is fully resolved this session.
- **partial** = the DN/TD is partially advanced; remainder rolls forward with explicit binding trigger.
- **unchanged** = no action this session; cited for audit-trail completeness.

| ID | Disposition | Note |
|----|-------------|------|
| DN-001 (LibRaw LGPL §6(a)) | **partial** | Decision-doc 0002 + build mechanism land THIS session; GitHub Release workflow wiring + legal-review-confirmed status deferred to release-engineering session. (DN-001 ownership split per PR1-T18.) |
| DN-005 (catalog schema) | **partial** | v1 CR3 columns populated post-LibRaw; v1 schema shape unchanged; v1→v2 migration framework + cull-score / dup-group tables rescheduled to session 03 (decision-doc 0001 § Amendments + cross-doc commit). |
| DN-006 (kamadak-exif CR3 failure) | **closed** | `parse_cr3_exif` dispatches to LibRaw; kamadak-exif removed atomically (Deliverable 4); CR3 ingest behavior verified by Acceptance 2a + 2b. |
| DN-007 (rusqlite stale) | **closed** | DN-007's Owner per `discovery-notes.md:86` is TD-002; closing TD-002 IS DN-007 closure. Status updated to "reconciled (2026-MM-DD, closed via TD-002 close)" at session-end. |
| DN-008 (test infrastructure) | **partial** | 6 of 12 rows landed (`{6, 17, 39, 42, 43, 49}` per Deliverable 6d) + `poison_for_testing` knob; 6 deferred to session 03+ (`{12, 13, 14, 18, 19, 34}`) with cross-ref this plan's enumeration. |
| DN-009 (verify-review-artifact.sh) | **unchanged** | Future session; binding trigger 2026-09-01 OR before first review artifact lands on main post-this-session. |
| DN-011 (DN-006 production trace) | **closed** | Alongside DN-006 closure; pre-flight (Deliverable 0) verifies LibRaw extracts for the 371-CR3 set BEFORE wiring. |
| DN-012 (T15 polish items) | **partial** | 3 of 4 items folded in (`KnownCamera::Display`, `UpsertOutcome #[non_exhaustive]`, workspace clippy comments); Windows case-sensitivity walker filter deferred with new binding trigger ("next session touching `WalkBuilder` OR by 2026-08-01"). |
| DN-013 (Windows LibRaw cross-compile) | **filed this session** | Per cross-doc commit; binding trigger v0.2 cut OR first Windows-using contributor. |
| DN-014 (Other RAW formats) | **filed this session** | Per cross-doc commit; binding trigger first session adding non-Canon CameraProfile. |
| DN-015 (heartbeat panic_for_testing vs TD-003 distinction) | **filed this session** | Informational; clarifies session 02's `panic_for_testing` knob is distinct from TD-003's `.join()` cleanup. |
| TD-001 (GH Actions SHA pinning) | **partial** | `actions/checkout@<pinned-SHA>` lands as part of Deliverable 3's git-lfs CI work; other actions' SHA pinning unchanged (binding trigger unfired). |
| TD-002 (rusqlite stale) | **closed** | Bumped to 0.40 voluntarily ahead of the calendar trigger (2026-08-01); structural trigger NOT fired (no schema columns added); bundled with LibRaw work for churn-minimization. |
| TD-003 (heartbeat join) | **unchanged** | All three trigger clauses confirmed unfired: (a) we're not touching `run_ingest`'s post-walk teardown (session 04 export pipeline is the likely first toucher); (b) date `2026-08-01` not yet expired; (c) no test-flake from stderr-ordering observed since the R2 commit. DN-015 cross-references for clarity. |
| TD-004 (LibRaw CVE monitoring) | **filed this session** | Per cross-doc commit; binding trigger first session touching `photohelper-raw` after 2026-08-01 OR any LibRaw CVE disclosure OR before first GitHub Release tag. |
| R2-T18 (4 R1.T10 WARN regression tests) | **closed** | All 4 paths covered via `panic_for_testing` knob (Deliverable 6c) + Deliverable 6e tests. R2-T11 sibling `lock-file-create` covered independently. |
| R2-T19 (128KB PhotoId test) | **closed in session 01** | The discriminating test already exists at `model.rs:770` (commit `681a3a2`); no session-02 action. Plan v1's redundant "replace" wording dropped per PR1-T30. |
| R2-T22 / R2-T23 (R1 count drifts) | **unchanged** | Session-01 R2 disposition was "Fix inline in R2 remediation"; if the inline fix didn't actually land, verify at session-end and file a TD if regression. Cosmetic; not blocking. |
| R2-M8 (silent ROLLBACK) | **closed** | Per Deliverable 6b; explicit match-arm distinguishing expected "no transaction is active" from real errors. |
| R2-T26 (`unused_crate_dependencies` lint) | **closed** | Per Deliverable 5b; lint added to workspace; coordinated with the `kamadak-exif` workspace-dep removal (Deliverable 4) for atomicity. |

### Plan revisions log

- **v1 (2026-05-28)**: initial; pre plan-review. Bundles LibRaw EXIF +
  decode + rusqlite bump + minor DN-008 row coverage. Two FFI mechanics
  flagged as DI-1/DI-2 for plan-review.
- **v2 (2026-05-28)**: post plan-review Round 1. Addresses 16 CRITICAL
  + 17 HIGH + most MEDIUM inline; LOW deferred to v3 if R2 doesn't
  surface CRITICAL-class regressions. Key changes:
  - DI-1 resolved inline (hand-rolled minimal FFI shim).
  - DI-2 resolved inline (vendored LibRaw source + cmake).
  - DI-3 → DN-013 (Windows cross-compile) + risk register row.
  - DI-4 resolved inline (drop kamadak-exif atomically with dispatch
    removal); DN-014 files re-expansion trigger for RAW_EXTS.
  - `RAW_EXTS` narrowed to `["cr3"]` for v0.1 (was 7-format silent
    fall-through to kamadak-exif).
  - `RawExif` / `RawImage` rewritten as strong types (private fields
    + fallible constructors + accessor methods + `NonZeroU32` +
    `ExifOrientation` enum + `CfaPattern` enum + `SensorLevels` +
    `BayerPlane` + `Send + Sync` static_assertions).
  - Error enum collapsed to one variant per type (`RawExifUnavailable`
    / `RawDecodeFailed`) with typed `RawExifCause` / `RawDecodeCause`
    sub-enums; `cause` field explicit; LibRaw numeric codes preserved.
  - Acceptance criterion 2 split into 2a (CI-verifiable) + 2b (manual
    smoke); corrected the "371/371" count to "370/370 of files that
    reached the parser."
  - Decision-doc 0001 amended (cross-doc commit): migration framework
    v1→v2 rescheduled from session 02 to session 03.
  - LGPL §6(b) → §6(a) corrected in DN-001 (cross-doc commit) + plan.
  - `Catalog::poison_for_testing` declared `#[cfg(test)]`-only with
    visibility constraint enforced via `nm` check; test split into 3
    distinct invariants.
  - `panic_for_testing` knob + heartbeat death-WARN regression test
    landed (closes R2-T18 fully — was 3/4 in v1).
  - Pre-flight Deliverable 0 added (was orphaned in v1 risk register).
  - DN-008 row enumeration corrected: 6 covered (`{6, 17, 39, 42,
    43, 49}`) + 6 deferred (`{12, 13, 14, 18, 19, 34}`) = 12 DN-008
    rows. "Row 32" repositioned as Acceptance 2a, not DN-008.
  - `RAW_EXTS` narrowed atomically with `kamadak-exif` removal +
    `unused_crate_dependencies` lint addition (per R2-T26).
  - SCUNet model name removed from §Out of scope (was ungoverned model
    pin per PR1-M13).
  - LibRaw version pinned per Acceptance criterion 7; TD-004 tracks
    CVE-monitoring gap.
  - `parse_exif_for(path, extension)` dispatch dropped (YAGNI per
    PR1-T1); single `parse_cr3_exif` function in CLI.
  - `ExifCompleteness` predicate added per PR1-T14; new `partial_exif`
    + `cr3_exif_absent` stats counters; `--strict` rejects either.
  - `IngestOutcome` gains `InsertedWithPartialExif` variant for
    exhaustivity per PR1-T37.
  - Scope-creep meta-risk dropped from risk register (PR1-T45).
  - Cross-references taxonomy defined (closed/partial/unchanged).
  - Test-plan duplicate rows removed (TD-002-stays-green + git-lfs-
    plumbing redundant with `just ci` per PR1-T30 / PR1-M18).
  - "Detailed implementation" empty h2 dropped per PR1-T16 (matches
    session-01's prose-note convention; closes the half-document
    review-boundary concern).
