# Session 02 — `libraw-cr3-decode`

> **Branch**: `session-02/libraw-cr3-decode`
> **Started**: 2026-05-28
> **Cadence**: A (tier-graduated, per `CLAUDE.md § Quality gates` and
> `docs/quality-assurance.md § Review cadence`)
> **Author**: Paulo Henrique Lerbach Rodrigues (Claude Code)
> **Plan revisions**: v1 (initial) → v2 (post plan-review Round 1) →
> **v3 (this revision; post plan-review Round 2 — see
> `docs/code-reviews/session-02-plan-round2.md`)**

> **Note on title slug**: branch is `libraw-cr3-decode` but the session lands
> LibRaw EXIF read **AND** RAW pixel decode in one PR (see §Scope rationale).

## Session contract (top block — reviewed at plan-review checkpoints)

### Goal

Land the LibRaw FFI integration that turns `photohelper-raw` from a one-line
stub into a working RAW pipeline for Canon R8 CR3. Two complementary
deliverables under the same FFI surface:

1. **`photohelper-raw::exif::read_cr3(path) -> Result<RawExif>`** — extract
   `Make`, `Model`, `Orientation`, `CaptureTime`, `Width`, `Height` from a
   Canon R8 CR3 ISO-BMFF container. This is the **DN-011 critical-path
   remediation**: kamadak-exif fails on **370/370** real Canon R8 CR3s that
   reached the parser in DN-011's production trace (1 of 371 walked files
   was skipped pre-parse as non-RAW), so LibRaw EXIF is the only path to a
   usable `--strict` mode for CR3 ingest.

2. **`photohelper-raw::decode::read_raw(path) -> Result<RawImage>`** — decode
   the Bayer-pattern sensor data into a `RawImage` ready to feed
   session 04's develop pipeline.

`ingest_one` rewires to call LibRaw for all `*.cr3` files. v0.1 narrows
`RAW_EXTS` to `["cr3"]` (was 8 extensions); the 7-format walker behavior
moves to DN-014, which binds re-expansion to the session that adds the
second `CameraProfile`.

Once wired, integration test row 32 flips its assertions from `is_none()`
to `Some("canon-r8")` and `--strict` on real CR3 fixtures exits 0 — closing
the DN-006 / DN-011 binding triggers.

### Scope rationale (why bundle EXIF + decode + rusqlite bump)

LibRaw is a single C library. Wiring its FFI surface for EXIF only, then
re-wiring for decode in a later session, would mean doing the FFI safety
review, LGPL static-link plumbing, and build-system configuration twice.
The EXIF + decode pairing keeps the FFI surface defined once and reviewed
once.

The rusqlite bump is bundled by calendar trigger, not by schema-touch.
TD-002's binding trigger has two OR-joined clauses: (a) by 2026-08-01, OR
(b) before session 02 introduces new schema columns. Clause (b) is NOT
fired (no new columns; populate-existing-NULLs is DML, not DDL). Clause
(a) is the operative trigger; bundling the bump here is voluntary, ahead
of the calendar, because session 02 is already in catalog-crate code
populating those columns.

### Deliverables (when the PR merges, the following will exist)

#### Deliverable 0 — Pre-flight feasibility probe

Before any FFI wiring, verify LibRaw can actually extract EXIF from the
user's R8 firmware revision AND verify the chosen LibRaw version is
CVE-clean.

- **Sequencing**: fires AFTER Deliverable 1's lint-override + cargo.toml
  edits (need LibRaw bindings to call) AND BEFORE Deliverable 4's `ingest`
  rewire.
- **Artifact**: `docs/analysis/ANL-001-libraw-cr3-preflight.md` with:
  - **§ LibRaw version**: chosen X.Y.Z (now `=0.22.1`, escalated from the
    plan-v3.1 default `=0.21.4` per ANL-001 § LibRaw version; the
    implementer is empowered to pick a different 0.21.x patch but the
    0.22.x cross-series jump exceeds that authority and required user
    consultation under the No-Acceptable-Trade-offs Policy. Pin landed
    2026-05-28 because LibRaw 0.22.1's release notes carry six TALOS-2026
    fixes AND two CR3-parser-specific hardenings that did NOT backport
    to 0.21.5b).
  - **§ CVE-posture-as-of-pin** (closes DN-018): MITRE CVE feed +
    [LibRaw GitHub Security Advisories](https://github.com/LibRaw/LibRaw/security/advisories)
    grep on the pin date for any open CVE affecting the chosen version;
    record per-CVE pass/fail. ABORT if any open CVE → escalate to
    plan-review v4.
  - **§ EXIF extraction**: per-file pass/fail + extracted Make / Model /
    Orientation / CaptureTime / Width / Height for the 371-CR3 set at
    `/Users/ph/Pictures/tests`. ABORT if >5% failure → escalate to
    plan-review v4.
- **Commit shape**: dedicated `chore(libraw): pre-flight EXIF + CVE-posture
  audit (Deliverable 0)` commit; result auditable in `git log`. **No
  Deliverable 4 or Deliverable 5 commit may land before this one.**
- **Verification surface**: the pre-flight commit message MUST include a
  line `cve-posture: clean (versus MITRE feed YYYY-MM-DD)` AND
  `pass-rate: N/371 (>=95%)` so session-end review can grep the commit.

#### Deliverable 1 — `photohelper-raw` real implementation

##### 1a — FFI module (`crates/photohelper-raw/src/ffi.rs`)

- **Strategy locked at plan-review v3 (per R2-T14)**: hand-rolled minimal
  FFI shim using LibRaw's **C-API accessor functions** (NOT `#[repr(C)]`
  field-access against `libraw_data_t`). LibRaw upstream documents
  `libraw_get_*` accessors as ABI-stable across version bumps; direct
  field-access against `libraw_data_t` is silently fragile across 0.21.x
  patch reorders.
- **Function set bound** (~15 functions; trigger to re-evaluate raised to
  >20):
  - Lifecycle: `libraw_init`, `libraw_open_file`, `libraw_open_wfile`
    (Windows wide-path; per R2-T15 — the real symbol; previously
    fabricated as `libraw_open_file_w`), `libraw_unpack`, `libraw_recycle`,
    `libraw_close`, `libraw_strerror`.
  - Accessors: `libraw_get_iwidth`, `libraw_get_iheight`,
    `libraw_get_cam_mul`, `libraw_get_pre_mul`, `libraw_get_rgb_cam`,
    `libraw_get_color_maximum`, `libraw_get_iparams` (returns `libraw_iparams_t` struct whose `cdesc[4]` field carries the per-channel color-naming string AND whose `filters` field carries the 2x2 CFA mosaic bitmask — use `LIBRAW_COLOR(filters, row, col)` recipe for `CfaPattern` discrimination per R3-T2 correction; `libraw_get_cdesc` is NOT a real symbol).
