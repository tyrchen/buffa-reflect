//! Build-script integration for `buffa-reflect`.
//!
//! Drives [`buffa_build`] to compile `.proto` files, additionally:
//!
//! * emits `OUT_DIR/file_descriptor_set.bin` (a wire-compatible
//!   `google.protobuf.FileDescriptorSet`),
//! * decorates every generated message struct with
//!   `#[derive(::buffa_reflect::ReflectMessage)]` and a
//!   `#[buffa_reflect(...)]` attribute that wires it back to either a
//!   user-supplied descriptor pool or the embedded descriptor bytes.
//!
//! See the crate-level `Builder` type for the full surface.

mod builder;

pub use crate::builder::{Builder, Error};
