//! Errors raised when constructing or querying a [`crate::DescriptorPool`].

use buffa::DecodeError;

/// All ways descriptor pool construction or lookup can fail.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DescriptorError {
    /// The serialized `FileDescriptorSet` bytes could not be parsed.
    #[error("failed to decode FileDescriptorSet bytes: {0}")]
    Decode(#[from] DecodeError),

    /// A `FileDescriptorProto.name` field was missing.
    #[error("file descriptor is missing the required `name` field")]
    MissingFileName,

    /// A nested type (message / enum / field / oneof) had no `name` set.
    #[error("descriptor in `{location}` is missing the required `name` field")]
    MissingName {
        /// Containing context (file or fully-qualified message name) for the
        /// offending descriptor.
        location: String,
    },

    /// A field referenced a `type_name` that no descriptor in the pool
    /// defines.
    #[error("field `{field}` references unknown type `{type_name}`")]
    UnresolvedType {
        /// Fully-qualified name of the field whose `type_name` is dangling.
        field: String,
        /// The unresolved `type_name` value verbatim.
        type_name: String,
    },

    /// Two descriptors share the same fully-qualified name within a single
    /// pool.
    #[error("duplicate type definition: `{0}`")]
    DuplicateType(String),

    /// Two `FileDescriptorProto`s carry the same `name`.
    #[error("duplicate file descriptor: `{0}`")]
    DuplicateFile(String),

    /// proto3 forbids `LABEL_REQUIRED`. The descriptor declared one anyway.
    #[error("field `{field}` uses `required` in a proto3 file")]
    Proto3RequiredField {
        /// Fully-qualified field name.
        field: String,
    },

    /// A field declared a number outside the protobuf-permitted range or
    /// inside the reserved internal range.
    #[error(
        "invalid field number {number} in `{message}`: must be in 1..={max} and outside \
         19000..=19999"
    )]
    InvalidFieldNumber {
        /// Fully-qualified message name owning the field.
        message: String,
        /// The offending number as it appeared on the descriptor.
        number: i32,
        /// Maximum permissible field number (`536_870_911`).
        max: u32,
    },

    /// `FieldDescriptorProto.type` was unset and no `type_name` was supplied
    /// — we cannot resolve the field's [`crate::Kind`].
    #[error("field `{field}` is missing both `type` and `type_name`")]
    MissingFieldType {
        /// Fully-qualified field name.
        field: String,
    },

    /// `FieldDescriptorProto.type` was
    /// [`MESSAGE`](buffa_descriptor::generated::descriptor::field_descriptor_proto::Type)
    /// or `GROUP`/`ENUM` but `type_name` was empty.
    #[error("field `{field}` has type {kind:?} but no `type_name`")]
    MissingTypeName {
        /// Fully-qualified field name.
        field: String,
        /// The descriptor's declared `type` enum value.
        kind: &'static str,
    },

    /// A `oneof_index` referenced a slot that does not exist in the
    /// containing message's `oneof_decl` list.
    #[error("field `{field}` references oneof index {index} but message has only {count} oneofs")]
    InvalidOneofIndex {
        /// Fully-qualified field name.
        field: String,
        /// The offending oneof index.
        index: i32,
        /// Number of `oneof_decl` entries in the containing message.
        count: usize,
    },

    /// A proto3 enum is missing a variant with value `0` (required for
    /// proto3 default semantics).
    #[error("proto3 enum `{0}` is missing the required value-0 variant")]
    Proto3EnumMissingZero(String),

    /// `FieldDescriptorProto.default_value` could not be parsed against
    /// the field's resolved kind.
    #[error("invalid default value for field `{field}`: `{value}` ({message})")]
    InvalidDefaultValue {
        /// Fully-qualified field name.
        field: String,
        /// The literal string that failed to parse.
        value: String,
        /// Parser-level diagnostic.
        message: String,
    },

    /// Generic descriptor-validation failure.
    #[error("descriptor validation: {0}")]
    Validation(String),
}
