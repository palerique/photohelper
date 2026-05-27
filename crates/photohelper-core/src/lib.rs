//! Domain model, pipeline trait, and shared error types for photohelper.
//!
//! Bootstrap stub: real types (`PhotoId`, `Photo`, `Catalog`, `Sidecar`,
//! `DevelopSettings`, `CullingScore`, `Pipeline` trait) land in session 01.

/// Crate version, sourced from `Cargo.toml`.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
