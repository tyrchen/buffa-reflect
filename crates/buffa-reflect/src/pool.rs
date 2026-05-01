//! [`DescriptorPool`] — the root handle holding decoded descriptors.

use std::sync::Arc;

use buffa::Message as _;
use buffa_descriptor::generated::descriptor::FileDescriptorSet;

use crate::{
    enumeration::EnumDescriptor, error::DescriptorError, file::FileDescriptor,
    message::MessageDescriptor,
};

/// Index into [`PoolInner::messages`].
pub(crate) type MessageIndex = u32;
/// Index into [`PoolInner::enums`].
pub(crate) type EnumIndex = u32;
/// Index into [`PoolInner::files`].
pub(crate) type FileIndex = u32;

/// A pool of protobuf descriptors built from one or more
/// `FileDescriptorSet`s.
///
/// Cloning is cheap (`Arc`-shared inner). All descriptor handles
/// ([`MessageDescriptor`], [`FieldDescriptor`](crate::FieldDescriptor),
/// [`EnumDescriptor`], …) own a `DescriptorPool` and are themselves cheap
/// to clone.
#[derive(Clone, Default)]
pub struct DescriptorPool {
    pub(crate) inner: Arc<PoolInner>,
}

impl std::fmt::Debug for DescriptorPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DescriptorPool")
            .field("files", &self.inner.files.len())
            .field("messages", &self.inner.messages.len())
            .field("enums", &self.inner.enums.len())
            .finish()
    }
}

#[derive(Clone, Default)]
pub(crate) struct PoolInner {
    pub(crate) names: hashbrown::HashMap<Box<str>, Definition>,
    pub(crate) file_names: hashbrown::HashMap<Box<str>, FileIndex>,
    pub(crate) files: Vec<FileEntry>,
    pub(crate) messages: Vec<MessageEntry>,
    pub(crate) enums: Vec<EnumEntry>,
}

/// What a fully-qualified name refers to in the pool.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Definition {
    Message(MessageIndex),
    Enum(EnumIndex),
}

#[derive(Clone)]
pub(crate) struct FileEntry {
    pub(crate) proto: buffa_descriptor::generated::descriptor::FileDescriptorProto,
    /// Top-level message indices owned by this file (nested messages live
    /// under their parent's `nested` list).
    pub(crate) messages: Vec<MessageIndex>,
    /// Top-level enum indices owned by this file.
    pub(crate) enums: Vec<EnumIndex>,
}

#[derive(Clone)]
pub(crate) struct MessageEntry {
    /// `<package>.<...>.<MessageName>` — no leading dot.
    pub(crate) full_name: Box<str>,
    /// The leaf name (the part after the last `.`).
    pub(crate) name: Box<str>,
    /// Index into `PoolInner::files`.
    pub(crate) file: FileIndex,
    /// Set when this message is nested inside another.
    pub(crate) parent: Option<MessageIndex>,
    /// Path used to locate the matching `DescriptorProto` inside the file.
    /// Each step is an index into `message_type` / `nested_type` of the
    /// previous level.
    pub(crate) proto_path: Vec<u32>,
    /// Resolved fields, in declaration order.
    pub(crate) fields: Vec<FieldEntry>,
    /// Resolved oneofs, in declaration order. Synthetic oneofs (proto3
    /// optional) are included; consumers can filter with
    /// [`crate::OneofDescriptor::is_synthetic`].
    pub(crate) oneofs: Vec<OneofEntry>,
    /// Nested message indices (recursive — children only, not grandchildren).
    pub(crate) nested_messages: Vec<MessageIndex>,
    /// Nested enum indices.
    pub(crate) nested_enums: Vec<EnumIndex>,
    /// Map from field number to position in `fields`.
    pub(crate) by_number: hashbrown::HashMap<u32, u32>,
    /// Map from proto field name to position in `fields`.
    pub(crate) by_name: hashbrown::HashMap<Box<str>, u32>,
    /// Map from JSON field name to position in `fields`.
    pub(crate) by_json_name: hashbrown::HashMap<Box<str>, u32>,
    /// True iff this message was synthesized as a `map<K, V>` entry by
    /// protoc. Such messages are not user-facing types in generated code.
    pub(crate) is_map_entry: bool,
}

#[derive(Clone)]
pub(crate) struct FieldEntry {
    pub(crate) name: Box<str>,
    pub(crate) full_name: Box<str>,
    pub(crate) json_name: Box<str>,
    pub(crate) number: u32,
    pub(crate) kind: KindRef,
    pub(crate) cardinality: crate::field::Cardinality,
    pub(crate) supports_presence: bool,
    pub(crate) is_packed: bool,
    /// When the field belongs to a oneof, the index into the parent
    /// message's `oneofs` list.
    pub(crate) oneof_index: Option<u32>,
    /// Position of the descriptor inside the parent's
    /// `DescriptorProto::field` list.
    pub(crate) proto_field_index: u32,
}

/// Internal, pool-relative version of [`crate::Kind`]. Avoids storing
/// `MessageDescriptor`/`EnumDescriptor` (which would require an `Arc<Pool>`
/// while the pool is still being built).
#[derive(Clone, Copy, Debug)]
pub(crate) enum KindRef {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes,
    Message(MessageIndex),
    Enum(EnumIndex),
}

