//! Build-script integration for `buffa-reflect`.
//!
//! Drives [`buffa_build`] to compile `.proto` files, additionally:
//!
//! * emits `OUT_DIR/file_descriptor_set.bin` (a wire-compatible
//!   `google.protobuf.FileDescriptorSet`),
//! * decorates every generated message struct with `#[derive(::buffa_reflect::ReflectMessage)]` and
//!   a `#[buffa_reflect(...)]` attribute that wires it back to either a user-supplied descriptor
//!   pool or the embedded descriptor bytes.
//!
//! See the crate-level `Builder` type for the full surface.
//!
//! ## Why sync std I/O / `Command` here
//!
//! This crate runs from `build.rs`, which cargo invokes synchronously
//! without a tokio runtime. `tokio::fs` / `tokio::process::Command` would
//! force every consumer to spin up a runtime in their build script for no
//! benefit. The workspace clippy lint banning sync I/O is therefore
//! suppressed at this crate's root with full justification.

#![allow(
    clippy::disallowed_types,
    clippy::disallowed_methods,
    reason = "build scripts and codegen run synchronously; pulling in a tokio runtime here would \
              burden every downstream build.rs."
)]

mod builder;

pub use crate::builder::{Builder, Error};
