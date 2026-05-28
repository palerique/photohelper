//! Domain types: `PhotoId`, `AbsPath`, `Photo`, `CameraId`, `KnownCamera`,
//! `ExifOrientation`, `Aspect`, `ExifMetadata`, `IngestOutcome`.
//!
//! All field-bearing types use private fields with constructor-validated
//! invariants. See `docs/plans/session-01.md` §Deliverables 2.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::engine::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::error::Error;

// =====================================================================
// PhotoId — content-derived 32-byte identifier
// =====================================================================

/// PhotoId-derivation hash window (head and tail). Closes the spec at
/// `docs/plans/session-01.md` §PhotoId derivation.
const HASH_WINDOW_BYTES: usize = 64 * 1024;

/// Lower clamp boundary for mtime (1995-01-01 UTC, Unix seconds).
pub const MTIME_LOWER_BOUND_UNIX_SECONDS: i64 = 788_918_400;

/// Upper clamp boundary for mtime (2100-01-01 UTC, Unix seconds). Static
/// (not `now() + 1 day`) so PhotoId derivation is run-independent.
pub const MTIME_UPPER_BOUND_UNIX_SECONDS: i64 = 4_102_444_800;

/// Content-addressed photo identifier.
///
/// 32 raw bytes derived from
/// `BLAKE3(file_size.to_le_bytes() || clamped_mtime.to_le_bytes() || first_64KB || last_64KB)`.
///
/// `Display` renders 43-char URL-safe base64 (no padding).
///
/// Constructors:
/// - [`PhotoId::derive`] — the canonical filesystem-derived constructor.
/// - [`crate::catalog_glue::photo_id_from_row_bytes`] — the only public path
///   for reconstructing a `PhotoId` from raw catalog bytes. Uses
///   `pub(crate)` access internally; outside callers must go through that
///   function's named-after-purpose API.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhotoId([u8; 32]);

impl PhotoId {
    /// Canonical filesystem derivation.
    ///
    /// # Errors
    /// - `Error::Io` for filesystem errors (`stat`, `read-prefix`).
    /// - `Error::HashWindowTooSmall` if `file_size == 0`.
    pub fn derive(path: &Path) -> Result<Self, Error> {
        let metadata = std::fs::metadata(path).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            op: "stat",
            source: e,
        })?;
        let file_size = metadata.len();
        if file_size == 0 {
            return Err(Error::HashWindowTooSmall {
                path: path.to_path_buf(),
                len: 0,
            });
        }
        let mtime_unix_seconds: i64 = match metadata.modified() {
            Ok(systime) => systime
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| {
                    i64::try_from(d.as_secs()).unwrap_or(MTIME_UPPER_BOUND_UNIX_SECONDS)
                }),
            Err(_) => 0,
        };
        let clamped = clamp_mtime(mtime_unix_seconds).0;
        Self::derive_with_clamped_mtime(path, file_size, clamped)
    }

    /// Derive with an explicit pre-clamped mtime (used by `ingest_one` so
    /// the same clamped value feeds both the hash and the catalog row).
    ///
    /// # Errors
    /// - `Error::Io` reading the file prefix/suffix.
    /// - `Error::HashWindowTooSmall` if `file_size == 0`.
    pub fn derive_with_clamped_mtime(
        path: &Path,
        file_size: u64,
        clamped_mtime_unix_seconds: i64,
    ) -> Result<Self, Error> {
        if file_size == 0 {
            return Err(Error::HashWindowTooSmall {
                path: path.to_path_buf(),
                len: 0,
            });
        }

        // R1.T3 fix: head + tail are DISJOINT. Before the fix, a 100KB
        // file would hash [0..64KB) plus [36KB..100KB) — [36KB..64KB)
        // was hashed twice (28KB of overlap), violating the "first 64KB
        // + last 64KB" spec. Tail starts at `max(file_size - 64KB,
        // head_end)`. For files ≤ 128KB the tail is whatever remains
        // after head; for > 128KB the tail is exactly the last 64KB.
        let head_len =
            usize::try_from(file_size.min(HASH_WINDOW_BYTES as u64)).unwrap_or(HASH_WINDOW_BYTES);
        let head_end_u64 = head_len as u64;
        let tail_start = file_size
            .saturating_sub(HASH_WINDOW_BYTES as u64)
            .max(head_end_u64);
        let tail_len_u64 = file_size.saturating_sub(tail_start);
        let tail_len = usize::try_from(tail_len_u64).unwrap_or(HASH_WINDOW_BYTES);

        let mut file = File::open(path).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            op: "read-prefix",
            source: e,
        })?;
        let mut head = vec![0u8; head_len];
        read_exact_n(&mut file, &mut head, path)?;

        let tail = if tail_len > 0 {
            use std::io::{Seek, SeekFrom};
            let tail_offset = i64::try_from(tail_len_u64).unwrap_or(i64::MAX);
            file.seek(SeekFrom::End(-tail_offset))
                .map_err(|e| Error::Io {
                    path: path.to_path_buf(),
                    op: "read-prefix",
                    source: e,
                })?;
            let mut tail_buf = vec![0u8; tail_len];
            read_exact_n(&mut file, &mut tail_buf, path)?;
            tail_buf
        } else {
            Vec::new()
        };

        Ok(Self::hash_parts(
            file_size,
            clamped_mtime_unix_seconds,
            &head,
            &tail,
        ))
    }

    /// Hash `(file_size_le || clamped_mtime_le || head || tail)` with BLAKE3.
    /// Infallible; returns the `PhotoId` directly.
    fn hash_parts(
        file_size: u64,
        clamped_mtime_unix_seconds: i64,
        head: &[u8],
        tail: &[u8],
    ) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&file_size.to_le_bytes());
        hasher.update(&clamped_mtime_unix_seconds.to_le_bytes());
        hasher.update(head);
        if !tail.is_empty() {
            hasher.update(tail);
        }
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(digest.as_bytes());
        Self(bytes)
    }

    /// Catalog-only reconstruction. `pub(crate)` so only `photohelper-core`
    /// itself can call it; outside callers go through
    /// [`crate::catalog_glue::photo_id_from_row_bytes`].
    pub(crate) fn from_db_bytes(raw: [u8; 32]) -> Self {
        Self(raw)
    }

    /// Raw bytes for catalog persistence.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

