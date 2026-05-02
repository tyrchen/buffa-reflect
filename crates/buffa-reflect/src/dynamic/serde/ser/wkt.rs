//! Well-known type JSON serializers.
//!
//! Hard-coded dispatch by `full_name` mirroring the proto3 JSON spec
//! and prost-reflect's table.

use base64::Engine as _;
use serde::ser::{Error as _, SerializeMap as _, SerializeSeq as _, Serializer};

use crate::dynamic::message::DynamicMessage;
use crate::dynamic::serde::SerializeOptions;
use crate::dynamic::serde::case::snake_to_lower_camel;
use crate::dynamic::value::Value;
use crate::field::Kind;

pub(super) fn serialize_wkt<S: Serializer>(
    full_name: &str,
    msg: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    match full_name {
        "google.protobuf.Empty" => serializer.serialize_map(Some(0))?.end(),
        "google.protobuf.Timestamp" => serialize_timestamp(msg, serializer),
        "google.protobuf.Duration" => serialize_duration(msg, serializer),
        "google.protobuf.FieldMask" => serialize_field_mask(msg, serializer),
        "google.protobuf.Struct" => serialize_struct(msg, serializer, options),
        "google.protobuf.ListValue" => serialize_list_value(msg, serializer, options),
        "google.protobuf.Value" => serialize_value(msg, serializer, options),
        "google.protobuf.Any" => serialize_any(msg, serializer, options),
        "google.protobuf.BoolValue"
        | "google.protobuf.StringValue"
        | "google.protobuf.BytesValue"
        | "google.protobuf.Int32Value"
        | "google.protobuf.Int64Value"
        | "google.protobuf.UInt32Value"
        | "google.protobuf.UInt64Value"
        | "google.protobuf.FloatValue"
        | "google.protobuf.DoubleValue" => serialize_scalar_wrapper(msg, serializer, options),
        _ => super::serialize_plain_message(msg, serializer, options),
    }
}

fn read_i64(msg: &DynamicMessage, name: &str) -> i64 {
    msg.get_field_by_name(name)
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

fn read_i32(msg: &DynamicMessage, name: &str) -> i32 {
    msg.get_field_by_name(name)
        .and_then(|v| v.as_i32())
        .unwrap_or(0)
}

fn serialize_timestamp<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let seconds = read_i64(msg, "seconds");
    let nanos = read_i32(msg, "nanos");
    let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(seconds, nanos as u32)
        .ok_or_else(|| S::Error::custom("timestamp out of range"))?;
    let s = datetime.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    serializer.serialize_str(&s)
}

fn serialize_duration<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let seconds = read_i64(msg, "seconds");
    let nanos = read_i32(msg, "nanos");
    let mut s = String::new();
    if nanos == 0 {
        s = format!("{seconds}s");
    } else {
        let neg = seconds < 0 || nanos < 0;
        let abs_seconds = seconds.unsigned_abs();
        let abs_nanos = nanos.unsigned_abs();
        let frac = format!("{abs_nanos:09}");
        let frac = frac.trim_end_matches('0');
        if neg {
            s.push('-');
        }
        s.push_str(&abs_seconds.to_string());
        if !frac.is_empty() {
            s.push('.');
            s.push_str(frac);
        }
        s.push('s');
    }
    serializer.serialize_str(&s)
}

fn serialize_field_mask<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let paths = msg.get_field_by_name("paths");
    let parts: Vec<String> = match paths {
        Some(v) => v
            .as_list()
            .unwrap_or(&[])
            .iter()
            .filter_map(|p| p.as_str().map(snake_to_lower_camel))
            .collect(),
        None => Vec::new(),
    };
    serializer.serialize_str(&parts.join(","))
}

fn serialize_struct<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let fields = msg.get_field_by_name("fields");
    let entries = match fields.as_ref().and_then(|v| v.as_map()) {
        Some(m) => m,
        None => return serializer.serialize_map(Some(0))?.end(),
    };
    let mut sorted: Vec<_> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut map = serializer.serialize_map(Some(sorted.len()))?;
    for (k, v) in sorted {
        let key = match k {
            crate::dynamic::value::MapKey::String(s) => s.clone(),
            other => format!("{other:?}"),
        };
        if let Value::Message(inner) = v {
            map.serialize_entry(
                &key,
                &WktSerializeMessage {
                    msg: inner,
                    options,
                },
            )?;
        }
    }
    map.end()
}

fn serialize_list_value<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let values = msg.get_field_by_name("values");
    let items = values
        .as_ref()
        .and_then(|v| v.as_list())
        .unwrap_or(&[])
        .to_vec();
    let mut seq = serializer.serialize_seq(Some(items.len()))?;
    for v in &items {
        if let Value::Message(inner) = v {
            seq.serialize_element(&WktSerializeMessage {
                msg: inner,
                options,
            })?;
        }
    }
    seq.end()
}

