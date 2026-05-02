//! [`Value`], [`MapKey`], and [`SetFieldError`] — the runtime value model
//! that backs [`super::DynamicMessage`].

use std::{collections::HashMap, sync::Arc};

use buffa::bytes::Bytes;

use crate::{
    dynamic::message::DynamicMessage,
    field::{FieldDescriptor, Kind},
};

/// A protobuf field value carried by [`super::DynamicMessage`].
///
/// One variant per scalar wire type, plus `EnumNumber` (open-enum
/// semantics — any `i32` is acceptable so unknown variants round-trip
/// losslessly), `Message`, `List`, and `Map`. The variant set mirrors
/// `prost-reflect`'s `Value` for consumer migration.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `bool`.
    Bool(bool),
    /// `int32` / `sint32` / `sfixed32`.
    I32(i32),
    /// `int64` / `sint64` / `sfixed64`.
    I64(i64),
    /// `uint32` / `fixed32`.
    U32(u32),
    /// `uint64` / `fixed64`.
    U64(u64),
    /// `float`.
    F32(f32),
    /// `double`.
    F64(f64),
    /// `string` (UTF-8).
    String(String),
    /// `bytes`.
    Bytes(Bytes),
    /// Enum variant by number.
    ///
    /// `i32` rather than a typed variant so forward-compat decoding (an
    /// unknown enum number) round-trips byte-identically. Matches
    /// proto3's open-enum semantics.
    EnumNumber(i32),
    /// Sub-message.
    Message(DynamicMessage),
    /// Repeated value (every element validated against the list's
    /// declared element [`Kind`]).
    List(Vec<Value>),
    /// `map<K, V>` field.
    Map(HashMap<MapKey, Value>),
}

/// A protobuf map key.
///
/// Variants are limited to those allowed by the proto spec: floats,
/// bytes, and message/enum keys are forbidden.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKey {
    /// `bool`.
    Bool(bool),
    /// `int32` / `sint32` / `sfixed32`.
    I32(i32),
    /// `int64` / `sint64` / `sfixed64`.
    I64(i64),
    /// `uint32` / `fixed32`.
    U32(u32),
    /// `uint64` / `fixed64`.
    U64(u64),
    /// `string`.
    String(String),
}

/// Reasons [`super::DynamicMessage::try_set_field`] (and friends) can fail.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SetFieldError {
    /// The named field / number does not exist on the descriptor.
    #[error("field not found")]
    NotFound,
    /// The supplied value's runtime shape did not match the field.
    #[error("invalid value for field `{}`", field.full_name())]
    InvalidType {
        /// The field that rejected the assignment.
        field: FieldDescriptor,
        /// The offending value (returned for diagnostic purposes).
        value: Box<Value>,
    },
}

impl Value {
    /// The proto default for the given [`Kind`] (singular form).
    ///
    /// For `Message` returns an empty [`super::DynamicMessage`] of the
    /// declared sub-message type. For `Enum`, the zero variant.
    #[must_use]
    pub fn default_value(kind: &Kind) -> Self {
        match kind {
            Kind::Bool => Value::Bool(false),
            Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => Value::I32(0),
            Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => Value::I64(0),
            Kind::Uint32 | Kind::Fixed32 => Value::U32(0),
            Kind::Uint64 | Kind::Fixed64 => Value::U64(0),
            Kind::Float => Value::F32(0.0),
            Kind::Double => Value::F64(0.0),
            Kind::String => Value::String(String::new()),
            Kind::Bytes => Value::Bytes(Bytes::new()),
            Kind::Enum(_) => Value::EnumNumber(0),
            Kind::Message(m) => Value::Message(DynamicMessage::new(m.clone())),
        }
    }

    /// The proto default for `field` accounting for cardinality (lists
    /// and maps default to the empty collection).
    ///
    /// Reads any pre-parsed `[default = …]` from the descriptor pool.
    /// When the descriptor declares no explicit default this returns
    /// the zero value for the field's [`Kind`].
    #[must_use]
    pub fn default_value_for_field(field: &FieldDescriptor) -> Self {
        if field.is_list() {
            return Value::List(Vec::new());
        }
        if field.is_map() {
            return Value::Map(HashMap::new());
        }
        if let Some(value) = field.parsed_default_value() {
            return value;
        }
        Value::default_value(&field.kind())
    }