fn read_exact_n(file: &mut File, buf: &mut [u8], path: &Path) -> Result<(), Error> {
    file.read_exact(buf).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        op: "read-prefix",
        source: e,
    })
}

impl fmt::Debug for PhotoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhotoId({self})")
    }
}

impl fmt::Display for PhotoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

// =====================================================================
// mtime clamp
// =====================================================================

/// Result of clamping an mtime to the allowed range. Tuple is `(clamped,
/// was_clamped)`.
#[must_use]
pub fn clamp_mtime(mtime_unix_seconds: i64) -> (i64, bool) {
    if mtime_unix_seconds < MTIME_LOWER_BOUND_UNIX_SECONDS {
        (MTIME_LOWER_BOUND_UNIX_SECONDS, true)
    } else if mtime_unix_seconds > MTIME_UPPER_BOUND_UNIX_SECONDS {
        (MTIME_UPPER_BOUND_UNIX_SECONDS, true)
    } else {
        (mtime_unix_seconds, false)
    }
}

// =====================================================================
// AbsPath — canonical absolute path with NUL-byte + escape rejection
// =====================================================================

/// Canonical absolute filesystem path.
///
/// Constructors:
/// - [`AbsPath::canonicalize`] — accept any path; rejects NUL bytes; runs
///   `std::fs::canonicalize`.
/// - [`AbsPath::canonicalize_within`] — same plus requires the canonical
///   form to be under `root` (closes path-traversal escape).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AbsPath(PathBuf);

impl AbsPath {
    /// Canonicalize any path, rejecting NUL bytes and non-existent paths.
    ///
    /// # Errors
    /// - `Error::Io { op: "canonicalize-nul-check" }` if path bytes contain NUL.
    /// - `Error::Io { op: "canonicalize" }` for filesystem failures.
    pub fn canonicalize(path: impl AsRef<Path>) -> Result<Self, Error> {
        let p = path.as_ref();
        if let Some(s) = p.to_str() {
            if s.contains('\0') {
                return Err(Error::Io {
                    path: p.to_path_buf(),
                    op: "canonicalize-nul-check",
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "path contains NUL byte",
                    ),
                });
            }
        }
        let canonical = std::fs::canonicalize(p).map_err(|e| Error::Io {
            path: p.to_path_buf(),
            op: "canonicalize",
            source: e,
        })?;
        Ok(Self(canonical))
    }

    /// Same as [`Self::canonicalize`] plus rejects paths whose canonical
    /// form escapes `root`.
    ///
    /// # Errors
    /// - Anything from [`Self::canonicalize`].
    /// - `Error::PathEscapesRoot` if the canonical form is not under `root`.
    pub fn canonicalize_within(root: &AbsPath, path: impl AsRef<Path>) -> Result<Self, Error> {
        let canonical = Self::canonicalize(path.as_ref())?;
        if !canonical.0.starts_with(root.as_path()) {
            return Err(Error::PathEscapesRoot {
                path: canonical.0,
                root: root.0.clone(),
            });
        }
        Ok(canonical)
    }

    /// Borrow as `&Path`.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for AbsPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

