use crate::error::Error;
use std::path::Path;

/// A strongly-typed wrapper around a `Path` guaranteeing an `.xmp` extension.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SidecarPath<'a>(&'a Path);

impl<'a> SidecarPath<'a> {
    /// Creates a new `SidecarPath`. Returns an error if the path doesn't have an `xmp` extension.
    pub fn new(path: &'a Path) -> Result<Self, Error> {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xmp"))
        {
            Ok(Self(path))
        } else {
            Err(Error::Validation {
                message: format!("path {} must have an .xmp extension", path.display()),
            })
        }
    }

    /// Returns the underlying `Path`.
    pub fn as_path(&self) -> &'a Path {
        self.0
    }
}

impl std::ops::Deref for SidecarPath<'_> {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl AsRef<Path> for SidecarPath<'_> {
    fn as_ref(&self) -> &Path {
        self.0
    }
}
