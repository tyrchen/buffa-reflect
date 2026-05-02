//! Proto3 canonical JSON for [`crate::DynamicMessage`].
//!
//! Provides `serde::Serialize` for `DynamicMessage` and a
//! `serde::de::DeserializeSeed` impl on `MessageDescriptor` that
//! pairs naturally with `serde_json::Deserializer`. Configuration
//! lives on [`SerializeOptions`] and [`DeserializeOptions`]; field
//! names match prost-reflect's so cross-ecosystem migration is
//! mechanical.

pub(crate) mod case;
mod de;
mod ser;

use serde::de::{DeserializeSeed, Deserializer};
use serde::ser::{Serialize, Serializer};

use crate::dynamic::message::DynamicMessage;
use crate::message::MessageDescriptor;

/// Knobs controlling JSON serialization. Defaults match the proto3
/// JSON canonical form.
#[derive(Debug, Clone)]
pub struct SerializeOptions {
    pub(crate) stringify_64_bit_integers: bool,
    pub(crate) use_enum_numbers: bool,
    pub(crate) use_proto_field_name: bool,
    pub(crate) skip_default_fields: bool,
}

impl Default for SerializeOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl SerializeOptions {
    /// Defaults: `stringify_64_bit_integers = true`, all others
    /// `false` except `skip_default_fields = true`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stringify_64_bit_integers: true,
            use_enum_numbers: false,
            use_proto_field_name: false,
            skip_default_fields: true,
        }
    }

    /// Encode `int64`/`uint64` as JSON strings (per proto3 spec).
    /// Default `true`.
    #[must_use]
    pub const fn stringify_64_bit_integers(mut self, yes: bool) -> Self {
        self.stringify_64_bit_integers = yes;
        self
    }

    /// Encode enums as JSON numbers rather than their declared names.
    /// Default `false`.
    #[must_use]
    pub const fn use_enum_numbers(mut self, yes: bool) -> Self {
        self.use_enum_numbers = yes;
        self
    }

    /// Use `snake_case` proto names instead of lower-camel-case JSON
    /// names. Default `false`.
    #[must_use]
    pub const fn use_proto_field_name(mut self, yes: bool) -> Self {
        self.use_proto_field_name = yes;
        self
    }

    /// Omit fields whose value equals the proto default. Default
    /// `true`.
    #[must_use]
    pub const fn skip_default_fields(mut self, yes: bool) -> Self {
        self.skip_default_fields = yes;
        self
    }
}

/// Knobs controlling JSON deserialization.
#[derive(Debug, Clone)]
pub struct DeserializeOptions {
    pub(crate) deny_unknown_fields: bool,
}

impl Default for DeserializeOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl DeserializeOptions {
    /// Defaults: `deny_unknown_fields = false` per proto3 JSON spec
    /// (`ignore_unknown_fields = true` by default). Flip the knob on
    /// when you'd rather catch schema mismatches early.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            deny_unknown_fields: false,
        }
    }

    /// When `true`, decoders raise `serde::de::Error::unknown_field`
    /// instead of silently dropping. Default `false`.
    #[must_use]
    pub const fn deny_unknown_fields(mut self, yes: bool) -> Self {
        self.deny_unknown_fields = yes;
        self
    }
}

impl Serialize for DynamicMessage {
    /// Serialize via the canonical proto3 JSON encoding using the
    /// default [`SerializeOptions`].
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize_with_options(serializer, &SerializeOptions::default())
    }
}

impl<'de> DeserializeSeed<'de> for MessageDescriptor {
    type Value = DynamicMessage;

    /// Make `MessageDescriptor` itself the seed — `descriptor.deserialize(&mut json)`.
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        DynamicMessage::deserialize(self, deserializer)
    }
}

impl DynamicMessage {
    /// Serialize with explicit [`SerializeOptions`].
    ///
    /// # Errors
    ///
    /// Bubbles up the `Serializer`'s own errors. Schema-level problems
    /// (e.g., a non-finite double inside `google.protobuf.Value`) are
    /// surfaced as `serde::ser::Error::custom(...)`.
    pub fn serialize_with_options<S: Serializer>(
        &self,
        serializer: S,
        options: &SerializeOptions,
    ) -> Result<S::Ok, S::Error> {
        ser::serialize_message(self, serializer, options)
    }

    /// Deserialize a [`DynamicMessage`] of the given descriptor from
    /// `deserializer`, using the canonical proto3 JSON encoding with
    /// default [`DeserializeOptions`].
    ///
    /// # Errors
    ///
    /// Bubbles up the `Deserializer`'s errors and any schema mismatches
    /// from the proto3 JSON mapping (surfaced as `Error::custom`).
    pub fn deserialize<'de, D: Deserializer<'de>>(
        descriptor: MessageDescriptor,
        deserializer: D,
    ) -> Result<Self, D::Error> {
        Self::deserialize_with_options(descriptor, deserializer, &DeserializeOptions::default())
    }

    /// Deserialize with explicit [`DeserializeOptions`].
    ///
    /// # Errors
    ///
    /// See [`Self::deserialize`].
    pub fn deserialize_with_options<'de, D: Deserializer<'de>>(
        descriptor: MessageDescriptor,
        deserializer: D,
        options: &DeserializeOptions,
    ) -> Result<Self, D::Error> {
        de::deserialize_message(&descriptor, deserializer, options)
    }
}

/// True iff `full_name` names a well-known type with a non-default
/// JSON mapping.
pub(crate) fn is_well_known_type(full_name: &str) -> bool {
    matches!(
        full_name,
        "google.protobuf.Any"
            | "google.protobuf.Timestamp"
            | "google.protobuf.Duration"
            | "google.protobuf.Struct"
            | "google.protobuf.FloatValue"
            | "google.protobuf.DoubleValue"
            | "google.protobuf.Int32Value"
            | "google.protobuf.Int64Value"
            | "google.protobuf.UInt32Value"
            | "google.protobuf.UInt64Value"
            | "google.protobuf.BoolValue"
            | "google.protobuf.StringValue"
            | "google.protobuf.BytesValue"
            | "google.protobuf.FieldMask"
            | "google.protobuf.ListValue"
            | "google.protobuf.Value"
            | "google.protobuf.Empty"
    )
}
