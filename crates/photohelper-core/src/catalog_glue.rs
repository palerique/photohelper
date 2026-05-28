//! Catalog-glue bridge.
//!
//! `PhotoId::from_db_bytes` is `pub(crate)` so only `photohelper-core` itself
//! can mint PhotoIds from raw bytes. `photohelper-catalog` reconstructs rows
//! by calling [`photo_id_from_row_bytes`] — the one public function on the
//! catalog-reconstruction path.
//!
//! A non-catalog caller writing
//! `core::catalog_glue::photo_id_from_row_bytes(arbitrary_bytes)` is visibly
//! misusing an API named after its purpose — strong intent signal even
//! though the function is technically `pub`.
//!
//! See `docs/plans/session-01.md` §Deliverables 2 + Round 3 Theme 2 closure.

use crate::model::PhotoId;

/// Reconstruct a `PhotoId` from raw 32-byte catalog row bytes.
///
/// Intended caller: `photohelper-catalog::row::PhotoRow::from_row`.
/// Any other caller is bypassing the content-derivation invariant —
/// the function name is the signal.
#[must_use]
pub fn photo_id_from_row_bytes(raw: [u8; 32]) -> PhotoId {
    PhotoId::from_db_bytes(raw)
}
