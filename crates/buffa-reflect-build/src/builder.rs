//! Builder placeholder; implemented in M4.

use std::path::PathBuf;

/// Builder for compiling `.proto` files with reflection metadata enabled.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    _files: Vec<PathBuf>,
}

/// Errors raised by [`Builder::compile`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Placeholder until M4.
    #[error("buffa-reflect-build is not implemented yet")]
    Unimplemented,
}

impl Builder {
    /// Construct an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}
