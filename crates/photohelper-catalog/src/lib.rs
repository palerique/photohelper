//! SQLite-backed catalog persistence for photohelper. See
//! `docs/plans/session-01.md` §Deliverables 4.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod catalog;
mod row;
mod schema;

pub use catalog::{Catalog, InsertEmbeddingOutcome, InsertScoreOutcome, UpsertOutcome};
pub use row::{CullRow, EmbeddingRow, PhotoRow};
