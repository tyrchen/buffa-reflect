//! `serde::Serialize` driver for [`crate::DynamicMessage`].

mod wkt;

use base64::Engine as _;
use serde::ser::{Error as _, SerializeMap as _, SerializeSeq as _, Serializer};

use crate::{
    dynamic::{
        message::DynamicMessage,
        serde::SerializeOptions,
        value::{MapKey, Value},
    },
    field::{FieldDescriptor, Kind},
};

pub(crate) fn serialize_message<S: Serializer>(
    message: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let full_name = message.descriptor().full_name().to_string();
    if super::is_well_known_type(&full_name) {
        return wkt::serialize_wkt(&full_name, message, serializer, options);
    }
    serialize_plain_message(message, serializer, options)
}

pub(super) fn serialize_plain_message<S: Serializer>(
    message: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let descriptor = message.descriptor();

    // Pre-compute which fields to emit. Iterate in proto declaration
    // order so the JSON output mirrors the source.
    let mut entries: Vec<(FieldDescriptor, Value)> = Vec::new();
    for field in descriptor.fields() {
        let included = if options.skip_default_fields {
            message.has_field(&field)
        } else {
            true
        };
        if !included {
            continue;
        }
        let value = match message.get_field(&field) {
            std::borrow::Cow::Borrowed(v) => v.clone(),
            std::borrow::Cow::Owned(v) => v,
        };
        entries.push((field, value));
    }

    let mut map = serializer.serialize_map(Some(entries.len()))?;
    for (field, value) in &entries {
        let key = if options.use_proto_field_name {
            field.name().to_string()
        } else {
            field.json_name().to_string()
        };
        map.serialize_entry(
            &key,
            &SerializeFieldValue {
                field,
                value,
                options,
            },
        )?;
    }
    map.end()
}

/// Helper newtype that pairs a `FieldDescriptor` with a `Value` so we
/// can pass a single `Serialize` impl to `SerializeMap::serialize_entry`.
pub(super) struct SerializeFieldValue<'a> {
    pub(super) field: &'a FieldDescriptor,
    pub(super) value: &'a Value,
    pub(super) options: &'a SerializeOptions,
}

impl serde::Serialize for SerializeFieldValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.field.is_map() {
            return serialize_map_value(self.field, self.value, serializer, self.options);
        }
        if self.field.is_list() {
            return serialize_list_value(self.field, self.value, serializer, self.options);
        }
        serialize_singular_value(&self.field.kind(), self.value, serializer, self.options)
    }
}

pub(super) fn serialize_singular_value<S: Serializer>(
    kind: &Kind,
    value: &Value,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    match (kind, value) {
        (Kind::Bool, Value::Bool(b)) => serializer.serialize_bool(*b),
        (Kind::Int32 | Kind::Sint32 | Kind::Sfixed32, Value::I32(v)) => {
            serializer.serialize_i32(*v)
        }
        (Kind::Uint32 | Kind::Fixed32, Value::U32(v)) => serializer.serialize_u32(*v),
        (Kind::Int64 | Kind::Sint64 | Kind::Sfixed64, Value::I64(v)) => {
            if options.stringify_64_bit_integers {
                serializer.collect_str(&v)
            } else {
                serializer.serialize_i64(*v)
            }
        }
        (Kind::Uint64 | Kind::Fixed64, Value::U64(v)) => {
            if options.stringify_64_bit_integers {
                serializer.collect_str(&v)
            } else {
                serializer.serialize_u64(*v)
            }
        }
        (Kind::Float, Value::F32(v)) => serialize_float(serializer, *v as f64),
        (Kind::Double, Value::F64(v)) => serialize_float(serializer, *v),
        (Kind::String, Value::String(s)) => serializer.serialize_str(s),
        (Kind::Bytes, Value::Bytes(b)) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(b.as_ref());
            serializer.serialize_str(&encoded)
        }
        (Kind::Enum(enum_d), Value::EnumNumber(n)) => {
            if options.use_enum_numbers {
                serializer.serialize_i32(*n)
            } else if let Some(v) = enum_d.values().find(|v| v.number() == *n) {
                serializer.serialize_str(v.name())
            } else {
                // Forward-compat: unknown numbers serialize as numbers.
                serializer.serialize_i32(*n)
            }
        }
        (Kind::Message(_), Value::Message(m)) => m.serialize_with_options(serializer, options),
        _ => Err(S::Error::custom(format!(
            "value/kind mismatch: kind={kind:?} value={value:?}"
        ))),
    }
}

fn serialize_float<S: Serializer>(serializer: S, v: f64) -> Result<S::Ok, S::Error> {
    if v.is_nan() {
        serializer.serialize_str("NaN")
    } else if v == f64::INFINITY {
        serializer.serialize_str("Infinity")
    } else if v == f64::NEG_INFINITY {
        serializer.serialize_str("-Infinity")
    } else {
        serializer.serialize_f64(v)
    }
}

fn serialize_list_value<S: Serializer>(
    field: &FieldDescriptor,
    value: &Value,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let items = value.as_list().unwrap_or(&[]);
    let mut seq = serializer.serialize_seq(Some(items.len()))?;
    let kind = field.kind();
    for item in items {
        seq.serialize_element(&SerializeScalarValue {
            kind: &kind,
            value: item,
            options,
        })?;
    }
    seq.end()
}

fn serialize_map_value<S: Serializer>(
    field: &FieldDescriptor,
    value: &Value,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let entries = match value.as_map() {
        Some(m) => m,
        None => return serializer.serialize_map(Some(0))?.end(),
    };
    let (key_kind, value_kind) = match crate::dynamic::value::map_entry_kinds(field) {
        Some(k) => k,
        None => return Err(S::Error::custom("map field without map-entry descriptor")),
    };
    let mut map = serializer.serialize_map(Some(entries.len()))?;
    // Sort keys for determinism.
    let mut sorted: Vec<(&MapKey, &Value)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (k, v) in sorted {
        let key = map_key_to_string(k, &key_kind);
        map.serialize_entry(
            &key,
            &SerializeScalarValue {
                kind: &value_kind,
                value: v,
                options,
            },
        )?;
    }
    map.end()
}

fn map_key_to_string(key: &MapKey, _kind: &Kind) -> String {
    match key {
        MapKey::Bool(b) => b.to_string(),
        MapKey::I32(v) => v.to_string(),
        MapKey::I64(v) => v.to_string(),
        MapKey::U32(v) => v.to_string(),
        MapKey::U64(v) => v.to_string(),
        MapKey::String(s) => s.clone(),
    }
}

/// Light-weight reusable `Serialize` over `(kind, value)` for elements
/// inside a list / map.
pub(super) struct SerializeScalarValue<'a> {
    pub(super) kind: &'a Kind,
    pub(super) value: &'a Value,
    pub(super) options: &'a SerializeOptions,
}

impl serde::Serialize for SerializeScalarValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_singular_value(self.kind, self.value, serializer, self.options)
    }
}