#[derive(Clone)]
pub(crate) struct OneofEntry {
    pub(crate) name: Box<str>,
    pub(crate) full_name: Box<str>,
    pub(crate) is_synthetic: bool,
    /// Indices into the parent message's `fields` vector.
    pub(crate) field_indices: Vec<u32>,
    /// Position in the owning message's `oneof_decl` list.
    pub(crate) proto_index: u32,
}

#[derive(Clone)]
pub(crate) struct EnumEntry {
    pub(crate) full_name: Box<str>,
    pub(crate) name: Box<str>,
    pub(crate) file: FileIndex,
    pub(crate) parent: Option<MessageIndex>,
    pub(crate) values: Vec<EnumValueEntry>,
    /// Same role as [`MessageEntry::proto_path`], but pointing into
    /// `enum_type` lists.
    pub(crate) proto_path: Vec<u32>,
    /// Map from variant name to position in `values`.
    pub(crate) by_name: hashbrown::HashMap<Box<str>, u32>,
    /// Map from variant number to position in `values`. (proto allows
    /// multiple variants with the same number when `allow_alias` is set; we
    /// keep the first.)
    pub(crate) by_number: hashbrown::HashMap<i32, u32>,
}

#[derive(Clone)]
pub(crate) struct EnumValueEntry {
    pub(crate) name: Box<str>,
    pub(crate) full_name: Box<str>,
    pub(crate) number: i32,
}

impl DescriptorPool {
    /// Construct an empty pool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode a serialized `google.protobuf.FileDescriptorSet` into a pool.
    ///
    /// # Errors
    /// Returns [`DescriptorError::Decode`] if the bytes are not a valid
    /// `FileDescriptorSet`, or any of the validation errors documented on
    /// [`DescriptorError`].
    pub fn decode(bytes: &[u8]) -> Result<Self, DescriptorError> {
        let fds = FileDescriptorSet::decode_from_slice(bytes)?;
        Self::from_file_descriptor_set(fds)
    }

    /// Build a pool from an already-decoded `FileDescriptorSet`.
    ///
    /// # Errors
    /// See [`DescriptorError`].
    pub fn from_file_descriptor_set(fds: FileDescriptorSet) -> Result<Self, DescriptorError> {
        let mut pool = Self::new();
        pool.add_file_descriptor_set(fds)?;
        Ok(pool)
    }

    /// Merge another `FileDescriptorSet` into the pool.
    ///
    /// # Errors
    /// See [`DescriptorError`].
    pub fn add_file_descriptor_set(
        &mut self,
        fds: FileDescriptorSet,
    ) -> Result<(), DescriptorError> {
        let inner = Arc::make_mut(&mut self.inner);
        crate::pool_build::ingest_file_descriptor_set(inner, fds)
    }

    /// Iterate over every file in the pool.
    pub fn files(&self) -> impl ExactSizeIterator<Item = FileDescriptor> + '_ {
        (0..self.inner.files.len() as u32).map(move |idx| FileDescriptor {
            pool: self.clone(),
            index: idx,
        })
    }

    /// Iterate over every message in the pool, including nested messages
    /// and synthesized map-entry messages.
    pub fn all_messages(&self) -> impl ExactSizeIterator<Item = MessageDescriptor> + '_ {
        (0..self.inner.messages.len() as u32).map(move |idx| MessageDescriptor {
            pool: self.clone(),
            index: idx,
        })
    }

    /// Iterate over every enum in the pool, including nested enums.
    pub fn all_enums(&self) -> impl ExactSizeIterator<Item = EnumDescriptor> + '_ {
        (0..self.inner.enums.len() as u32).map(move |idx| EnumDescriptor {
            pool: self.clone(),
            index: idx,
        })
    }

    /// Resolve a fully-qualified message name. Accepts either `pkg.Name` or
    /// `.pkg.Name` (the leading dot is stripped).
    #[must_use]
    pub fn get_message_by_name(&self, full_name: &str) -> Option<MessageDescriptor> {
        let key = full_name.strip_prefix('.').unwrap_or(full_name);
        match self.inner.names.get(key)? {
            Definition::Message(idx) => Some(MessageDescriptor {
                pool: self.clone(),
                index: *idx,
            }),
            Definition::Enum(_) => None,
        }
    }

    /// Resolve a fully-qualified enum name (with or without leading dot).
    #[must_use]
    pub fn get_enum_by_name(&self, full_name: &str) -> Option<EnumDescriptor> {
        let key = full_name.strip_prefix('.').unwrap_or(full_name);
        match self.inner.names.get(key)? {
            Definition::Enum(idx) => Some(EnumDescriptor {
                pool: self.clone(),
                index: *idx,
            }),
            Definition::Message(_) => None,
        }
    }

    /// Resolve a file by its descriptor name (e.g. `acme/api/v1/user.proto`).
    #[must_use]
    pub fn get_file_by_name(&self, name: &str) -> Option<FileDescriptor> {
        self.inner.file_names.get(name).map(|idx| FileDescriptor {
            pool: self.clone(),
            index: *idx,
        })
    }
}