- **`unsafe_code` discipline (per PR1-T21; Cargo lint-syntax constraint per R3-M2 — verified against [Cargo manifest reference](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section))**:
  - `crates/photohelper-raw/Cargo.toml [lints]` REMOVES `workspace = true`
    and ADDS explicit per-key: `[lints.rust] unsafe_code = { level = "allow", priority = 1 }` + RESTATES every workspace lint (`missing_docs = "warn"` from `[lints.rust]`; `pedantic`, `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, plus the existing allow overrides from `[lints.clippy]`) explicitly per-crate. Cargo lint inheritance does NOT merge with per-key overrides per the [Cargo manifest reference](https://doc.rust-lang.org/cargo/reference/manifest.html#the-lints-section); restating preserves R2-T8's intent (no enforcement loss on clippy lints).
  - `src/ffi.rs` head: `#![deny(unsafe_op_in_unsafe_fn)]` — every `unsafe fn` body still requires inner `unsafe { ... }` with `// SAFETY:` comment.
  - `src/exif.rs`, `src/decode.rs` heads: `#![forbid(unsafe_code)]` (NOT `deny`; the `forbid` ratchet cannot be downgraded by inner attributes, so new modules accidentally adding `unsafe` fail at compile time). **Plan-v3.2 correction (per Deliverable 1a scaffolding commit)**: `src/lib.rs` does NOT carry the file-level `forbid` even though earlier plan revisions listed it. Reason: inner attributes at the crate root (`lib.rs`) propagate to every submodule, and `forbid` cannot be downgraded — `#![forbid(unsafe_code)]` at `lib.rs` would make `ffi.rs`'s `unsafe` blocks fail to compile no matter what attribute `ffi.rs` carries. The Cargo.toml crate-level `allow` is the lib.rs baseline; the file-level `forbid` lives on `exif.rs`/`decode.rs` (and any future non-FFI source files); the CI grep gate below is the third defense layer.
  - Workspace `Cargo.toml` `[workspace.lints.clippy]` adds `undocumented_unsafe_blocks = "deny"`.
  - **CI grep gate** (defense-in-depth for new files inheriting crate-level
    `allow`): `just ci` runs `scripts/check-unsafe-isolation.sh` (a wrapper around `rg --type rust --glob '!ffi.rs' '\bunsafe\s*(\{|fn\b|trait\b|impl\b)' crates/photohelper-raw/src/`) and fails if any match.
- **Path encoding (per PR1-T20)**:
  - `pub(crate) struct RawPath` newtype wrapping a `&Path`: NUL-byte
    interior → `Err(Error::RawPath { reason: "interior-nul-byte" })`;
    non-UTF-8 path on Unix → typed error (not panic); Windows long path →
    automatically `\\?\`-prefixed.
  - Per-OS conversion: Unix uses `OsStr::as_bytes() + CString::new()`;
    Windows uses `OsStr::encode_wide() + null-terminate + libraw_open_wfile`.

##### 1b — `RawExif` type (`crates/photohelper-raw/src/exif.rs`)

Per PR1-T5 (strong-type discipline), `RawExif` is private-fields +
fallible constructor + accessor methods — NOT bag-of-public-fields:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawExif {
    make: String,
    model: String,
    orientation: ExifOrientation,            // strong enum, not int
    capture_time_unix_seconds: Option<i64>,  // matches catalog wire shape
    width: NonZeroU32,                       // non-zero invariant via type
    height: NonZeroU32,                      // non-zero invariant via type
}
static_assertions::assert_impl_all!(RawExif: Send, Sync);  // module-scope, NOT cfg(test)

impl RawExif {
    pub(crate) fn from_libraw_fields(fields: RawExifFields) -> Result<Self, Error> { ... }

    pub fn make(&self) -> &str { ... }
    pub fn model(&self) -> &str { ... }
    pub fn orientation(&self) -> ExifOrientation { ... }
    /// Capture time as Unix seconds (UTC).
    ///
    /// **UTC assumption**: LibRaw's `imgdata.other.timestamp` is `time_t`
    /// interpreted as wall-clock UTC absent EXIF timezone metadata.
    /// CR3 EXIF's `DateTimeOriginal` is naïve wall-clock; the UTC assumption
    /// is the safest default for chronological sorting. **DN-016** tracks
    /// per-EXIF-tag timezone recovery for v0.2.
    pub fn capture_time_unix_seconds(&self) -> Option<i64> { ... }
    pub fn width(&self) -> NonZeroU32 { ... }
    pub fn height(&self) -> NonZeroU32 { ... }
}

struct RawExifFields {
    make: String,
    model: String,
    orientation: ExifOrientation,
    capture_time_unix_seconds: Option<i64>,
    width: NonZeroU32,
    height: NonZeroU32,
}

pub fn read_cr3(path: &Path) -> Result<RawExif, Error> {
    let raw_path = RawPath::new(path)?;
    let fields = ffi::parse_libraw_fields(&raw_path)?;
    RawExif::from_libraw_fields(fields)
}
```

- `orientation: ExifOrientation` (not `u8` / `i64`); out-of-range from
  LibRaw → `Error::RawExifUnavailable { cause: ExifMalformed { field: "orientation", raw_value } }`.
- `capture_time_unix_seconds: Option<i64>` field type matches the public
  accessor for derive-stability (per R2-M7; avoids `time::OffsetDateTime`
  `Eq` instability across the `time` crate's minor versions). If conversion
  in `from_libraw_fields` needs `OffsetDateTime` as a stepping-stone, do
  the conversion in-line and store as `i64`.
- LibRaw value source: `libraw_get_iwidth` / `libraw_get_iheight`
  (post-rotation visible-area pixels). Documented in decision-doc 0001's
  § History amendment at session-end.
- Constructor split per R2-T5: `read_cr3` (public) → `ffi::parse_libraw_fields` (parse boundary) → `RawExif::from_libraw_fields` (small cross-field validation; currently no cross-field invariants). Keeps the FFI-shape-coupling in one parse function unit-testable with synthesized `libraw_data_t`.

##### 1c — `RawImage` + companion types (`crates/photohelper-raw/src/decode.rs`)

Per PR1-T5 + R2-T5 + R2-T6 (strong types extend to ALL fields including
WhiteBalance / ColorMatrix; accessors do NOT panic on OOB):

```rust
#[derive(Debug)]  // NO Clone (50 MB heap)
pub struct RawImage {
    pixels: BayerPlane,                          // length invariant inside
    cfa_pattern: CfaPattern,                     // 4-variant enum
    levels: SensorLevels,                        // black < white invariant
    as_shot_white_balance: WhiteBalance,         // RGGB-order private fields
    color_matrix: CamRgbToXyzD65Matrix,          // direction in type
}
static_assertions::assert_impl_all!(RawImage: Send, Sync);

// BayerPlane: holds the raw pixel buffer + dimensions
#[derive(Debug)]  // NO Clone (50 MB heap)
pub struct BayerPlane {
    data: Box<[u16]>,
    width: NonZeroU32,
    height: NonZeroU32,
}

impl BayerPlane {
    pub(crate) fn new(data: Vec<u16>, width: NonZeroU32, height: NonZeroU32) -> Result<Self, Error> {
        let expected = (width.get() as u64) * (height.get() as u64);
        if data.len() as u64 != expected {
            return Err(Error::RawImageDimensionMismatch {
                path: PathBuf::new(),
                declared_pixels: expected,
                actual_pixels: data.len() as u64,
            });
        }
        Ok(Self { data: data.into_boxed_slice(), width, height })
    }

    // Per R2-T5: ALL accessors are fallible (no panic on OOB).
    pub fn width(&self) -> NonZeroU32 { self.width }
    pub fn height(&self) -> NonZeroU32 { self.height }
    pub fn row(&self, y: u32) -> Option<&[u16]> {
        if y >= self.height.get() { return None; }
        let w = self.width.get() as usize;
        let start = y as usize * w;
        self.data.get(start..start + w)
    }
    pub fn pixel(&self, x: u32, y: u32) -> Option<u16> {
        let row = self.row(y)?;
        row.get(x as usize).copied()
    }
    // Iterator API for session 04 demosaic (preferred over indexed access):
    pub fn rows(&self) -> impl Iterator<Item = &[u16]> {
        let w = self.width.get() as usize;
        self.data.chunks_exact(w)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CfaPattern {
    Rggb,
    Bggr,
    Grbg,
    Gbrg,
    // X-Trans / Foveon / monochrome deferred per DN-014.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensorLevels {
    black: u16,
    white: u16,
    bit_depth: SensorBitDepth,  // R2-T6 expansion: dynamic-range floor
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SensorBitDepth(u8);  // constrained 8..=16 via SensorBitDepth::new

impl SensorBitDepth {
    pub(crate) fn new(bits: u8) -> Result<Self, Error> {
        if !(8..=16).contains(&bits) {
            return Err(Error::RawInvalidBitDepth { value: bits });
        }
        Ok(Self(bits))
    }
    pub fn get(&self) -> u8 { self.0 }
}
// Per R3-T5: SensorLevels::new must call bit_depth.get() (NOT bit_depth.0).
// Add Error::RawInvalidBitDepth { value: u8 } variant to the Error enum.

impl SensorLevels {
    pub(crate) fn new(black: u16, white: u16, bit_depth: SensorBitDepth) -> Result<Self, Error> {
        if black >= white {
            return Err(Error::RawInvalidLevels { path: PathBuf::new(), black, white });
        }
        // Dynamic-range floor: at least 256 steps (rejects black=0, white=1 nonsense)
        if (white - black) < 256 {
            return Err(Error::RawInvalidLevels { path: PathBuf::new(), black, white });
        }
        // Bit-depth bound: white must fit in the declared bit depth
        let max_for_depth = (1u32 << bit_depth.0) - 1;
        if (white as u32) > max_for_depth {
            return Err(Error::RawInvalidLevels { path: PathBuf::new(), black, white });
        }
        Ok(Self { black, white, bit_depth })
    }
    pub fn black(&self) -> u16 { self.black }
    pub fn white(&self) -> u16 { self.white }
    pub fn bit_depth(&self) -> SensorBitDepth { self.bit_depth }
}

// WhiteBalance: LibRaw cam_mul is documented R/G1/B/G2 on Canon
// (NOT RGGB — common misconception; see LibRaw API docs).
// Note: G1 and G2 are the two Bayer greens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhiteBalance {
    r: f32,
    g1: f32,
    b: f32,
    g2: f32,
}

impl WhiteBalance {
    pub(crate) fn from_libraw_cam_mul(cam_mul: [f32; 4]) -> Result<Self, Error> {
        let [r, g1, b, g2] = cam_mul;
        // R2-T6: reject "unloaded" (all-zero)
        if cam_mul.iter().all(|x| *x == 0.0) {
            return Err(Error::RawDecodeFailed {
                path: PathBuf::new(),
                cause: RawDecodeCause::WhiteBalanceUnloaded,
            });
        }
        // R2-T6: reject NaN / negative (physically nonsense)
        if cam_mul.iter().any(|x| !x.is_finite() || *x < 0.0) {
            return Err(Error::RawDecodeFailed {
                path: PathBuf::new(),
                cause: RawDecodeCause::WhiteBalanceInvalid { values: cam_mul },
            });
        }
        Ok(Self { r, g1, b, g2 })
    }
    pub fn r(&self) -> f32 { self.r }
    pub fn g1(&self) -> f32 { self.g1 }
    pub fn b(&self) -> f32 { self.b }
    pub fn g2(&self) -> f32 { self.g2 }
}

// ColorMatrix: direction encoded in type name (per R2-T6).
// CamRGB → XYZ at D65 illuminant. v0.1 ships as-shot only;
// per-illuminant matrices (D55, A, etc.) deferred per DN-017.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CamRgbToXyzD65Matrix {
    m: [[f32; 3]; 3],
}

impl CamRgbToXyzD65Matrix {
    pub(crate) fn from_libraw_rgb_cam(rgb_cam: [[f32; 3]; 3]) -> Result<Self, Error> {
        // R2-T6: reject identity-as-unloaded
        let is_identity = (0..3).all(|i| (0..3).all(|j| {
            let expected = if i == j { 1.0 } else { 0.0 };
            (rgb_cam[i][j] - expected).abs() < 1e-6
        }));
        if is_identity {
            return Err(Error::RawDecodeFailed {
                path: PathBuf::new(),
                cause: RawDecodeCause::ColorMatrixUnloaded,
            });
        }
        // R2-T6: reject all-zero / NaN entries
        if rgb_cam.iter().flatten().any(|x| !x.is_finite()) {
            return Err(Error::RawDecodeFailed {
                path: PathBuf::new(),
                cause: RawDecodeCause::ColorMatrixInvalid,
            });
        }
        Ok(Self { m: rgb_cam })
    }
    pub fn as_array(&self) -> &[[f32; 3]; 3] { &self.m }
}
```

- **Memory pressure SLO (per R2-T16 — corrected from v2's under-quote)**:
  per-decode peak is `2 × width × height × 2 bytes` (BayerPlane + LibRaw
  internal buffer overlap during ownership transfer) PLUS LibRaw's
  4-channel demosaic prep buffer (~96-200 MB), total **~150-250 MB per
  worker** for R8 24Mpix raw. With rayon's 8 workers transient peak is
  **~1.2-2 GB**. Documented inline so session 04's develop pipeline can
  plan back-pressure. Test plan asserts the per-decode RSS bound.
- **Ownership-transfer mechanism (per R2-T16)**: `BayerPlane::new` COPIES
  from `imgdata.rawdata.raw_image` into a new `Vec<u16>`; LibRaw's
  internal buffer freed by `libraw_recycle` after the copy. This costs
  one full-buffer duplication per decode but keeps the deallocation
  contract Rust-native (no `Box`-from-LibRaw-allocator).

##### 1d — Error enum (`crates/photohelper-raw/src/lib.rs`)

Per PR1-T2 (5-way CRITICAL) + R2-T7 (Error::Exif coordination) + R2-T13
(op tags) + R2-T12 (RawDecodeCause simplification):

```rust
// in photohelper-raw::Error (NOT photohelper-core; keeps core
// storage-agnostic and free of LibRaw transitive dependency — R1
// strength preserved; cross-ref session-01 R2 "core → ⊥" claim)
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
    #[error("RAW invalid sensor levels at {path}: black={black}, white={white}")]
    RawInvalidLevels { path: PathBuf, black: u16, white: u16 },
    #[error("RAW path validation failed at {path}: {reason}")]
    RawPath { path: PathBuf, reason: &'static str },
}

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum RawExifCause {
    // R2-T13: op: tag preserved so operators can discriminate
    // "LibRaw OOM during open" from "OOM during unpack."
    #[error("LibRaw call failed (op={op}, code={libraw_code})")]
    LibRawCallFailed { libraw_code: i32, op: &'static str },
    #[error("LibRaw opened file but EXIF fields are absent (corrupt CR3)")]
    ExifFieldsMissing,
    #[error("LibRaw reports unsupported format / camera: make={libraw_make:?} model={libraw_model:?}")]
    UnsupportedFormat { libraw_make: String, libraw_model: String },
    #[error("EXIF field '{field}' malformed: raw_value={raw_value:?}")]
    ExifMalformed { field: &'static str, raw_value: String },
}

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum RawDecodeCause {
    // Per R2-T12 path (b): a single LibRaw-call sink + typed-WB/ColorMatrix
    // sub-variants. The libraw_code carries the discrimination signal;
    // operator routing in session 04 develop pipeline.
    #[error("LibRaw call failed (op={op}, code={libraw_code})")]
    LibRawCallFailed { libraw_code: i32, op: &'static str },
    #[error("White balance unloaded by LibRaw (all-zero cam_mul)")]
    WhiteBalanceUnloaded,
    #[error("White balance invalid (NaN or negative values): {values:?}")]
    WhiteBalanceInvalid { values: [f32; 4] },
    #[error("Color matrix unloaded by LibRaw (identity matrix)")]
    ColorMatrixUnloaded,
    #[error("Color matrix invalid (NaN entries)")]
    ColorMatrixInvalid,
}
```

- **`cause` is typed enum** (closes PR1-T2 issue 1). LibRaw numeric code
  preserved via `libraw_code`; `op` tag preserved via `&'static str`.
- **`photohelper-core::Error` does NOT gain LibRaw variants** (closes
  PR1-T2 issue 3 + R2-T7). The CLI's `parse_cr3_exif` boundary converts
  `photohelper_raw::Error` → `photohelper_core::Error::Exif { path, source: Box::new(e) }` at the cross-crate boundary. **Per R2-T7**: the existing `Error::Exif` variant in `photohelper-core/src/error.rs:36-43` is RECYCLED (no rename, no deprecation, no new variant); the constructor uses `source: Box::new(e)` directly (NO `BoxedSourceError(...)` wrap — `BoxedSourceError` is a type alias per `error.rs:17`, not a tuple struct).
- **Dispatch-site routing (per PR1-T2 + PR1-T14 + R2-T20)**: `ingest_one` MUST `match` on the cause variant:
  - `RawExifCause::ExifFieldsMissing` → log WARN with `event = "cr3-exif-fields-absent"`; bump `IngestStats::cr3_exif_absent`; `--strict` rejects.
  - `RawExifCause::LibRawCallFailed { op, libraw_code }` → log WARN with op + code; bump `IngestStats::errored`; `--strict` rejects.
  - `RawExifCause::UnsupportedFormat { ... }` → log WARN; bump `IngestStats::unknown_camera`; `--strict` rejects.
  - `RawExifCause::ExifMalformed { field, raw_value }` → log WARN per field; bump `IngestStats::errored`; `--strict` rejects.

#### Deliverable 2 — LibRaw build-system + LGPL §6(a) scaffolding

##### 2a — Build system

- **Strategy locked at plan-review v3** (per R2-T4) and **version escalated
  at plan-v3.2** (post-Deliverable-0 per `docs/analysis/ANL-001-libraw-cr3-preflight.md`):
  vendor LibRaw **=0.22.1** at `crates/photohelper-raw/vendor/libraw-0.22.1/`.
  Plan-v3.1 targeted `=0.21.4`; Deliverable 0 escalated to `=0.22.1` because
  the 0.22.1 release ships TALOS-2026 fixes + two CR3-parser hardenings
  that did NOT backport to 0.21.5b (LibRaw 0.21.x is effectively EOL post-
  2025-12-25).
- `crates/photohelper-raw/build.rs` invokes `cmake` (via the `cmake`
  crate) to compile vendored LibRaw as a static library; links the result
  into `photohelper-raw`.
- **SHA-256 verification**: tarball SHA-256 recorded at
  `crates/photohelper-raw/vendor/libraw-0.22.1.tar.gz.sha256` and verified
  by `build.rs` at the start of the build. Tampered tarball → build fails
  with actionable error.
- **Actionable build errors**: `build.rs` emits `cargo:warning=` lines on
  system-toolchain failures naming the exact package needed (e.g.
  "`brew install cmake`" / "`apt install cmake`").

##### 2b — LGPL §6(a) compliance (`docs/adr/0002-libraw-lgpl-static-link-mechanics.md`)

Per R2-M2 (PR1-L5): this is an **ADR** (binding for every release), not a
smaller decision-doc. File at `docs/adr/0002-libraw-lgpl-static-link-mechanics.md`.

- Records the §6(a) artifact shape: per-release
  `vendor/libraw-0.22.1.tar.gz` ships alongside the binary in GitHub
  Releases; relinking instructions in the release-notes template.
- **§6(a), not §6(b)** — verified via `docs/discovery-notes.md § DN-001`
  (corrected per plan-review PR1-T17). Quotes LGPL-2.1 §6(a) verbatim.
- **DN-001 ownership split**: this session owns decision-doc + build
  mechanism; release-engineering session owns the GitHub Release workflow
  that actually uploads the tarball.
- **Legal review caveat (per PR1-M18)**: ADR ships as DRAFT with status
  `"Accepted pending legal review before first GitHub Release tag"`.
  Release-engineering session re-validates with counsel before tagging v0.1.

#### Deliverable 3 — Real CR3 fixtures via Git LFS

- Git LFS initialized (`.gitattributes` + `.lfsconfig`). Standardize on
  "Git LFS" capitalization in prose; `git-lfs` only for the CLI binary
  (per PR1-L11 + R2-L5).
- Fixtures at `tests/fixtures/cr3/`: ≥2 sanitized Canon R8 CR3s, each `>1 MB`.
- **License audit**: every fixture CC0 or equivalent unencumbered; sources
  cited in `tests/fixtures/cr3/README.md`.
- **EXIF sanitization gate (PR1-T11)**: every fixture passes through
  `exiftool -all= -tagsfromfile @ -Make -Model -Orientation -DateTimeOriginal -ExifImageWidth -ExifImageHeight -Software -overwrite_original sanitized.cr3`. README records exact invocation + `exiftool -G -a` "after" dump per fixture.
- **CI sanitization lint (per R2-T9 — REWRITTEN as allow-list, NOT
  deny-list)**: `tests/fixtures/sanitize-check.sh` runs from `just ci`
  and asserts:
  - `exiftool -G -a -ee` output on every `*.cr3` fixture contains ONLY
    the asserted-survivor tag set (Make, Model, Orientation,
    DateTimeOriginal, ExifImageWidth, ExifImageHeight, Software, plus
    mandatory ISO-BMFF container metadata: `[File]:FileType`,
    `[File]:FileSize`, `[ExifTool]:ExifToolVersion`, etc.). Any other
    tag → CI fails.
  - **Two-stage embedded-preview check** (per R3-T8): `exiftool -ee` does NOT actually descend into IFD0:Preview embedded JPEGs in CR3 (per ExifTool docs, `-ee` extracts from EPS/PDF/JPEG MPF/AVCHD streams, not from embedded-preview-JPEG inside a CR3). The sanitize-check MUST: (1) `exiftool -b -PreviewImage "$fixture" > /tmp/preview.jpg 2>/dev/null || true`; (2) if `/tmp/preview.jpg` is non-empty, run `exiftool -G -a /tmp/preview.jpg` and assert it contains ONLY the asserted-survivor tag set (same allow-list). Without (2), GPS/owner data in the preview JPEG ships unsanitized despite the parent CR3 being clean.
  - `exiftool` version pinned to a specific point release for reproducibility (e.g. `exiftool 13.06`); pin recorded as a hard equality check in `sanitize-check.sh`: `[ "$(exiftool -ver)" = "13.06" ] || { echo "exiftool version mismatch"; exit 1; }`.
- **`fixture_is_real_cr3` helper (PR1-T13)**:
  `tests/common/fixtures.rs::fixture_is_real_cr3(path)` verifies file
  is ≥1 MB AND first 16 bytes do NOT start with the LFS pointer magic
  (`version https://git-lfs`). Tests that depend on the fixture MUST
  call this helper; failure ⇒ `panic!()` with actionable message.
  **Silent-skip explicitly rejected.**
- **CI checkout shape**: `.github/workflows/ci.yml` uses
  `actions/checkout@<pinned-SHA>` with `lfs: true` (LFS objects fetched
  at checkout time). This pins `actions/checkout` per TD-001 incidentally
  (other actions' SHA pins NOT advanced; TD-001 remains `unchanged` —
  see §Cross-references).
- **Developer onboarding**: `README.md` gains a one-line note that
  `git lfs install` is now a `cargo test` prerequisite.

#### Deliverable 4 — `photohelper-cli::commands::ingest` rewired for LibRaw

##### 4a — `RAW_EXTS` narrowing + dispatch removal

**Per R2-T8 (atomic commit shape)**: ONE conventional commit
`refactor(session-02): atomic kamadak-exif removal + RAW_EXTS narrow +
unused_crate_dependencies lint` containing ALL of:

1. `crates/photohelper-cli/src/commands/ingest.rs:27` `RAW_EXTS` constant changed from 8 extensions to `&["cr3"]`. Walker's `is_raw_extension` filter consults the new constant; non-CR3 files fall through to `SkippedNonRaw`.
2. `parse_exif_for(path, extension)` dispatcher DELETED; replaced with `parse_cr3_exif(path) -> Result<ExifMetadata, Error>` (single function, no dispatch).
3. `Cargo.toml [workspace.dependencies] kamadak-exif = "0.6"` line removed.
4. `crates/photohelper-cli/Cargo.toml` per-crate `kamadak-exif.workspace = true` line removed.
5. `crates/photohelper-cli/tests/cli.rs` JPEG-path test deleted.
6. `Cargo.toml [workspace.lints.rust]` adds `unused_crate_dependencies = "warn"`.
7a. **`crates/photohelper-core/Cargo.toml` removes the unused `trybuild.workspace = true` dev-dep declaration** (closes R3-T4: the lint added in item 6 would otherwise fire on this existing declaration; per `photohelper-core/Cargo.toml:31`'s "declared but unused in this session" comment, the dep was scaffolded for DN-008 row 6 which now lands separately under Deliverable 6d's `trybuild` test). When the row-6 trybuild test lands (Deliverable 6d), it re-declares `trybuild.workspace = true` in the SAME commit that consumes it.
7. `docs/discovery-notes.md § DN-006 Status` updated to "kamadak-exif removed in session 02; replaced by LibRaw for the only RAW format in v0.1."

All 7 in one commit so `just ci` is green at every commit boundary.
Cross-ref DN-014 for re-expansion binding trigger.

##### 4b — `ExifCompleteness` predicate (per PR1-T14)

`ExifCompleteness` lives in `crates/photohelper-core/src/model.rs` next
to `ExifMetadata` (per R2-M1):

```rust
pub enum ExifCompleteness {
    Full,
    Partial { missing: Vec<&'static str> },  // non-empty by construction (constructor enforces)
    Empty,
}
impl ExifMetadata {
    pub fn completeness(&self) -> ExifCompleteness { ... }
}
```

- **`Partial { missing }` non-empty invariant**: `completeness()` returns
  `Empty` for all-fields-absent, `Full` for all-present, `Partial { missing }` ONLY when `missing` is non-empty. Constructor enforces.
- Routing to WARN messages per variant; distinct event tags
  (`exif-empty`, `exif-partial`).

##### 4c — `IngestStats` counter additions (per R2-T20 — canonical semantics table)

```rust
struct IngestStats {
    // ... existing fields (walked, ingested, superseded, already_catalogued,
    //     unknown_camera, no_exif, mtime_anomalous, skipped_non_raw,
    //     skipped_too_small, errored) ...

    // NEW per session 02:
    partial_exif: AtomicU64,        // ExifCompleteness::Partial fired
    cr3_exif_absent: AtomicU64,     // RawExifCause::ExifFieldsMissing fired
}
```

**Per-counter semantics table** (closes R2-T20):

| Counter | Trigger | WARN event | `--strict` rejects? | Catalog row consequence |
|---------|---------|------------|---------------------|-------------------------|
| `walked` | every file visited by walker | (none) | n/a | n/a |
| `ingested` | row inserted (new content) | (none) | n/a | row exists |
| `superseded` | content changed at known path | INFO | n/a | old row superseded; new row inserted |
| `already_catalogued` | re-ingest of same content | (none) | n/a | no-op |
| `unknown_camera` | `RawExifCause::UnsupportedFormat` OR Make/Model not in registry | WARN `event="unknown-camera"` | YES | row inserted with `camera_slug = NULL` |
| `no_exif` | **DEAD post-LibRaw** (kamadak-exif removed) — kept as label in summary_line for catalog-format stability; always reads 0 in v0.1; revived when JPEG ingest lands | — | (n/a; always 0) | n/a |
| `mtime_anomalous` | filesystem mtime outside [1995-01-01, 2100-01-01] | INFO | YES | row inserted with `mtime_anomalous = 1` |
| `skipped_non_raw` | walker found a non-RAW file | (none) | n/a | no row |
| `skipped_too_small` | file size 0 | INFO | YES | no row |
| `errored` | `RawExifCause::LibRawCallFailed` OR `RawExifCause::ExifMalformed` OR any other unhandled error | WARN per error | YES | NO row (error path) |
| **`partial_exif`** (NEW) | `ExifCompleteness::Partial` after LibRaw | WARN `event="exif-partial"` naming missing fields | YES | row inserted with partial NULLs |
| **`cr3_exif_absent`** (NEW) | `RawExifCause::ExifFieldsMissing` (LibRaw opened CR3 but EXIF box empty) | WARN `event="cr3-exif-fields-absent"` | YES | NO row (error path) |

##### 4d — `--strict` semantics (per PR1-T14 + R2-T20)

```rust
fn strict_fails(stats: &IngestStats) -> bool {
    stats.unknown_camera.load(Ordering::Relaxed) > 0
        || stats.mtime_anomalous.load(Ordering::Relaxed) > 0
        || stats.errored.load(Ordering::Relaxed) > 0
        || stats.skipped_too_small.load(Ordering::Relaxed) > 0
        || stats.partial_exif.load(Ordering::Relaxed) > 0
        || stats.cr3_exif_absent.load(Ordering::Relaxed) > 0
    // no_exif intentionally NOT in the predicate — dead post-LibRaw
}
```

Cross-ref R2-T12 in the gate's source comment.

##### 4e — `IngestOutcome` exhaustivity (per PR1-T37 → corrected ID = PR1-M17 per R2-T1; per R2-M6 payload simplification)

```rust
pub enum IngestOutcome {
    Inserted(PhotoId),
    SupersededPrevious(PhotoId),
    AlreadyCatalogued(PhotoId),
    InsertedWithPartialExif(PhotoId),  // R2-M6: no missing_fields payload (WARN already fired upstream)
    // ... existing variants
}
```

`apply_outcome` matches `InsertedWithPartialExif` and bumps the
`partial_exif` counter. Distinct from `Inserted` only in the counter
routing; the WARN message + missing-field detail live in the upstream
`parse_cr3_exif` point-of-decision per session-01 R3.T10's
"facts at the point of decision" principle.

##### 4f — `summary_line()` shape

Post-Deliverable 4c, summary line is (in production order):
```
walked: N, ingested: N, superseded: N, already-catalogued: N,
unknown-camera: N, no-exif: 0, partial_exif: N, cr3_exif_absent: N,
mtime-anomalous: N, skipped (non-RAW): N, skipped (too-small): N, errored: N
```

#### Deliverable 5 — `photohelper-catalog` touch + rusqlite bump

##### 5a — TD-002 rusqlite 0.32 → 0.40 bump (per PR1-T12 + R2-M12)

- Bump `rusqlite` workspace dep to 0.40 (or latest at implementation).
- **Enumerated API surface changes** between 0.32 and 0.40 touching the
  catalog call sites:
  - `Connection::open` / `open_with_flags` `OpenFlags` enum (specifically
    `SQLITE_OPEN_NO_MUTEX` default).
  - `TransactionBehavior::Immediate` signature stability.
  - `params!` macro variadic / type-coercion.
  - `execute_batch` PRAGMA-row handling.
  - `Error` variant additions (`match QueryReturnedNoRows` arm at
    `catalog.rs:321,343` MUST not silently dispatch to wildcard).
- **Bump-verification tests (R2-M12 — adds 3 sub-rows over v2)**:
  - `PRAGMA journal_mode = WAL` read-back returns `"wal"` post-init.
  - Roundtrip: open → write row → close → re-open → read row back.
  - `rusqlite::version_number() >= 3_045_000` (pins SQLite >= 3.45).
  - **NEW**: concurrent connections to same DB write concurrently → no deadlock, both rows present (pins `SQLITE_OPEN_NO_MUTEX` default).
  - **NEW**: `TransactionBehavior::Immediate` in connection A; concurrent `begin_immediate()` in connection B → expects `SQLITE_BUSY` (pins Immediate semantics).
  - **NEW**: `params![i64::MAX, i64::MIN]` round-trips without truncation (pins type coercion).

##### 5b — Schema column population (no shape change)

- `Catalog::upsert` inputs carry populated EXIF columns for CR3 rows.
  SQL unchanged. **No `PRAGMA user_version` bump** (DML not DDL).
- **NULL semantics shift (PR1-M14)**: pre-session-02 ingest left CR3
  columns NULL ("we didn't try"); post-session-02 populates them
  ("LibRaw extracted"). Catalogs ingested-pre-02 NOT backfilled by the
  session-02 binary.
- **Era-partitioning contract (per R2-M9)**: queries discriminating
  pre/post-02 ingest behavior MUST jointly filter on
  `ingested_at_unix_seconds >= <session-02-merge-timestamp> AND superseded_at_unix_seconds IS NULL`. The `SupersededPrevious` code
  path (mtime change → new PhotoId → old row marked superseded) creates
  ambiguous rows-per-path during era crossings; the conjunction handles
  it. Session 03's `ingest --reindex` is the canonical migration mechanism.

##### 5c — Decision-doc 0001 § History append

At session-end (NOT this commit), append `§ History` entry to
`docs/decisions/0001-catalog-schema-v1.md`:
> "**2026-MM-DD (session 02 merge)** — LibRaw landed in session 02; CR3
> EXIF columns now populated; schema shape unchanged; NULL semantics as
> described in plan v3 Deliverable 5b."

**Test plan row (per R2-M7; R3-T1 dropped phantom `R2-PT7`)**: session-end verifies the History
entry exists via grep on the decision doc.

#### Deliverable 6 — Test infrastructure (DN-008 subset + R2-T18 + R2-M8)

##### 6a — `poison_for_testing` knob

```rust
#[cfg(test)]
impl Catalog {
    pub(crate) fn poison_for_testing(&self) { ... }
}
```

- `#[cfg(test)]`-gated; **the `#[cfg(any(test, feature = "test-helpers"))]` escape hatch is EXPLICITLY REJECTED** (per R2-T18); workspace-level CI gate `! rg "cfg\\(any\\(test, feature" crates/` enforces.
- Three distinct tests (per PR1-T15 → corrected R2-T1 ID drift to PR1-T15
  sub-issue, was "PR1-T42"):
  - `poison_propagates_as_catalog_poisoned_error`
  - `poison_rollback_discards_panicked_workers_partial_insert`
  - `poison_recovery_admits_subsequent_inserts` (via drop-and-reopen)

##### 6b — R2-M8 silent ROLLBACK fix (per PR1-T32)

`catalog.rs:297` replaced with explicit match distinguishing
`"cannot rollback - no transaction is active"` from real errors;
unexpected errors logged with `op = "poison-recovery-rollback"`.

##### 6c — Heartbeat panic-for-testing (per R2-T3 — env-var, NOT `#[cfg(test)]`)

**Per R2-T3 (CRITICAL design hole)**: `#[cfg(test)]`-gated knob is
unreachable from subprocess integration tests. Switching to env-var
trigger matches the existing `PHOTOHELPER_HEARTBEAT_INTERVAL_MS` pattern
and works in release builds (the env-var is parsed on heartbeat-thread
startup; absent → normal behavior).

```rust
fn heartbeat_loop(stop_flag: Arc<AtomicBool>, granularity: Duration) {
    // Per R3-T3: env-var read gated on cfg!(debug_assertions) so release
    // builds cannot be DoS'd by accidental env-var export.
    let should_panic = cfg!(debug_assertions) && std::env::var("PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING")
        .map(|v| v == "1")
        .unwrap_or(false);
    // Note (per R3-M11): strict "1"-only contract; "true"/"yes"/"on"/etc
    // silently parse to false. Documented in rustdoc on heartbeat_loop.
    let mut tick = 0u32;
    loop {
        // ... existing tick logic ...
        if should_panic && tick == 0 {
            #[allow(clippy::panic, reason = "R3-T3 TD-005: env-var-triggered test affordance; debug_assertions gate prevents release DoS")]
            panic!("heartbeat death triggered by PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING");
        }
        tick += 1;
    }
}
```

R2-T18 4-of-4 closure verified via subprocess integration test:
`Command::cargo_bin("photohelper").env("PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING", "1").args(...).assert().success().stderr(contains("heartbeat-death-WARN"))`.

**Per R3-T7 (panic vs exit-code contract)**: the heartbeat thread panics; the parent process catches the panic via `JoinHandle::is_finished()` check at end-of-run (existing pattern from R1.T2); parent emits `WARN event="heartbeat-death-WARN"` to stderr and exits 0 (degraded-continue contract). Test assertion uses `.success()` because the parent process survives; substring assertion is on the PARENT-emitted WARN tag (not the panic-site message which may be lost depending on `panic = "unwind"` vs `panic = "abort"` profile config).

**Per R3-T2 sibling (env-var DoS guard against accidental export)**: the env-var is read only when `cfg!(debug_assertions)` is true — release builds compile out the env-var read entirely, so a contributor exporting `PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING=1` in production environments has no effect. Test environments use debug builds (default `cargo test`) which honor the env-var.

##### 6d — DN-008 row coverage (per PR1-T3)

**Covered this session (6 of 12)**: rows `{6, 17, 39, 42, 43, 49}`:
- Row 6: `trybuild` compile-fail for `assert_send_sync!(Arc<Catalog>)`.
- Row 17 (per PR1-T3 — restored): hardlink fixture → one row in `photos`; stderr contains `hardlink-dedup` INFO at `-v`; **second SELECT confirms PhotoId identical for both paths** (per R2-M8 — strengthens the dedup assertion).
- Row 39: `--strict` exit-code on CR3-only directory (post-LibRaw).
- Row 42: walker edge cases (mtime future, nested dirs, broken symlinks).
- Row 43: `mtime_anomalous` flag round-trip.
- Row 49: fatal exit codes (catalog locked, permission denied, disk full).

**Deferred to session 03+ (6 of 12)**: `{12, 13, 14, 18, 19, 34}` — see
§Out of scope row for per-row deferral rationale.

##### 6e — R2-T18 four WARN regression tests (per PR1-T4)

Per the §Test plan FFI error-path table; closes all 4 R1.T10 WARN paths:
1. `build_global already initialized` — run `ingest` twice in-process.
2. `wal_checkpoint recovered N frames` — kill + reopen catalog.
3. `file-lock` op-tag — parent-dir non-writable.
4. heartbeat death — via env-var knob from 6c.

Plus R2-T11 sibling `lock-file-create` op-tag (independent test row).

##### 6f — R2-T19 disposition

**Closed in session 01 R2 remediation** at
`crates/photohelper-core/src/model.rs:770` (commit `681a3a2`).
No session-02 action.

#### Deliverable 7 — DN-012 polish items (per PR1-T25 per-item enumeration)

**Folded into this session (binding trigger fires)**:
- `KnownCamera::Display` impl.
- `UpsertOutcome::#[non_exhaustive]`.
- Workspace clippy allow-list per-line rationale comments.

**Deferred**: Windows case-sensitivity walker filter — DN-012 binding
trigger updated at session-end to "next session that touches
`ingest.rs::WalkBuilder` filter OR by 2026-08-01."

### Out of scope (explicit deferrals)

| Item | Owner |
|------|-------|
| AI culling (`cull` subcommand real impl) — DN-005 partial advance via this session | session 03 |
| AI denoise (`develop` subcommand; model TBD pending session-04 plan-review) | session 04+ |
| XMP sidecar I/O (`crs:` / `ph:` namespaces) | session 04+ |
| JPEG export + watermarks (`export` subcommand) | session 05 |
| `cull-score` + `dup-group` tables + migration framework v1 → v2 — DN-005, decision-doc 0001 § Amendments | session 03 |
| Release-engineering wiring (musl static, codesign, Authenticode, winget, Homebrew tap, GitHub Release workflow, LGPL §6(a) tarball upload) — DN-001 release-engineering half | release session |
| `scripts/verify-review-artifact.sh` (bash port of fox's mjs enforcer) — DN-009 | future |
| GitHub Actions SHA pinning for non-`actions/checkout` actions — TD-001 | first external PR / release tag |
| Heartbeat-thread `.join()` cleanup — TD-003 | session 04 export pipeline OR 2026-08-01 |
| DN-008 rows {12, 13, 14, 18, 19, 34} | session 03+ |
| DN-012 Windows case-sensitivity walker filter — DN-012 (updated binding trigger) | next `WalkBuilder` touch OR 2026-08-01 |
| Windows LibRaw build + cross-compile audit — DN-013 | v0.2 cut OR first Windows-using contributor |
| Other RAW formats (CR2, NEF, ARW, RAF, ORF, RW2, DNG) — DN-014 | session adding second `CameraProfile` |
| LibRaw C-library CVE monitoring (osv-scanner wiring) — TD-004 | first `photohelper-raw` touch after 2026-08-01 OR any LibRaw CVE OR before first Release tag |
| Tile-based / streaming RAW decode (`read_raw_tile`, `read_raw_into`) | session 04 if memory pressure forces — file DN if surfaces |
| Non-Bayer sensor support (X-Trans, Foveon, monochrome) | session adding non-Bayer `CameraProfile` — file DN if surfaces |
| EXIF timezone-aware capture-time recovery — DN-016 | session 04+ if develop pipeline exposes time-zone-sensitive feature |
| WhiteBalance rebalance + per-illuminant ColorMatrix recovery — DN-017 | session 04+ develop pipeline |

### Acceptance criteria (Definition of Done)

1. **`just ci` green** locally and on GitHub Actions (all jobs).

2. **(2a — CI-verifiable)** `cargo test --workspace` against the
   Git-LFS-committed CR3 fixtures asserts (**conjunctive SQL** per
   R2-T17):
   ```sql
   SELECT COUNT(*) FROM photos
     WHERE make IS NOT NULL
       AND model IS NOT NULL
       AND camera_slug = 'canon-r8'
       AND capture_time_unix_seconds IS NOT NULL
       AND width > 0
       AND height > 0
   ```
   returns `= fixture_count`. AND `--strict` exit code = 0 on the fixture
   set. AND `stderr` contains EACH of (on separate `assert!` lines, per
   R2-T17): `no-exif: 0`, `partial_exif: 0`, `cr3_exif_absent: 0`,
   `errored: 0`.

   **(2b — manual smoke; recorded in PR description)** pre-merge, the
   author runs (with the clean-catalog precondition; **path-safety check
   per R2-M6**):
   ```bash
   [ -d "$HOME/Pictures/tests/.photohelper" ] \
     && rm -rf "$HOME/Pictures/tests/.photohelper" \
     || echo "no catalog to clean"
   photohelper ingest "$HOME/Pictures/tests" --strict
   ```
   Expected POST-LibRaw summary (per R2-M4 — extended with new counters):
   ```
   walked: 371, ingested: 370, superseded: 0, already-catalogued: 0,
   unknown-camera: 0, no-exif: 0, partial_exif: 0, cr3_exif_absent: 0,
   mtime-anomalous: 0, skipped (non-RAW): 1, skipped (too-small): 0,
   errored: 0
   ```
   (The 370 CR3s that previously yielded `no-exif: 370` per DN-011 now
   yield `ingested: 370`; total walked stays at 371 because the 1
   non-RAW file is filtered post-walk.) Author copy-pastes actual
   summary into PR body. **Not a CI gate**.

3. **`photohelper-raw::ffi` is the only crate with `unsafe` blocks**
   (enforced by lints, not by convention; per PR1-T21 + R2-T8):
   - Workspace `unsafe_code = "forbid"`; per-crate override at `ffi.rs`
     file head only; other `photohelper-raw/src/*.rs` files
     `#![forbid(unsafe_code)]`.
   - CI gate `! rg "unsafe\\s*\\{|unsafe\\s+fn" crates/photohelper-raw/src/ --glob '!ffi.rs'` runs from `just ci`.
   - `clippy::undocumented_unsafe_blocks = "deny"`.

4. **`cargo audit --deny warnings` clean** on bumped `rusqlite` + new
   LibRaw build inputs. **Caveat (per PR1-T10)**: `cargo audit` does NOT
   cover LibRaw C-library CVEs; TD-004 + DN-018 cover the gap.

5. **`docs/adr/0002-libraw-lgpl-static-link-mechanics.md` exists as
   DRAFT** (per R2-M2 + PR1-M18) with Status
   `"Accepted pending legal review before first GitHub Release tag"`.

6. **Zero CRITICAL findings open at session end** (closed inline OR
   filed as TD/DN with binding triggers per CLAUDE.md No-Acceptable-
   Trade-offs Policy). MEDIUM findings remediated before session end per
   `quality-assurance.md § Findings triage`. LOW ship with TD/DN OR
   accepted explicitly. HIGH carry-forward ≤ 2.

7. **LibRaw upstream pinned to exact `=0.22.1`** (per PR1-T10 + R2-T4;
   version escalated from `=0.21.4` at plan-v3.2 per Deliverable 0
   pre-flight; rationale in `docs/analysis/ANL-001-libraw-cr3-preflight.md
   § LibRaw version`):
   - `crates/photohelper-raw/vendor/libraw-0.22.1.tar.gz` exists.
   - `crates/photohelper-raw/vendor/libraw-0.22.1.tar.gz.sha256` exists.
   - `build.rs` verifies SHA-256 at build-time.
   - The exact `0.22.1` recorded in ADR-0002 and `Cargo.toml` metadata.

8. **No `*_for_testing` method appears in the release-build binary
   symbol table** (per PR1-T15 + R2-T18). Two-pronged CI gate runs from
   `just ci` (NOT a Rust lint — per R3-T11 correction; no rustc/clippy
   lint exists that pattern-matches `cfg(any(test, feature = ...))`):
   - **Symbol-table scan**: `scripts/check-no-test-helpers.sh` runs
     `nm target/release/photohelper` (Linux/macOS) OR
     `dumpbin /symbols target/release/photohelper.exe` (Windows) and
     fails if any symbol matches `*_for_testing`.
   - **Source-text grep gate**: `! rg "cfg\\(any\\(test, feature" crates/`
     fails if any crate uses the `cfg(any(test, feature = "..."))`
     escape-hatch pattern (closes at source-text level).

### Test plan

| Deliverable | Unit | Integration |
|-------------|------|-------------|
| **Deliverable 0 — Pre-flight** | n/a | Manual `docs/analysis/ANL-001`: per-file pass/fail for 371-CR3 set + CVE-posture-as-of-pin. ABORT trigger: >5% failure OR any open CVE on chosen LibRaw version. |
| **LibRaw build-system** | n/a | CI matrix `linux-x86_64` + `macos-arm64` builds clean. Static-link assertion (per R2-M10): `! nm -D target/release/photohelper 2>/dev/null \| grep -q ' U libraw_'` on Linux; `! otool -L target/release/photohelper \| grep -q 'libraw'` on macOS. Build.rs SHA-256 verification fires on tampered tarball. Missing-cmake error path: `build.rs` factored into `fn detect_cmake() -> Result<PathBuf, ToolchainError>` library function; unit-test with mocked `which`-style lookup (per R2-M10 — corrected from phantom `R2-PT3`). |
| **`photohelper-raw::ffi` path encoding** (PR1-T20) | NUL-byte interior → `Error::RawPath { reason: "interior-nul-byte" }`; NUL at first byte → same; non-UTF-8 path on Unix → typed error; emoji/CJK path on macOS APFS → `Ok`; symlink loop → `Error::RawPath`; Windows long path (>260 chars) → `\\?\`-prefixed `Ok`. **Plus happy-path: valid ASCII path → `Ok(RawPath)`** (R2-T4 boundary-pair coverage). | n/a |
| **`photohelper-raw::ffi` LibRaw error-path table** | n/a | Per `RawExifCause` variant: `chmod 000` → `LibRawCallFailed { op: "libraw_open_file", libraw_code }`; CR2 fixture → `UnsupportedFormat { libraw_make, libraw_model }` (closes R2-M11 wrong-format coverage; corrected from phantom `R2-PT8`); hex-edited CR3 with EXIF box zeros (via committed `tests/fixtures/cr3/gen_sad_path_fixtures.sh` per R2-T19) → `ExifFieldsMissing`; `ulimit -v` low + decode → `LibRawCallFailed { op: "libraw_unpack", libraw_code }` (resource exhaustion class); truncated CR3 → `LibRawCallFailed { op: "libraw_open_file" }`. |
| **`photohelper-raw::exif::read_cr3` field conversions** | LibRaw stub (synthesized `RawExifFields`): trailing-NUL trim on `make`/`model`; `time_t = 0` / `i64::MAX` boundary on timestamp; **each of orientation 0/1/2/.../8/9 round-trips through `ExifOrientation::from_tag`** (per R2-M11 — full domain coverage; corrected from phantom `R2-PT4`); `iwidth = 0` → `ExifMalformed { field: "width" }`; `iheight = 0` → `ExifMalformed { field: "height" }`; UTF-8-invalid bytes in `make` → typed error; UTF-8-invalid in `model` → typed error. | Real Canon R8 fixture: `make() == "Canon"`, `model() == "Canon EOS R8"` (or what LibRaw actually reports — recorded in ANL-001), `orientation() == ExifOrientation::Normal` (for known-orientation fixture), `capture_time_unix_seconds().is_some()`, `width().get() > 0`, `height().get() > 0`. |
| **`photohelper-raw::decode::read_raw` shape + invariants** | `BayerPlane::new` rejects `data.len() != w*h`; `BayerPlane::row(h)` returns `None` (OOB; per R2-T5); `BayerPlane::pixel(w, h)` returns `None`; **`SensorLevels::new` rejects `black >= white`, `white - black < 256`, `white > (1<<bit_depth)-1`** (per R2-T6 expansion); **`WhiteBalance::from_libraw_cam_mul([0.0, 0.0, 0.0, 0.0])` returns `Err(WhiteBalanceUnloaded)`; `[NaN, 1.0, 1.0, 1.0]` returns `Err(WhiteBalanceInvalid)`** (per R2-T6); **`CamRgbToXyzD65Matrix::from_libraw_rgb_cam(identity_3x3)` returns `Err(ColorMatrixUnloaded)`; matrix with NaN entry returns `Err(ColorMatrixInvalid)`** (per R2-T6); `CfaPattern` derivable from each of the 4 valid `cdesc[4]` patterns. | Real Canon R8 fixture: `pixel_count == width * height`; `pixels[0..1000]` not all zero / not all `u16::MAX`; `levels.black() < levels.white()`; `matches!(cfa_pattern, CfaPattern::Rggb)`; `RawImage` peak RSS < 300 MB per worker via `getrusage(RUSAGE_SELF).ru_maxrss` post-decode (per R2-T16). |
| **`From<RawExif> for ExifMetadata` conversion** | Field-by-field mapping unit test (in `photohelper-cli::commands::ingest`). | Covered by `parse_cr3_exif` integration tests. |
| **`ingest` rewire** | Mock-free: `parse_cr3_exif(real_cr3_path)` returns `Ok(metadata)` with `make = "Canon"`. `ExifCompleteness::completeness()` returns `Full`/`Partial { missing }`/`Empty` per fixture inputs. **`apply_outcome(InsertedWithPartialExif(...), &mut stats)` asserts `stats.partial_exif == 1`** (per R2-M6 — direct counter-wiring assertion; corrected from phantom `R2-PT2`). | **Happy path** (Acceptance 2a). **Sad paths (PR1-T29)**: `strict_mode_fails_on_unknown_camera_real_cr3` (CR3 with unrecognized Model — via committed `gen_sad_path_fixtures.sh`); `strict_mode_fails_on_libraw_error_real_cr3` (corrupted CR3); `strict_mode_fails_on_partial_exif_real_cr3` (hex-edited per `gen_sad_path_fixtures.sh`). Per R2-T19: fixture-generation script committed to repo so the sad-path edits are deterministic. |
| **Narrowed `RAW_EXTS = ["cr3"]`** | n/a | Mixed-content directory with CR3 + ARW + NEF: walker walks all 3, ingests only CR3, counts other 2 under `skipped (non-RAW)`. |
| **TD-002 rusqlite bump verification** | n/a | Per R2-M12 — 6 sub-tests: (1) PRAGMA WAL read-back; (2) roundtrip; (3) version_number ≥ 3_045_000; (4) concurrent connections no deadlock; (5) TransactionBehavior::Immediate rejects concurrent writes with SQLITE_BUSY; (6) params! type coercion roundtrip without truncation. |
| **`Catalog::poison_for_testing`** | Three tests (per PR1-T15): poison_propagates_as_catalog_poisoned_error; poison_rollback_discards_panicked_workers_partial_insert; poison_recovery_admits_subsequent_inserts (drop-and-reopen — poison is permanent). | n/a |
| **R2-M8 silent ROLLBACK fix** | n/a | Poison + simulated disk-full during ROLLBACK → WARN with `op = "poison-recovery-rollback"` substring. |
| **Heartbeat panic-for-testing env-var (per R2-T3)** | n/a | `Command::cargo_bin("photohelper").env("PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING", "1").args(["ingest", fixture_dir]).assert().success().stderr(contains("heartbeat death triggered"))`. Concrete substring assertion (per R2-T10 — replaces vague "heartbeat-death-WARN substring"). |
| **R2-T18 paths 1-3 + R2-T11 sibling** | n/a | Per the FFI error-path table above PLUS: `ingest` twice in-process → `build_global already initialized` WARN; kill + reopen catalog → `wal_checkpoint recovered N frames` WARN; parent-dir read-only → BOTH `file-lock` AND `lock-file-create` op-tag WARNs. |
| **DN-008 rows** | Per Deliverable 6d enumeration. | Row 17 (hardlink): `ingest` writes ONE row; stderr contains `hardlink-dedup` INFO at `-v`; **second SELECT confirms identical PhotoId for both paths** (per R2-M8). |
| **Git LFS fixture sanity** (PR1-T13) | `fixture_is_real_cr3` helper unit: synthesized LFS pointer → `Err`; 1MB+ binary not starting with pointer → `Ok`. | All real-CR3 tests call `fixture_is_real_cr3` at top; failure ⇒ panic with actionable message. |
| **Fixture EXIF sanitization (PR1-T11 + R2-T9)** | `tests/fixtures/sanitize-check.sh` allow-list lint: `exiftool -G -a -ee` on every `*.cr3` MUST contain ONLY the asserted-survivor tag set; any other tag → CI fails. Embedded-preview check via `exiftool -ee -G -a` MUST also be in the survivor set; NO preview-IFD GPS/owner. | n/a |
| **Decision-doc 0001 § History entry** (per R2-M7) | n/a | Session-end verifies `grep -E '## (History|Amendments)' docs/decisions/0001-catalog-schema-v1.md` matches AND grep for `LibRaw landed in session 02` substring. (Note per R3-M3: §5c appends to `§ Amendments` since that section already exists from the v1→v2 reschedule cross-doc commit; not a new `§ History` section.) |

### Checkpoints firing this session (Cadence A)

Mandatory per Cadence A § Review cadence: plan-review at session start
(firing now); session-end at session close.

Session-02-specific sub-component reviews (Tier 4, 3-5 agents):

| Checkpoint | When | Artifact |
|------------|------|----------|
| Sub-component — `photohelper-raw::ffi` | When `ffi.rs` first exposes a non-scaffold public API | `docs/code-reviews/session-02-photohelper-raw-ffi-round{1,2}.md` |
| Sub-component — LibRaw build-system / LGPL | When `build.rs` + ADR-0002 land | `docs/code-reviews/session-02-libraw-build-round{1,2}.md` |

### Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| LibRaw fails on the user's R8 firmware revision | low | Deliverable 0 pre-flight runs BEFORE FFI wiring; ABORT trigger at >5% failure. |
| Hand-rolled FFI shim grows beyond 20 functions in implementation | medium | Re-evaluation trigger raised to >20 (per R2-T14 accounting for C-API accessors); escalate to plan-review v4 if hit; consider `libraw-sys` adoption. |
| LibRaw vendored-source build (cmake) hits a compiler-flag landmine on macOS arm64 | medium | Switch to `pkg-config` against system LibRaw; defer Windows to v0.2 explicitly (DN-013). |
| LibRaw vendored-source build fails on contributor machine | medium | `build.rs` emits actionable `cargo:warning=` lines; `README.md` documents the prerequisite list. |
| `rusqlite 0.40` introduces unforeseen API surface needing migration | low | TD-002 declares 0.40 API-compatible for ops we use; Deliverable 5a enumerated API surface changes verified pre-bump. Fallback to `rusqlite 0.3X` intermediate ONLY with concurrent TD-002 amendment. |
| Git LFS bandwidth quota hit on free tier | low | Initial fixture set ≤30 MB; revisit if total > 500 MB. |
| Windows LibRaw cross-compile blocked (cluster) | medium | DN-013 bounds scope to v0.2. If two or more LibRaw risks fire concurrently, escalate to a dedicated session for LibRaw FFI alone. |
| LibRaw C-library CVE disclosed during session 02 implementation | low | TD-004 (ongoing) + DN-018 (pre-flight check); pre-merge holds if a CVE lands during implementation. |

### Cross-references

**Verb taxonomy (4 verbs; per PR1-L12 + R2-T10)**:
- **closed** = binding trigger satisfied; DN/TD/finding fully resolved this session.
- **partial** = DN/TD partially advanced; remainder rolls forward with explicit binding trigger.
- **unchanged** = no action this session; cited for audit-trail completeness.
- **filed** = new DN/TD entry created this session as part of a cross-doc commit (with binding trigger).

| ID | Disposition | Note |
|----|-------------|------|
| DN-001 (LibRaw LGPL §6(a)) | partial | ADR-0002 + build mechanism THIS session; release workflow wiring deferred. |
| DN-005 (catalog schema) | partial | v1 CR3 columns populated; schema shape unchanged; v1→v2 migration framework rescheduled to session 03 (decision-doc 0001 § Amendments). |
| DN-006 (kamadak-exif CR3 failure) | closed | `parse_cr3_exif` dispatches to LibRaw; kamadak-exif removed atomically (Deliverable 4a); verified by Acceptance 2a + 2b. |
| DN-007 (rusqlite stale) | closed | DN-007's Owner per `discovery-notes.md:86` is TD-002; closing TD-002 IS DN-007 closure. Status update at session-end. |
| DN-008 (test infrastructure) | partial | 6 of 12 rows landed (`{6, 17, 39, 42, 43, 49}` per Deliverable 6d); 6 deferred (`{12, 13, 14, 18, 19, 34}`). |
| DN-009 (verify-review-artifact.sh) | unchanged | Future session; binding trigger 2026-09-01 OR before first review artifact post-this-session. |
| DN-011 (DN-006 production trace) | closed | Alongside DN-006; pre-flight verifies LibRaw extracts for 371-CR3 set BEFORE wiring. |
| DN-012 (T15 polish items) | partial | 3 of 4 folded in; Windows case-sensitivity walker deferred with updated trigger. |
| DN-013 (Windows LibRaw cross-compile) | filed | Per cross-doc commit; trigger v0.2 cut OR first Windows-using contributor. |
| DN-014 (Other RAW formats) | filed | Per cross-doc commit; trigger first session adding non-Canon `CameraProfile`. |
| DN-015 (heartbeat panic_for_testing vs TD-003 distinction) | filed | Informational. |
| DN-016 (EXIF timezone recovery) | filed | Per R2-T2 cross-doc remediation; trigger session 04+ if develop pipeline exposes time-zone-sensitive feature. |
| DN-017 (WhiteBalance rebalance / per-illuminant ColorMatrix) | filed | Per R2-T2 cross-doc remediation; trigger session 04+ develop pipeline. |
| DN-018 (LibRaw CVE-posture-as-of-pin audit owner) | filed | Per R2-T4 cross-doc remediation; trigger Deliverable 0 pre-flight. |
| TD-001 (GH Actions SHA pinning) | unchanged | `actions/checkout` SHA-pin lands incidentally for Deliverable 3 LFS work; does NOT close TD-001 (other 2 actions still `@vN` floating; binding trigger 'external PR / release tag' unfired). Per R2-T11. |
| TD-002 (rusqlite stale) | closed | Bumped voluntarily ahead of calendar trigger 2026-08-01; structural trigger NOT fired (no new columns); bundled with LibRaw work. |
| TD-003 (heartbeat join) | unchanged | All 3 trigger clauses unfired: (a) not touching `run_ingest`'s post-walk teardown; (b) 2026-08-01 not yet expired; (c) no test-flake from stderr-ordering observed. |
| TD-004 (LibRaw CVE monitoring) | filed | Per cross-doc commit; trigger first `photohelper-raw` touch after 2026-08-01 OR any LibRaw CVE OR before first GitHub Release. |
| R2-T18 (4 R1.T10 WARN regression tests) | closed | All 4 paths via env-var `PHOTOHELPER_HEARTBEAT_PANIC_FOR_TESTING` (Deliverable 6c) + Deliverable 6e tests. |
| R2-T19 (128KB PhotoId test) | closed in session 01 | Test at `model.rs:770` (commit `681a3a2`). |
| R2-T22 / R2-T23 (R1 count drifts) | unchanged | Cosmetic; not blocking. |
| R2-M8 (silent ROLLBACK) | closed | Per Deliverable 6b. |
| R2-T26 → MAPPED TO R2-T8 (`unused_crate_dependencies` lint addition is bullet 6 of R2-T8's atomic 7-file commit shape) | closed | Per Deliverable 4a §4a items 6 + 7a (atomic with kamadak-exif removal AND atomic with photohelper-core/Cargo.toml trybuild dep removal — closes R3-T4 coordination). |

### Commit-scope convention (per PR1-T31)

This session uses `<type>(session-02): ...` for all commits to match
session-01's pattern. EXCEPTIONS noted inline:
- Deliverable 0 pre-flight: `chore(libraw): pre-flight EXIF + CVE-posture audit (Deliverable 0)` — component-scoped because the pre-flight artifact is cross-session-portable infrastructure.

Per-component scopes (e.g. `feat(photohelper-raw):`) require an ADR to
change project-wide convention; not adopted this session.

### Plan revisions log

- **v1 (2026-05-28)**: initial; pre plan-review.
- **v2 (2026-05-28)**: post plan-review Round 1. Addresses 16 CRITICAL + 17 HIGH + most MEDIUM inline; see `docs/code-reviews/session-02-plan-round1.md` for full findings.
- **v3 (2026-05-28)**: post plan-review Round 2. Addresses 9 CRITICAL + 14 HIGH + most MEDIUM; see `docs/code-reviews/session-02-plan-round2.md`. Key changes:
  - Phantom PR1-T# IDs corrected throughout (R2-T1); v2 invented PR1-T34/T35/T36/T37/T42/T44/T45/PR1-AU which don't exist in R1.
  - DN-016 / DN-017 / DN-018 filed in cross-doc commit (R2-T2 phantom DN closed).
  - LibRaw FFI strategy switched from `#[repr(C)]` mirrors to C-API accessors (R2-T14); function set bound to ~15 via `libraw_get_*` calls.
  - `libraw_open_wfile` symbol correction (R2-T15; was fabricated as `libraw_open_file_w`).
  - LibRaw version pinned to `=0.21.4` with Deliverable-0 verification step (R2-T4).
  - `BayerPlane::row(y) -> Option<&[u16]>` + `pixel(x,y) -> Option<u16>` (R2-T5; was infallible-panic-on-OOB).
  - `WhiteBalance` + `CamRgbToXyzD65Matrix` rewritten as proper newtypes with private fields + fallible constructors + NaN/negative/identity rejection (R2-T6).
  - `Error::Exif` slot RECYCLED for the new LibRaw boundary (R2-T7); constructor syntax fixed (`Box::new(e)` direct, no `BoxedSourceError(...)` wrap — it's a type alias).
  - `panic_for_testing` heartbeat hook switched from `#[cfg(test)]` to env-var (R2-T3); subprocess integration tests now actually exercise the path.
  - Sanitize-check.sh rewritten as allow-list (R2-T9) — closes PII gap (LensSerialNumber / IPTC creators / embedded preview thumbnails now caught).
  - `RAW_EXTS` narrowing atomic commit shape pinned (R2-T8): all 7 file changes in one commit.
  - `RawExifCause::LibRawCallFailed { libraw_code, op }` carries op tag (R2-T13); `RawDecodeCause` simplified per R2-T12 path (b) plus typed WB/ColorMatrix sub-variants.
  - `SensorLevels` invariant tightened: dynamic-range floor 256, bit-depth check (R2-T6 expansion).
  - Memory pressure SLO corrected to 150-250 MB per worker / 1.2-2 GB transient (R2-T16; was 50 MB / 800 MB).
  - Verb taxonomy expanded to 4 verbs (closed/partial/unchanged/filed) per R2-T10.
  - TD-001 reclassified to `unchanged` (R2-T11; the partial-progress framing was wrong for an all-or-nothing TD).
  - Per-counter semantics table added (R2-T20) — definitive `IngestStats` field/trigger/strict-contribution map.
  - Acceptance 2a SQL conjunction tightened (R2-T17); Acceptance 2b summary line extended with new counters (R2-M4) + path-safety check (R2-M6).
  - Acceptance 8 added: no `*_for_testing` symbols in release binary; `cfg(any(test, feature))` workspace lint forbids escape hatch (R2-T18 + PR1-T15 closure).
  - Decision-doc 0002 moved to `docs/adr/` (R2-M2; binding for every release).
  - Plan-revisions log trimmed (R2-T21); §"Plan-review decisions resolved at Round 1" deleted (R2-T22).
  - Commit-scope convention section added (PR1-T31 / R2-T23).
  - SCUNet residue scrubbed from README.md + HANDOFF_REPORT.md (R2-M5; in cross-doc commit).
  - Sad-path fixture construction protocol pinned via `tests/fixtures/cr3/gen_sad_path_fixtures.sh` (R2-T19).
  - Static-link CI assertion predicate spelled out (R2-M10).
  - `ExifMalformed` test coverage expanded to orientation 0/9, height 0, UTF-8-invalid model (R2-M11).
  - Rusqlite test sub-rows expanded to 6 (R2-M12).
  - Era-partitioning predicate documented (R2-M9).
  - DN-008 row 17 dedup assertion strengthened (R2-M8).
  - LibRaw build-system `detect_cmake` library factoring + unit test (R2-M10 — corrected from R3-T1 phantom `R2-PT3`).
  - `IngestOutcome::InsertedWithPartialExif` payload simplified to `PhotoId` only (R2-M6).
  - Hardlink dedup test asserts PhotoId equality (R2-M8).
- **v3.1 (2026-05-28)**: post plan-review Round 3 (targeted remediation). Plan-v3 phantom-ID drift (R3-T1: R2-S2, R2-T26, R2-PT2..8) corrected; design CRITICALs that the agents flagged inline (R3-T3 heartbeat env-var DoS guard; R3-T5 SensorBitDepth ctor delegation; R3-T7 panic-vs-exit-code contract; R3-T8 sanitize-check preview descent; R3-T11 Acceptance 8 wording) folded inline; remaining design-class CRITICALs (R3-T6 RawDecodeCause dispatch; R3-T10 PathBuf empty-path) filed as TD-005/006/007 with binding triggers; R4 NOT fired per agent consensus.
- **v3.2 (2026-05-28)**: post Deliverable 0 pre-flight (`docs/analysis/ANL-001-libraw-cr3-preflight.md`). LibRaw version escalated from `=0.21.4` to `=0.22.1` because LibRaw 0.22.1 ships six TALOS-2026 fixes + two CR3-parser hardenings ("zero all buffers before fread", 64-bit unsigned file offsets) that did NOT backport to 0.21.5b — the 0.21.x branch is effectively EOL post-2025-12-25. Choice exceeded the plan's implementer-granted authority (which is limited to picking the latest 0.21.x); user consultation under No-Acceptable-Trade-offs Policy approved the escalation. Pre-flight EXIF extraction was 370/370 (100%); CVE-posture (MITRE NVD + LibRaw GHSA) clean for both candidates. DN-018 status flipped to closed (Deliverable 0 owner satisfied).
