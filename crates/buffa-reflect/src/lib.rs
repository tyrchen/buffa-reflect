//! Runtime reflection for the buffa protobuf implementation.
//!
//! This crate provides a [`DescriptorPool`] that decodes a serialized
//! `google.protobuf.FileDescriptorSet` into navigable descriptor handles
//! ([`MessageDescriptor`], [`FieldDescriptor`], [`EnumDescriptor`],
//! [`OneofDescriptor`], …) and a one-method [`ReflectMessage`] trait that
//! every generated buffa message can implement (typically via the
//! `#[derive(ReflectMessage)]` macro re-exported from this crate).
//!
//! # Quick start
//!
//! ```no_run
//! use buffa_reflect::DescriptorPool;
//!
//! # fn main() -> Result<(), buffa_reflect::DescriptorError> {
//! const FDS_BYTES: &[u8] = b""; // produced by `buffa-reflect-build`
//! let pool = DescriptorPool::decode(FDS_BYTES)?;
//! for msg in pool.all_messages() {
//!     println!("{}: {} fields", msg.full_name(), msg.fields().len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! See [`buffa-reflect-build`](https://docs.rs/buffa-reflect-build) for the
//! companion build-script crate that produces the descriptor set bytes and
//! decorates generated messages with `#[derive(ReflectMessage)]`.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod enumeration;
pub mod error;
pub mod field;
pub mod file;
pub mod message;
pub mod oneof;
pub mod pool;
mod pool_build;
pub mod reflect;

pub use crate::enumeration::{EnumDescriptor, EnumValueDescriptor};
pub use crate::error::DescriptorError;
pub use crate::field::{Cardinality, FieldDescriptor, Kind};
pub use crate::file::FileDescriptor;
pub use crate::message::MessageDescriptor;
pub use crate::oneof::OneofDescriptor;
pub use crate::pool::DescriptorPool;
pub use crate::reflect::ReflectMessage;

// Derive macros and traits live in separate namespaces, so the macro can
// share the trait's name and users write a single `#[derive(ReflectMessage)]`.
#[cfg(feature = "derive")]
pub use buffa_reflect_derive::ReflectMessage;