// =====================================================================
// Camera identity
// =====================================================================

/// Whether a photo's camera was recognized by `CameraRegistry`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CameraId {
    /// Recognized body (has a `CameraProfile` registered).
    Known(KnownCamera),
    /// Unrecognized body; raw EXIF strings preserved.
    Unknown {
        /// Raw EXIF Make.
        make: String,
        /// Raw EXIF Model.
        model: String,
    },
}

impl fmt::Display for CameraId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known(k) => write!(f, "Known({})", k.slug()),
            Self::Unknown { make, model } => {
                write!(f, "Unknown(make={make:?}, model={model:?})")
            }
        }
    }
}

/// The closed set of recognized camera bodies. `#[non_exhaustive]` so
/// adding a new body in session 02+ is non-breaking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KnownCamera {
    /// Canon EOS R8 (full-frame mirrorless, RF mount, 24MP).
    CanonR8,
}

impl KnownCamera {
    /// Stable string identifier used in `photos.camera_slug`.
    #[must_use]
    pub fn slug(&self) -> &'static str {
        match self {
            Self::CanonR8 => "canon-r8",
        }
    }

    /// Parse from a slug; returns `None` for unknown values.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "canon-r8" => Some(Self::CanonR8),
            _ => None,
        }
    }

    /// Human-readable model name (for log lines / progress messages).
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::CanonR8 => "Canon EOS R8",
        }
    }
}

impl std::fmt::Display for KnownCamera {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

// =====================================================================
// ExifOrientation — EXIF tag values 1..=8
// =====================================================================

/// EXIF orientation tag (canonical names per EXIF spec).
///
/// Mapping is the standard EXIF tag 1..=8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExifOrientation {
    /// Tag 1: no transform.
    Normal,
    /// Tag 2: horizontal flip.
    MirrorHorizontal,
    /// Tag 3: rotate 180°.
    Rotate180,
    /// Tag 4: vertical flip.
    MirrorVertical,
    /// Tag 5: transpose (horizontal flip + 90° CW rotation).
    Transpose,
    /// Tag 6: rotate 90° clockwise.
    Rotate90Cw,
    /// Tag 7: transverse (horizontal flip + 90° CCW rotation).
    Transverse,
    /// Tag 8: rotate 90° counter-clockwise.
    Rotate90Ccw,
}

impl ExifOrientation {
    /// Parse EXIF tag value 1..=8.
    ///
    /// # Errors
    /// - `Error::InvalidExifOrientationTag { tag }` for values outside 1..=8.
    ///   (R2-T9: rustdoc previously claimed `Error::Exif`, but R1.T11
    ///   replaced the empty-PathBuf-sentinel `Error::Exif` with the
    ///   dedicated path-free variant.)
    pub fn from_tag(tag: i64) -> Result<Self, Error> {
        match tag {
            1 => Ok(Self::Normal),
            2 => Ok(Self::MirrorHorizontal),
            3 => Ok(Self::Rotate180),
            4 => Ok(Self::MirrorVertical),
            5 => Ok(Self::Transpose),
            6 => Ok(Self::Rotate90Cw),
            7 => Ok(Self::Transverse),
            8 => Ok(Self::Rotate90Ccw),
            other => Err(Error::InvalidExifOrientationTag { tag: other }),
        }
    }

    /// Render as EXIF tag value 1..=8.
    #[must_use]
    pub fn to_tag(&self) -> i64 {
        match self {
            Self::Normal => 1,
            Self::MirrorHorizontal => 2,
            Self::Rotate180 => 3,
            Self::MirrorVertical => 4,
            Self::Transpose => 5,
            Self::Rotate90Cw => 6,
            Self::Transverse => 7,
            Self::Rotate90Ccw => 8,
        }
    }
}

// =====================================================================
// Aspect — high-level orientation classification
// =====================================================================

/// High-level photo orientation derived from `(width, height, orientation)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Aspect {
    /// Wider than tall.
    Landscape,
    /// Taller than wide.
    Portrait,
    /// Equal sides.
    Square,
}

// =====================================================================
// ExifMetadata — parsed EXIF as a small struct
// =====================================================================

