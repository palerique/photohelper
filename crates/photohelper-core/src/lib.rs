//! Domain model, error types, and the catalog-glue bridge for photohelper.
//!
//! See `docs/plans/session-01.md` for the design contract.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod catalog_glue;
pub mod error;
pub mod model;

pub use error::Error;

/// Crate version, sourced from `Cargo.toml`.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