    /// True iff `self` equals the proto default for `kind`.
    ///
    /// Lists and maps are default iff empty. `Message` is default iff
    /// it is a fresh, populated-fields-empty instance.
    #[must_use]
    pub fn is_default(&self, kind: &Kind) -> bool {
        match (self, kind) {
            (Value::Bool(b), Kind::Bool) => !*b,
            (Value::I32(v), Kind::Int32 | Kind::Sint32 | Kind::Sfixed32) => *v == 0,
            (Value::I64(v), Kind::Int64 | Kind::Sint64 | Kind::Sfixed64) => *v == 0,
            (Value::U32(v), Kind::Uint32 | Kind::Fixed32) => *v == 0,
            (Value::U64(v), Kind::Uint64 | Kind::Fixed64) => *v == 0,
            (Value::F32(v), Kind::Float) => *v == 0.0,
            (Value::F64(v), Kind::Double) => *v == 0.0,
            (Value::String(s), Kind::String) => s.is_empty(),
            (Value::Bytes(b), Kind::Bytes) => b.is_empty(),
            (Value::EnumNumber(n), Kind::Enum(_)) => *n == 0,
            (Value::Message(m), Kind::Message(_)) => m.is_empty(),
            (Value::List(l), _) => l.is_empty(),
            (Value::Map(m), _) => m.is_empty(),
            _ => false,
        }
    }

    /// Validate that `self` matches `field`'s declared shape.
    ///
    /// Recursive: list elements validate against the element kind, map
    /// keys/values against their declared kinds, and sub-message values
    /// must share the field's expected message descriptor (compared
    /// pool-pointer + index).
    #[must_use]
    pub fn is_valid_for_field(&self, field: &FieldDescriptor) -> bool {
        if field.is_list() {
            if let Value::List(items) = self {
                let kind = field.kind();
                return items.iter().all(|v| value_matches_kind(v, &kind));
            }
            return false;
        }
        if field.is_map() {
            if let Value::Map(entries) = self {
                let (key_kind, value_kind) = match map_entry_kinds(field) {
                    Some(kinds) => kinds,
                    None => return false,
                };
                return entries.iter().all(|(k, v)| {
                    map_key_matches_kind(k, &key_kind) && value_matches_kind(v, &value_kind)
                });
            }
            return false;
        }
        value_matches_kind(self, &field.kind())
    }

    // Typed accessors ------------------------------------------------------

    /// Returns the `bool` payload, if any.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the `i32` payload, if any.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::I32(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the `i64` payload, if any.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::I64(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the `u32` payload, if any.
    #[must_use]
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::U32(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the `u64` payload, if any.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::U64(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the `f32` payload, if any.
    #[must_use]
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::F32(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the `f64` payload, if any.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the `&str` payload, if any.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(v) => Some(v.as_str()),
            _ => None,
        }
    }
    /// Returns the `&Bytes` payload, if any.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            Value::Bytes(v) => Some(v),
            _ => None,
        }
    }
    /// Returns the enum-number payload, if any.
    #[must_use]
    pub fn as_enum_number(&self) -> Option<i32> {
        match self {
            Value::EnumNumber(v) => Some(*v),
            _ => None,
        }
    }
    /// Returns the [`super::DynamicMessage`] payload, if any.
    #[must_use]
    pub fn as_message(&self) -> Option<&DynamicMessage> {
        match self {
            Value::Message(v) => Some(v),
            _ => None,
        }
    }
    /// Mutable form of [`Self::as_message`].
    pub fn as_message_mut(&mut self) -> Option<&mut DynamicMessage> {
        match self {
            Value::Message(v) => Some(v),
            _ => None,
        }
    }
    /// Returns the list payload, if any.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v.as_slice()),
            _ => None,
        }
    }
    /// Mutable form of [`Self::as_list`].
    pub fn as_list_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::List(v) => Some(v),
            _ => None,
        }
    }
    /// Returns the map payload, if any.
    #[must_use]
    pub fn as_map(&self) -> Option<&HashMap<MapKey, Value>> {
        match self {
            Value::Map(v) => Some(v),
            _ => None,
        }
    }
    /// Mutable form of [`Self::as_map`].
    pub fn as_map_mut(&mut self) -> Option<&mut HashMap<MapKey, Value>> {
        match self {
            Value::Map(v) => Some(v),
            _ => None,
        }
    }
}