fn serialize_value<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    // `Value` is a oneof of {null_value, number_value, string_value,
    // bool_value, struct_value, list_value}. Find the populated one.
    let descriptor = msg.descriptor();
    if msg.has_field_by_name("null_value") {
        return serializer.serialize_unit();
    }
    if msg.has_field_by_name("number_value") {
        let n = msg
            .get_field_by_name("number_value")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if !n.is_finite() {
            return Err(S::Error::custom(
                "cannot serialize non-finite double in google.protobuf.Value",
            ));
        }
        return serializer.serialize_f64(n);
    }
    if msg.has_field_by_name("string_value") {
        let s = msg
            .get_field_by_name("string_value")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        return serializer.serialize_str(&s);
    }
    if msg.has_field_by_name("bool_value") {
        let b = msg
            .get_field_by_name("bool_value")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        return serializer.serialize_bool(b);
    }
    if let Some(struct_field) = descriptor.get_field_by_name("struct_value")
        && msg.has_field_by_name("struct_value")
    {
        let v = msg.get_field(&struct_field);
        if let Value::Message(inner) = v.as_ref() {
            return serialize_struct(inner, serializer, options);
        }
    }
    if let Some(list_field) = descriptor.get_field_by_name("list_value")
        && msg.has_field_by_name("list_value")
    {
        let v = msg.get_field(&list_field);
        if let Value::Message(inner) = v.as_ref() {
            return serialize_list_value(inner, serializer, options);
        }
    }
    serializer.serialize_unit()
}

fn serialize_any<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let type_url = msg
        .get_field_by_name("type_url")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let value_bytes = msg
        .get_field_by_name("value")
        .and_then(|v| v.as_bytes().cloned())
        .unwrap_or_default();
    let inner_name = type_url.rsplit('/').next().unwrap_or(&type_url).to_string();
    let pool = msg.parent_pool();
    let inner_desc = pool.get_message_by_name(&inner_name).ok_or_else(|| {
        S::Error::custom(format!("Any payload type `{inner_name}` not found in pool"))
    })?;
    let inner = DynamicMessage::decode(inner_desc, value_bytes.as_ref())
        .map_err(|e| S::Error::custom(format!("Any payload decode failed: {e}")))?;
    let inner_full_name = inner.descriptor().full_name().to_string();

    if super::super::is_well_known_type(&inner_full_name)
        && inner_full_name != "google.protobuf.Any"
    {
        // WKT inside Any: emit `{ "@type": "...", "value": <wkt> }`.
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("@type", &type_url)?;
        map.serialize_entry(
            "value",
            &WktSerializeMessage {
                msg: &inner,
                options,
            },
        )?;
        map.end()
    } else {
        // Plain message inside Any: inline its fields with `@type` first.
        let descriptor = inner.descriptor();
        let mut entries: Vec<(String, Value)> = Vec::new();
        for field in descriptor.fields() {
            if options.skip_default_fields && !inner.has_field(&field) {
                continue;
            }
            let key = if options.use_proto_field_name {
                field.name().to_string()
            } else {
                field.json_name().to_string()
            };
            let value = inner.get_field(&field).into_owned();
            entries.push((key, value));
        }
        let mut map = serializer.serialize_map(Some(entries.len() + 1))?;
        map.serialize_entry("@type", &type_url)?;
        for (key, value) in &entries {
            // Re-resolve the field from the inner descriptor so kind+repeated info is correct.
            if let Some(field) = inner.descriptor().fields().find(|f| {
                if options.use_proto_field_name {
                    f.name() == key.as_str()
                } else {
                    f.json_name() == key.as_str()
                }
            }) {
                map.serialize_entry(
                    key,
                    &super::SerializeFieldValue {
                        field: &field,
                        value,
                        options,
                    },
                )?;
            }
        }
        map.end()
    }
}

fn serialize_scalar_wrapper<S: Serializer>(
    msg: &DynamicMessage,
    serializer: S,
    options: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    let descriptor = msg.descriptor();
    let value_field = descriptor
        .get_field_by_name("value")
        .ok_or_else(|| S::Error::custom("wrapper missing `value` field"))?;
    let v = msg.get_field(&value_field);
    let kind = value_field.kind();
    match (&kind, v.as_ref()) {
        (Kind::String, Value::String(s)) => serializer.serialize_str(s),
        (Kind::Bytes, Value::Bytes(b)) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(b.as_ref());
            serializer.serialize_str(&encoded)
        }
        (Kind::Bool, Value::Bool(b)) => serializer.serialize_bool(*b),
        (Kind::Int32, Value::I32(v)) => serializer.serialize_i32(*v),
        (Kind::Uint32, Value::U32(v)) => serializer.serialize_u32(*v),
        (Kind::Int64, Value::I64(v)) => {
            if options.stringify_64_bit_integers {
                serializer.collect_str(v)
            } else {
                serializer.serialize_i64(*v)
            }
        }
        (Kind::Uint64, Value::U64(v)) => {
            if options.stringify_64_bit_integers {
                serializer.collect_str(v)
            } else {
                serializer.serialize_u64(*v)
            }
        }
        (Kind::Float, Value::F32(v)) => {
            super::serialize_singular_value(&Kind::Float, &Value::F32(*v), serializer, options)
        }
        (Kind::Double, Value::F64(v)) => {
            super::serialize_singular_value(&Kind::Double, &Value::F64(*v), serializer, options)
        }
        _ => Err(S::Error::custom("scalar wrapper kind/value mismatch")),
    }
}

struct WktSerializeMessage<'a> {
    msg: &'a DynamicMessage,
    options: &'a SerializeOptions,
}

impl serde::Serialize for WktSerializeMessage<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        super::serialize_message(self.msg, serializer, self.options)
    }
}