/// EXIF fields photohelper cares about. All `Option` because real-world
/// files (and unrecognized containers) may omit any of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExifMetadata {
    /// Camera make (raw EXIF string).
    pub make: Option<String>,
    /// Camera model (raw EXIF string).
    pub model: Option<String>,
    /// Capture time as Unix seconds.
    pub capture_time_unix_seconds: Option<i64>,
    /// Image width in pixels.
    pub width: Option<u32>,
    /// Image height in pixels.
    pub height: Option<u32>,
    /// EXIF orientation tag.
    pub orientation: Option<ExifOrientation>,
}

impl ExifMetadata {
    /// True iff every field is `None`. `ingest_one` reads this to bump the
    /// `no_exif` `IngestStats` counter at the point of decision, then proceeds
    /// to `catalog.upsert` with NULL EXIF columns per the DN-006 fallback.
    /// (Prior to R2-T2 this docstring claimed the empty-EXIF case routed to
    /// `IngestOutcome::NoExifFields`; that variant was deleted as dead code.)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.make.is_none()
            && self.model.is_none()
            && self.capture_time_unix_seconds.is_none()
            && self.width.is_none()
            && self.height.is_none()
            && self.orientation.is_none()
    }
}

// =====================================================================
// Photo — fields private, fallible constructor
// =====================================================================

/// A photo identified by its canonical path + content-derived `PhotoId`.
///
/// Constructor enforces `file_size > 0` and that `source_path` is the
/// canonical absolute form (already validated by `AbsPath`).
#[derive(Clone, Debug)]
pub struct Photo {
    photo_id: PhotoId,
    source_path: AbsPath,
    file_size: u64,
    clamped_mtime_unix_seconds: i64,
    mtime_anomalous: bool,
    camera_id: Option<CameraId>,
    exif: ExifMetadata,
}

impl Photo {
    /// Construct a `Photo` from already-validated filesystem facts.
    ///
    /// # Errors
    /// - `Error::Io { op: "stat" }` if `file_size == 0` (degenerate).
    pub fn from_filesystem(
        canonical: AbsPath,
        file_size: u64,
        clamped_mtime_unix_seconds: i64,
        mtime_anomalous: bool,
        photo_id: PhotoId,
        camera_id: Option<CameraId>,
        exif: ExifMetadata,
    ) -> Result<Self, Error> {
        if file_size == 0 {
            return Err(Error::Io {
                path: canonical.0.clone(),
                op: "stat",
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "file_size must be > 0",
                ),
            });
        }
        Ok(Self {
            photo_id,
            source_path: canonical,
            file_size,
            clamped_mtime_unix_seconds,
            mtime_anomalous,
            camera_id,
            exif,
        })
    }

    /// Content-derived identifier.
    #[must_use]
    pub fn photo_id(&self) -> PhotoId {
        self.photo_id
    }

    /// Canonical source path.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        self.source_path.as_path()
    }

    /// File size in bytes.
    #[must_use]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Clamped mtime in Unix seconds (also fed into the hash).
    #[must_use]
    pub fn clamped_mtime_unix_seconds(&self) -> i64 {
        self.clamped_mtime_unix_seconds
    }

    /// Whether the original mtime was clamped to the allowed range.
    #[must_use]
    pub fn mtime_anomalous(&self) -> bool {
        self.mtime_anomalous
    }

    /// Camera identity (None if EXIF lookup failed entirely).
    #[must_use]
    pub fn camera_id(&self) -> Option<&CameraId> {
        self.camera_id.as_ref()
    }

    /// Borrow the parsed EXIF metadata.
    #[must_use]
    pub fn exif(&self) -> &ExifMetadata {
        &self.exif
    }

    /// Derive the high-level aspect (Landscape / Portrait / Square) from
    /// the EXIF dimensions + orientation. Returns `None` if either width
    /// or height is missing.
    #[must_use]
    pub fn aspect(&self) -> Option<Aspect> {
        let (w, h) = (self.exif.width?, self.exif.height?);
        // EXIF orientations 5..=8 swap the visual width/height.
        let (vw, vh) = match self.exif.orientation {
            Some(
                ExifOrientation::Transpose
                | ExifOrientation::Rotate90Cw
                | ExifOrientation::Transverse
                | ExifOrientation::Rotate90Ccw,
            ) => (h, w),
            _ => (w, h),
        };
        Some(match vw.cmp(&vh) {
            std::cmp::Ordering::Greater => Aspect::Landscape,
            std::cmp::Ordering::Less => Aspect::Portrait,
            std::cmp::Ordering::Equal => Aspect::Square,
        })
    }
}