impl MapKey {
    /// The proto default for the given map-key kind.
    ///
    /// # Panics
    ///
    /// Panics if `kind` is not a legal map-key shape (floats, bytes,
    /// messages, and enums are forbidden by the proto spec). The pool
    /// builder rejects such map declarations, so this only fires for
    /// hand-constructed misuse.
    #[must_use]
    pub fn default_value(kind: &Kind) -> Self {
        match kind {
            Kind::Bool => MapKey::Bool(false),
            Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => MapKey::I32(0),
            Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => MapKey::I64(0),
            Kind::Uint32 | Kind::Fixed32 => MapKey::U32(0),
            Kind::Uint64 | Kind::Fixed64 => MapKey::U64(0),
            Kind::String => MapKey::String(String::new()),
            other => panic!("invalid map-key kind: {other:?}"),
        }
    }
}

/// Used by both singular validation and list-element / map-value validation.
pub(crate) fn value_matches_kind(value: &Value, kind: &Kind) -> bool {
    match (value, kind) {
        (Value::Bool(_), Kind::Bool) => true,
        (Value::I32(_), Kind::Int32 | Kind::Sint32 | Kind::Sfixed32) => true,
        (Value::I64(_), Kind::Int64 | Kind::Sint64 | Kind::Sfixed64) => true,
        (Value::U32(_), Kind::Uint32 | Kind::Fixed32) => true,
        (Value::U64(_), Kind::Uint64 | Kind::Fixed64) => true,
        (Value::F32(_), Kind::Float) => true,
        (Value::F64(_), Kind::Double) => true,
        (Value::String(_), Kind::String) => true,
        (Value::Bytes(_), Kind::Bytes) => true,
        (Value::EnumNumber(_), Kind::Enum(_)) => true,
        (Value::Message(m), Kind::Message(d)) => {
            Arc::ptr_eq(&m.descriptor().pool.inner, &d.pool.inner)
                && m.descriptor().index == d.index
        }
        _ => false,
    }
}

pub(crate) fn map_key_matches_kind(key: &MapKey, kind: &Kind) -> bool {
    matches!(
        (key, kind),
        (MapKey::Bool(_), Kind::Bool)
            | (MapKey::I32(_), Kind::Int32 | Kind::Sint32 | Kind::Sfixed32)
            | (MapKey::I64(_), Kind::Int64 | Kind::Sint64 | Kind::Sfixed64)
            | (MapKey::U32(_), Kind::Uint32 | Kind::Fixed32)
            | (MapKey::U64(_), Kind::Uint64 | Kind::Fixed64)
            | (MapKey::String(_), Kind::String)
    )
}

/// Resolve the `(key_kind, value_kind)` of a map field's synthetic
/// entry message.
pub(crate) fn map_entry_kinds(field: &FieldDescriptor) -> Option<(Kind, Kind)> {
    let Kind::Message(entry) = field.kind() else {
        return None;
    };
    if !entry.is_map_entry() {
        return None;
    }
    let key = entry.get_field_by_number(1)?.kind();
    let value = entry.get_field_by_number(2)?.kind();
    Some((key, value))
}

/// Convenience constructors for common literal types.
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::I32(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::I64(v)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::U32(v)
    }
}
impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::U64(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::F32(v)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::F64(v)
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_owned())
    }
}
impl From<Bytes> for Value {
    fn from(v: Bytes) -> Self {
        Value::Bytes(v)
    }
}
impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(Bytes::from(v))
    }
}
impl From<DynamicMessage> for Value {
    fn from(v: DynamicMessage) -> Self {
        Value::Message(v)
    }
}
