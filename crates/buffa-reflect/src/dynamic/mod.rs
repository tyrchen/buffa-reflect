//! Runtime-typed protobuf messages keyed by [`MessageDescriptor`].
//!
//! Phase 2a delivers [`DynamicMessage`] — a message whose field set is
//! discovered at runtime from a descriptor. Use it when the wire schema is
//! known but the Rust type is not (e.g. proto-aware proxies, schema
//! migration tools, generic CLIs).
//!
//! # Examples
//!
//! ```no_run
//! use buffa_reflect::{DescriptorPool, DynamicMessage};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let bytes: &[u8] = &[];
//! let pool = DescriptorPool::decode(bytes)?;
//! let descriptor = pool.get_message_by_name("acme.v1.User").unwrap();
//!
//! let wire: &[u8] = &[];
//! let mut dyn_msg = DynamicMessage::decode(descriptor, wire)?;
//! dyn_msg.set_field_by_name("name", "Alice".into());
//! let bytes = dyn_msg.encode_to_vec();
//! # let _ = bytes;
//! # Ok(())
//! # }
//! ```

pub(crate) mod defaults;
mod fields;
mod message;
mod message_codec;
mod message_decode;
mod unknown;
mod value;

pub use crate::dynamic::{
    message::DynamicMessage,
    unknown::{UnknownField, UnknownFieldSet},
    value::{MapKey, SetFieldError, Value},
};
