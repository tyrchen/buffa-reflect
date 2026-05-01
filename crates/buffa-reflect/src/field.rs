//! [`FieldDescriptor`], [`Kind`], and [`Cardinality`].

use buffa_descriptor::generated::descriptor::FieldDescriptorProto;

use crate::enumeration::EnumDescriptor;
use crate::message::MessageDescriptor;
use crate::oneof::OneofDescriptor;
use crate::pool::{DescriptorPool, KindRef, MessageIndex};

/// Handle to one field in a [`MessageDescriptor`].
#[derive(Clone, Debug)]
pub struct FieldDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) message: MessageIndex,
    pub(crate) index: u32,
}

impl FieldDescriptor {
    fn entry(&self) -> &crate::pool::FieldEntry {
        &self.pool.inner.messages[self.message as usize].fields[self.index as usize]
    }

    /// Proto field name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.entry().name
    }

    /// `<message.full_name>.<name>`, no leading dot.
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.entry().full_name
    }

    /// JSON field name (lower-camelCase by default; honors a user-supplied
    /// `json_name` option when present).
    #[must_use]
    pub fn json_name(&self) -> &str {
        &self.entry().json_name
    }

    /// Tag number on the wire.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.entry().number
    }

    /// Resolved field [`Kind`].
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self.entry().kind {
            KindRef::Double => Kind::Double,
            KindRef::Float => Kind::Float,
            KindRef::Int32 => Kind::Int32,
            KindRef::Int64 => Kind::Int64,
            KindRef::Uint32 => Kind::Uint32,
            KindRef::Uint64 => Kind::Uint64,
            KindRef::Sint32 => Kind::Sint32,
            KindRef::Sint64 => Kind::Sint64,
            KindRef::Fixed32 => Kind::Fixed32,
            KindRef::Fixed64 => Kind::Fixed64,
            KindRef::Sfixed32 => Kind::Sfixed32,
            KindRef::Sfixed64 => Kind::Sfixed64,
            KindRef::Bool => Kind::Bool,
            KindRef::String => Kind::String,
            KindRef::Bytes => Kind::Bytes,
            KindRef::Message(idx) => Kind::Message(MessageDescriptor {
                pool: self.pool.clone(),
                index: idx,
            }),
            KindRef::Enum(idx) => Kind::Enum(EnumDescriptor {
                pool: self.pool.clone(),
                index: idx,
            }),
        }
    }

    /// `Optional`, `Required`, or `Repeated`.
    #[must_use]
    pub fn cardinality(&self) -> Cardinality {
        self.entry().cardinality
    }

    /// True iff the wire format tracks presence for this field (proto2
    /// scalars, message-typed fields, oneof members, proto3 `optional`).
    #[must_use]
    pub fn supports_presence(&self) -> bool {
        self.entry().supports_presence
    }

    /// True iff the field uses packed encoding for repeated scalars.
    #[must_use]
    pub fn is_packed(&self) -> bool {
        self.entry().is_packed
    }

    /// True iff this field models a `map<K, V>`.
    ///
    /// A field is a map iff its kind is a message that carries the
    /// `map_entry = true` option.
    #[must_use]
    pub fn is_map(&self) -> bool {
        match self.entry().kind {
            KindRef::Message(idx) => self.pool.inner.messages[idx as usize].is_map_entry,
            _ => false,
        }
    }

    /// Containing oneof, when this field is part of one.
    #[must_use]
    pub fn containing_oneof(&self) -> Option<OneofDescriptor> {
        self.entry().oneof_index.map(|oi| OneofDescriptor {
            pool: self.pool.clone(),
            message: self.message,
            index: oi,
        })
    }

    /// Owning message.
    #[must_use]
    pub fn parent_message(&self) -> MessageDescriptor {
        MessageDescriptor {
            pool: self.pool.clone(),
            index: self.message,
        }
    }

    /// Raw [`FieldDescriptorProto`] for advanced use.
    #[must_use]
    pub fn descriptor_proto(&self) -> &FieldDescriptorProto {
        let msg_entry = &self.pool.inner.messages[self.message as usize];
        let file = &self.pool.inner.files[msg_entry.file as usize];
        let msg_proto =
            crate::pool_build::resolve_message_proto(&file.proto, &msg_entry.proto_path);
        &msg_proto.field[self.entry().proto_field_index as usize]
    }
}

impl PartialEq for FieldDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner)
            && self.message == other.message
            && self.index == other.index
    }
}

impl Eq for FieldDescriptor {}

/// Resolved scalar / aggregate type for a field.
///
/// Message and enum kinds carry the resolved descriptor handle, not just
/// the type name.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Kind {
    /// `double`
    Double,
    /// `float`
    Float,
    /// `int32`
    Int32,
    /// `int64`
    Int64,
    /// `uint32`
    Uint32,
    /// `uint64`
    Uint64,
    /// `sint32`
    Sint32,
    /// `sint64`
    Sint64,
    /// `fixed32`
    Fixed32,
    /// `fixed64`
    Fixed64,
    /// `sfixed32`
    Sfixed32,
    /// `sfixed64`
    Sfixed64,
    /// `bool`
    Bool,
    /// `string`
    String,
    /// `bytes`
    Bytes,
    /// Sub-message reference.
    Message(MessageDescriptor),
    /// Enum reference.
    Enum(EnumDescriptor),
}

/// Field cardinality (proto label).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cardinality {
    /// `optional` (the proto3 default and proto2 explicit `optional`).
    Optional,
    /// `required` — proto2 only.
    Required,
    /// `repeated`.
    Repeated,
}