// =====================================================================
// IngestOutcome — per-file ingest result
// =====================================================================

/// What happened to a single file during ingest.
///
/// Boolean tally signals (`camera_known`, `no_exif_fields`,
/// `mtime_anomalous`) are NOT carried on the variants — the driver bumps
/// the right `IngestStats` atomics at the point of decision inside
/// `ingest_one`. Single source of truth per fact (closes R3.T10).
///
/// R2-T2 dropped `#[non_exhaustive]` because the enum + the sole driver
/// (`photohelper-cli::commands::ingest::apply_outcome`) ship in the same
/// workspace; an exhaustive match gives strictly stronger guarantees than
/// a runtime WARN on the wildcard arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    /// Newly inserted (no prior row at this `source_path`).
    Inserted(PhotoId),
    /// Same `source_path` had a different `PhotoId`; old row marked
    /// `superseded_at_unix_seconds`.
    SupersededPrevious {
        /// New row's PhotoId.
        new: PhotoId,
        /// Previously-superseded row's PhotoId.
        old: PhotoId,
    },
    /// Same `PhotoId` already exists in the catalog (e.g. re-ingest, or
    /// a hardlink at a different path that resolved to the same content).
    AlreadyCatalogued(PhotoId),
    /// File extension is not in the RAW allowlist.
    SkippedNonRaw,
    /// File is too small to hash (e.g. 0 bytes).
    SkippedHashWindowTooSmall,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_mtime_in_range_returns_unchanged() {
        let (out, anomalous) = clamp_mtime(1_577_836_800); // 2020-01-01
        assert_eq!(out, 1_577_836_800);
        assert!(!anomalous);
    }

    #[test]
    fn clamp_mtime_below_lower_bound_clamps_up() {
        let (out, anomalous) = clamp_mtime(-1);
        assert_eq!(out, MTIME_LOWER_BOUND_UNIX_SECONDS);
        assert!(anomalous);
    }

    #[test]
    fn clamp_mtime_above_upper_bound_clamps_down() {
        let (out, anomalous) = clamp_mtime(5_000_000_000);
        assert_eq!(out, MTIME_UPPER_BOUND_UNIX_SECONDS);
        assert!(anomalous);
    }

    #[test]
    fn exif_orientation_round_trip_for_all_8_tags() {
        for n in 1..=8i64 {
            let parsed = ExifOrientation::from_tag(n).expect("valid tag");
            assert_eq!(parsed.to_tag(), n, "round-trip for tag {n}");
        }
    }

    #[test]
    fn exif_orientation_tag_5_is_transpose() {
        assert_eq!(
            ExifOrientation::from_tag(5).unwrap(),
            ExifOrientation::Transpose
        );
    }

    #[test]
    fn exif_orientation_tag_7_is_transverse() {
        assert_eq!(
            ExifOrientation::from_tag(7).unwrap(),
            ExifOrientation::Transverse
        );
    }

    #[test]
    fn exif_orientation_tag_0_returns_invalid_tag_error() {
        assert!(matches!(
            ExifOrientation::from_tag(0),
            Err(Error::InvalidExifOrientationTag { tag: 0 })
        ));
    }

    #[test]
    fn exif_orientation_tag_9_returns_invalid_tag_error() {
        assert!(matches!(
            ExifOrientation::from_tag(9),
            Err(Error::InvalidExifOrientationTag { tag: 9 })
        ));
    }

    #[test]
    fn known_camera_slug_round_trip() {
        assert_eq!(KnownCamera::CanonR8.slug(), "canon-r8");
        assert_eq!(
            KnownCamera::from_slug("canon-r8"),
            Some(KnownCamera::CanonR8)
        );
        assert_eq!(KnownCamera::from_slug("nope"), None);
    }

    #[test]
    fn photoid_display_is_43_chars_url_safe_base64() {
        let id = PhotoId([0u8; 32]);
        let s = format!("{id}");
        assert_eq!(s.len(), 43, "32 bytes base64url no-pad = ceil(32*8/6) = 43");
        assert!(
            !s.contains('+') && !s.contains('/') && !s.contains('='),
            "URL-safe no-pad alphabet"
        );
    }

    #[test]
    fn photoid_derive_zero_byte_file_returns_hash_window_too_small() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("empty.cr3");
        std::fs::write(&p, b"").unwrap();
        let err = PhotoId::derive(&p).unwrap_err();
        assert!(matches!(err, Error::HashWindowTooSmall { len: 0, .. }));
    }

    #[test]
    fn photoid_derive_small_file_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("small.cr3");
        std::fs::write(&p, vec![0x42u8; 100]).unwrap();
        let id = PhotoId::derive(&p).expect("100-byte file should hash");
        assert_ne!(id.as_bytes(), &[0u8; 32]);
    }

    #[test]
    fn photoid_derive_window_disjoint_for_files_64k_to_128k() {
        // R1.T3 regression test: two 100KB files differing ONLY in
        // bytes [40000..50000) must produce different PhotoIds. With
        // the pre-fix overlapping windows, bytes [36864..65536) were
        // hashed twice; the [40000..50000) difference still affected
        // both head and tail, so this test cannot distinguish overlap
        // from disjoint *just* by hash difference. Instead, assert
        // the spec invariant: head reads exactly first 64KB; tail
        // reads exactly the bytes after that until file_size. For a
        // 100KB file: head [0..64KB), tail [64KB..100KB) = 36KB.
        // Construct content where differing the byte at offset 70000
        // (in the tail region only) changes the hash, and differing
        // the byte at offset 30000 (in head only) also changes the
        // hash. The previous overlap meant bytes [36864..65536) were
        // hashed twice — visible as 2× hash sensitivity to changes
        // there. Disjoint = bytes hashed exactly once.
        let dir = tempfile::tempdir().unwrap();
        let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0);

        // Compute the EXACT expected hash for a 100KB all-zero file
        // under the disjoint-window spec: head = [0..64KB) of zeros,
        // tail = [64KB..100KB) of zeros (36KB), file_size = 102400.
        let file_size: u64 = 100 * 1024;
        let head = vec![0u8; 64 * 1024];
        let tail = vec![0u8; 36 * 1024];
        let mut hasher = blake3::Hasher::new();
        hasher.update(&file_size.to_le_bytes());
        hasher.update(&1_577_836_800_i64.to_le_bytes());
        hasher.update(&head);
        hasher.update(&tail);
        let expected = *hasher.finalize().as_bytes();

        let p = dir.path().join("100k.cr3");
        std::fs::write(&p, vec![0u8; 100 * 1024]).unwrap();
        filetime::set_file_mtime(&p, mtime).unwrap();
        let actual = PhotoId::derive_with_clamped_mtime(&p, file_size, 1_577_836_800).unwrap();
        assert_eq!(
            actual.as_bytes(),
            &expected,
            "100KB file must hash with DISJOINT head[0..64KB) + tail[64KB..100KB), not overlapping windows",
        );
    }

    #[test]
    fn photoid_derive_window_disjoint_distinguishes_overlap_region_changes() {
        // R2-T19 rewrite: the previous `..._exactly_128k` test used an
        // all-0xAA 128KB file — the one size at which the BUGGY pre-R1.T3
        // code and the FIXED disjoint code feed IDENTICAL bytes to BLAKE3
        // (head=[0..64KB) of 0xAA, tail=[64KB..128KB) of 0xAA in both
        // implementations). That test passed against either implementation
        // and so did not actually pin the disjoint-window invariant.
        //
        // This rewrite uses 96KB files where the bytes in the overlap
        // window [32KB..64KB) differ between two files. Under the BUGGY
        // overlap code (head=[0..64KB), tail=[32KB..96KB)) the differing
        // bytes get hashed TWICE; under the FIXED disjoint code
        // (head=[0..64KB), tail=[64KB..96KB)) the differing bytes get
        // hashed ONCE in the head and NOT in the tail. The total hashed
        // byte count differs between the two implementations for the same
        // 96KB content — so a regression to overlap math would change
        // the resulting hash, failing this test.
        let dir = tempfile::tempdir().unwrap();
        let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0);
        let file_size: u64 = 96 * 1024;

        // Compute the EXPECTED hash under the DISJOINT invariant:
        //   head = file[0..64KB)
        //   tail = file[64KB..96KB)   (no overlap, no gap)
        let mut content = vec![0xAAu8; (96 * 1024) as usize];
        // Place a distinct sentinel in the would-be-overlap region
        // [32KB..64KB) — these bytes appear in head exactly once under
        // the disjoint code, and would appear in BOTH head and tail
        // under the buggy code (different hash).
        for byte in content.iter_mut().take(64 * 1024).skip(32 * 1024) {
            *byte = 0x77;
        }
        let head_disjoint = &content[0..(64 * 1024)];
        let tail_disjoint = &content[(64 * 1024)..(96 * 1024)];
        let mut hasher = blake3::Hasher::new();
        hasher.update(&file_size.to_le_bytes());
        hasher.update(&1_577_836_800_i64.to_le_bytes());
        hasher.update(head_disjoint);
        hasher.update(tail_disjoint);
        let expected_disjoint = *hasher.finalize().as_bytes();

        // Also compute the BUGGY-overlap hash to confirm it would differ —
        // if a regression to overlap math lands, the actual hash matches
        // this value instead, and the != assertion below would fail.
        let head_buggy = &content[0..(64 * 1024)];
        let tail_buggy = &content[(32 * 1024)..(96 * 1024)];
        let mut buggy_hasher = blake3::Hasher::new();
        buggy_hasher.update(&file_size.to_le_bytes());
        buggy_hasher.update(&1_577_836_800_i64.to_le_bytes());
        buggy_hasher.update(head_buggy);
        buggy_hasher.update(tail_buggy);
        let expected_buggy = *buggy_hasher.finalize().as_bytes();

        // Sanity: the two expected hashes MUST differ (or this whole
        // test isn't discriminating anything).
        assert_ne!(
            expected_disjoint, expected_buggy,
            "test fixture is broken: disjoint and buggy implementations would produce the same hash here",
        );

        let p = dir.path().join("96k.cr3");
        std::fs::write(&p, &content).unwrap();
        filetime::set_file_mtime(&p, mtime).unwrap();
        let actual = PhotoId::derive_with_clamped_mtime(&p, file_size, 1_577_836_800).unwrap();
        assert_eq!(
            actual.as_bytes(),
            &expected_disjoint,
            "PhotoId must follow the DISJOINT invariant; a regression to overlapping windows would produce {expected_buggy:?}",
        );
        assert_ne!(
            actual.as_bytes(),
            &expected_buggy,
            "PhotoId must NOT match the buggy-overlap hash",
        );
    }

    #[test]
    fn photoid_derive_stability_same_content_same_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("stable.cr3");
        std::fs::write(&p, vec![0xAAu8; 1024]).unwrap();
        // Pin mtime to make this deterministic across runs.
        let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0);
        filetime::set_file_mtime(&p, mtime).unwrap();
        let id_a = PhotoId::derive(&p).unwrap();
        let id_b = PhotoId::derive(&p).unwrap();
        assert_eq!(id_a, id_b);
    }

    #[test]
    fn photoid_derive_distinguishability_different_sizes_same_window_content() {
        let dir = tempfile::tempdir().unwrap();
        let p_small = dir.path().join("a.cr3");
        let p_big = dir.path().join("b.cr3");
        // Both files start (and end, since <64KB) with all-zero content but
        // have different sizes. file_size prefix should distinguish them.
        std::fs::write(&p_small, vec![0u8; 100]).unwrap();
        std::fs::write(&p_big, vec![0u8; 200]).unwrap();
        let mtime = filetime::FileTime::from_unix_time(1_577_836_800, 0);
        filetime::set_file_mtime(&p_small, mtime).unwrap();
        filetime::set_file_mtime(&p_big, mtime).unwrap();
        let a = PhotoId::derive(&p_small).unwrap();
        let b = PhotoId::derive(&p_big).unwrap();
        assert_ne!(
            a, b,
            "file_size prefix should distinguish equal-content files of different sizes"
        );
    }

    #[test]
    fn photoid_le_endian_regression_fixed_input_produces_fixed_output() {
        // Compute the expected hash deterministically using the same recipe
        // as derive_with_clamped_mtime: file_size_le || mtime_le || content
        // (no tail, since file is < 64KB).
        let content = vec![0xCCu8; 256];
        let file_size: u64 = content.len() as u64;
        let mtime: i64 = 1_577_836_800;
        let mut hasher = blake3::Hasher::new();
        hasher.update(&file_size.to_le_bytes());
        hasher.update(&mtime.to_le_bytes());
        hasher.update(&content);
        let expected = *hasher.finalize().as_bytes();

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("le.cr3");
        std::fs::write(&p, &content).unwrap();
        let actual = PhotoId::derive_with_clamped_mtime(&p, file_size, mtime).unwrap();
        assert_eq!(actual.as_bytes(), &expected);
    }

    #[test]
    fn photoid_display_round_trips_via_from_db_bytes() {
        let original = PhotoId::from_db_bytes([42u8; 32]);
        let display = format!("{original}");
        let decoded = URL_SAFE_NO_PAD.decode(&display).unwrap();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&decoded);
        let reconstructed = crate::catalog_glue::photo_id_from_row_bytes(bytes);
        assert_eq!(original, reconstructed);
    }

    #[test]
    fn abspath_canonicalize_rejects_nul_byte() {
        let bad = std::path::PathBuf::from("/tmp/has\0nul");
        let err = AbsPath::canonicalize(&bad).unwrap_err();
        assert!(matches!(
            err,
            Error::Io {
                op: "canonicalize-nul-check",
                ..
            }
        ));
    }

    #[test]
    fn abspath_canonicalize_rejects_nonexistent() {
        let bad = std::path::PathBuf::from("/this/does/not/exist/anywhere");
        let err = AbsPath::canonicalize(&bad).unwrap_err();
        assert!(matches!(
            err,
            Error::Io {
                op: "canonicalize",
                ..
            }
        ));
    }

    #[test]
    fn abspath_canonicalize_within_root_equals_path_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let root = AbsPath::canonicalize(dir.path()).unwrap();
        let same = AbsPath::canonicalize_within(&root, dir.path()).unwrap();
        assert_eq!(root, same);
    }

    #[test]
    #[cfg(unix)]
    fn abspath_canonicalize_within_rejects_symlink_escape() {
        let inside = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("secret");
        std::fs::write(&outside_file, b"x").unwrap();
        let escape = inside.path().join("escape");
        std::os::unix::fs::symlink(&outside_file, &escape).unwrap();
        let root = AbsPath::canonicalize(inside.path()).unwrap();
        let err = AbsPath::canonicalize_within(&root, &escape).unwrap_err();
        assert!(matches!(err, Error::PathEscapesRoot { .. }));
    }

    #[test]
    fn aspect_landscape_wider_than_tall_no_orientation_swap() {
        let p = make_photo_with_exif(ExifMetadata {
            width: Some(4000),
            height: Some(3000),
            orientation: Some(ExifOrientation::Normal),
            ..ExifMetadata::default()
        });
        assert_eq!(p.aspect(), Some(Aspect::Landscape));
    }

    #[test]
    fn aspect_portrait_taller_than_wide_no_orientation_swap() {
        let p = make_photo_with_exif(ExifMetadata {
            width: Some(3000),
            height: Some(4000),
            orientation: Some(ExifOrientation::Normal),
            ..ExifMetadata::default()
        });
        assert_eq!(p.aspect(), Some(Aspect::Portrait));
    }

    #[test]
    fn aspect_landscape_via_rotate_90cw_swap() {
        // Sensor 4000x3000 with Rotate90Cw renders 3000x4000 visually =
        // portrait? Actually rotating a landscape sensor 90 CW produces
        // a portrait display. Pixel dims swap.
        let p = make_photo_with_exif(ExifMetadata {
            width: Some(4000),
            height: Some(3000),
            orientation: Some(ExifOrientation::Rotate90Cw),
            ..ExifMetadata::default()
        });
        assert_eq!(p.aspect(), Some(Aspect::Portrait));
    }

    #[test]
    fn aspect_square_equal_dims() {
        let p = make_photo_with_exif(ExifMetadata {
            width: Some(1000),
            height: Some(1000),
            orientation: Some(ExifOrientation::Normal),
            ..ExifMetadata::default()
        });
        assert_eq!(p.aspect(), Some(Aspect::Square));
    }

    #[test]
    fn aspect_none_when_dims_missing() {
        let p = make_photo_with_exif(ExifMetadata::default());
        assert_eq!(p.aspect(), None);
    }

    #[test]
    fn exif_metadata_is_empty_for_default() {
        assert!(ExifMetadata::default().is_empty());
    }

    #[test]
    fn exif_metadata_is_not_empty_when_make_set() {
        let m = ExifMetadata {
            make: Some("Canon".into()),
            ..ExifMetadata::default()
        };
        assert!(!m.is_empty());
    }

    #[test]
    fn photo_constructor_rejects_zero_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = AbsPath::canonicalize(dir.path()).unwrap();
        let pid = PhotoId::from_db_bytes([1u8; 32]);
        let err = Photo::from_filesystem(
            p,
            0,
            1_577_836_800,
            false,
            pid,
            None,
            ExifMetadata::default(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Io { op: "stat", .. }));
    }

    fn make_photo_with_exif(exif: ExifMetadata) -> Photo {
        let dir = tempfile::tempdir().unwrap();
        let p = AbsPath::canonicalize(dir.path()).unwrap();
        let pid = PhotoId::from_db_bytes([0u8; 32]);
        Photo::from_filesystem(p, 1024, 1_577_836_800, false, pid, None, exif).unwrap()
    }
}
