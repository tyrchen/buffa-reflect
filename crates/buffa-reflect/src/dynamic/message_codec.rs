//! Wire-format encode dispatch for [`super::DynamicMessage`].

use buffa::{
    EncodeError,
    bytes::BufMut,
    encoding::{Tag, WireType, encode_varint, varint_len},
};

use crate::{
    dynamic::{
        fields::ValueOrUnknown,
        message::DynamicMessage,
        value::{MapKey, Value},
    },
    field::{FieldDescriptor, Kind},
};

/// Compute the encoded byte length of `msg` (known + unknown fields).
pub(super) fn encoded_len(msg: &DynamicMessage) -> usize {
    let mut size = 0;
    for (number, entry) in msg.iter_storage() {
        match entry {
            ValueOrUnknown::Value(v) => {
                if let Some(field) = msg.descriptor().get_field_by_number(number) {
                    size += encoded_field_len(&field, v);
                }
            }
            ValueOrUnknown::Unknown(set) => {
                size += set.encoded_len(number);
            }
            ValueOrUnknown::Taken => {}
        }
    }
    size
}

/// Encode `msg` to `buf` (known + unknown fields, interleaved by number).
pub(super) fn encode<B: BufMut>(msg: &DynamicMessage, buf: &mut B) -> Result<(), EncodeError> {
    for (number, entry) in msg.iter_storage() {
        match entry {
            ValueOrUnknown::Value(v) => {
                if let Some(field) = msg.descriptor().get_field_by_number(number) {
                    encode_field(&field, v, buf)?;
                }
            }
            ValueOrUnknown::Unknown(set) => {
                set.encode(number, buf);
            }
            ValueOrUnknown::Taken => {}
        }
    }
    Ok(())
}

// ── per-field length / write ────────────────────────────────────────────

fn encoded_field_len(field: &FieldDescriptor, value: &Value) -> usize {
    if field.is_map() {
        if let Value::Map(entries) = value {
            return encoded_map_len(field, entries);
        }
        return 0;
    }
    if field.is_list() {
        if let Value::List(items) = value {
            return encoded_list_len(field, items);
        }
        return 0;
    }
    encoded_singular_len(field.number(), &field.kind(), value)
}

fn encode_field<B: BufMut>(
    field: &FieldDescriptor,
    value: &Value,
    buf: &mut B,
) -> Result<(), EncodeError> {
    if field.is_map() {
        if let Value::Map(entries) = value {
            encode_map(field, entries, buf)?;
        }
        return Ok(());
    }
    if field.is_list() {
        if let Value::List(items) = value {
            encode_list(field, items, buf)?;
        }
        return Ok(());
    }
    encode_singular(field.number(), &field.kind(), value, buf)
}

// ── singular ────────────────────────────────────────────────────────────

fn encoded_singular_len(number: u32, kind: &Kind, value: &Value) -> usize {
    let tag = tag_len(number, wire_type_for(kind));
    tag + value_payload_len(kind, value)
}

fn encode_singular<B: BufMut>(
    number: u32,
    kind: &Kind,
    value: &Value,
    buf: &mut B,
) -> Result<(), EncodeError> {
    Tag::new(number, wire_type_for(kind)).encode(buf);
    write_payload(kind, value, buf)
}

fn wire_type_for(kind: &Kind) -> WireType {
    match kind {
        Kind::Bool
        | Kind::Int32
        | Kind::Int64
        | Kind::Uint32
        | Kind::Uint64
        | Kind::Sint32
        | Kind::Sint64
        | Kind::Enum(_) => WireType::Varint,
        Kind::Fixed64 | Kind::Sfixed64 | Kind::Double => WireType::Fixed64,
        Kind::Fixed32 | Kind::Sfixed32 | Kind::Float => WireType::Fixed32,
        Kind::String | Kind::Bytes | Kind::Message(_) => WireType::LengthDelimited,
    }
}

fn tag_len(number: u32, wire_type: WireType) -> usize {
    let tag_value = ((number as u64) << 3) | (wire_type as u64);
    varint_len(tag_value)
}

fn value_payload_len(kind: &Kind, value: &Value) -> usize {
    match (kind, value) {
        (Kind::Bool, Value::Bool(b)) => varint_len(*b as u64),
        (Kind::Int32, Value::I32(v)) => varint_len(*v as u64),
        (Kind::Int64, Value::I64(v)) => varint_len(*v as u64),
        (Kind::Uint32, Value::U32(v)) => varint_len(*v as u64),
        (Kind::Uint64, Value::U64(v)) => varint_len(*v),
        (Kind::Sint32, Value::I32(v)) => varint_len(zigzag32(*v) as u64),
        (Kind::Sint64, Value::I64(v)) => varint_len(zigzag64(*v)),
        (Kind::Enum(_), Value::EnumNumber(v)) => varint_len(*v as i64 as u64),
        (Kind::Fixed64, Value::U64(_)) | (Kind::Sfixed64, Value::I64(_)) => 8,
        (Kind::Double, Value::F64(_)) => 8,
        (Kind::Fixed32, Value::U32(_)) | (Kind::Sfixed32, Value::I32(_)) => 4,
        (Kind::Float, Value::F32(_)) => 4,
        (Kind::String, Value::String(s)) => varint_len(s.len() as u64) + s.len(),
        (Kind::Bytes, Value::Bytes(b)) => varint_len(b.len() as u64) + b.len(),
        (Kind::Message(_), Value::Message(m)) => {
            let inner = m.encoded_len();
            varint_len(inner as u64) + inner
        }
        _ => 0,
    }
}

fn write_payload<B: BufMut>(kind: &Kind, value: &Value, buf: &mut B) -> Result<(), EncodeError> {
    match (kind, value) {
        (Kind::Bool, Value::Bool(b)) => encode_varint(*b as u64, buf),
        (Kind::Int32, Value::I32(v)) => encode_varint(*v as u64, buf),
        (Kind::Int64, Value::I64(v)) => encode_varint(*v as u64, buf),
        (Kind::Uint32, Value::U32(v)) => encode_varint(*v as u64, buf),
        (Kind::Uint64, Value::U64(v)) => encode_varint(*v, buf),
        (Kind::Sint32, Value::I32(v)) => encode_varint(zigzag32(*v) as u64, buf),
        (Kind::Sint64, Value::I64(v)) => encode_varint(zigzag64(*v), buf),
        (Kind::Enum(_), Value::EnumNumber(v)) => encode_varint(*v as i64 as u64, buf),
        (Kind::Fixed64, Value::U64(v)) => buf.put_u64_le(*v),
        (Kind::Sfixed64, Value::I64(v)) => buf.put_i64_le(*v),
        (Kind::Double, Value::F64(v)) => buf.put_f64_le(*v),
        (Kind::Fixed32, Value::U32(v)) => buf.put_u32_le(*v),
        (Kind::Sfixed32, Value::I32(v)) => buf.put_i32_le(*v),
        (Kind::Float, Value::F32(v)) => buf.put_f32_le(*v),
        (Kind::String, Value::String(s)) => {
            encode_varint(s.len() as u64, buf);
            buf.put_slice(s.as_bytes());
        }
        (Kind::Bytes, Value::Bytes(b)) => {
            encode_varint(b.len() as u64, buf);
            buf.put_slice(b.as_ref());
        }
        (Kind::Message(_), Value::Message(m)) => {
            let inner = m.encoded_len();
            encode_varint(inner as u64, buf);
            m.encode(buf)?;
        }
        _ => {} // mismatched value/kind: silent skip (validation upstream)
    }
    Ok(())
}

#[inline]
fn zigzag32(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

#[inline]
fn zigzag64(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

// ── repeated ────────────────────────────────────────────────────────────

fn encoded_list_len(field: &FieldDescriptor, items: &[Value]) -> usize {
    let kind = field.kind();
    if items.is_empty() {
        return 0;
    }
    if field.is_packed() && field.is_packable() {
        // Single tag + length + concatenated payloads.
        let body: usize = items.iter().map(|v| value_payload_len(&kind, v)).sum();
        tag_len(field.number(), WireType::LengthDelimited) + varint_len(body as u64) + body
    } else {
        // One tag-and-value per element.
        items
            .iter()
            .map(|v| encoded_singular_len(field.number(), &kind, v))
            .sum()
    }
}

fn encode_list<B: BufMut>(
    field: &FieldDescriptor,
    items: &[Value],
    buf: &mut B,
) -> Result<(), EncodeError> {
    if items.is_empty() {
        return Ok(());
    }
    let kind = field.kind();
    if field.is_packed() && field.is_packable() {
        Tag::new(field.number(), WireType::LengthDelimited).encode(buf);
        let body: usize = items.iter().map(|v| value_payload_len(&kind, v)).sum();
        encode_varint(body as u64, buf);
        for v in items {
            write_payload(&kind, v, buf)?;
        }
    } else {
        for v in items {
            encode_singular(field.number(), &kind, v, buf)?;
        }
    }
    Ok(())
}

// ── map ─────────────────────────────────────────────────────────────────

fn encoded_map_len(
    field: &FieldDescriptor,
    entries: &std::collections::HashMap<MapKey, Value>,
) -> usize {
    let Kind::Message(entry_desc) = field.kind() else {
        return 0;
    };
    let key_field = match entry_desc.get_field_by_number(1) {
        Some(f) => f,
        None => return 0,
    };
    let value_field = match entry_desc.get_field_by_number(2) {
        Some(f) => f,
        None => return 0,
    };
    let key_kind = key_field.kind();
    let value_kind = value_field.kind();

    let mut total = 0;
    for (k, v) in entries {
        let key_value = map_key_to_value(k);
        let inner = encoded_singular_len(1, &key_kind, &key_value)
            + encoded_singular_len(2, &value_kind, v);
        total +=
            tag_len(field.number(), WireType::LengthDelimited) + varint_len(inner as u64) + inner;
    }
    total
}

fn encode_map<B: BufMut>(
    field: &FieldDescriptor,
    entries: &std::collections::HashMap<MapKey, Value>,
    buf: &mut B,
) -> Result<(), EncodeError> {
    let Kind::Message(entry_desc) = field.kind() else {
        return Ok(());
    };
    let key_field = match entry_desc.get_field_by_number(1) {
        Some(f) => f,
        None => return Ok(()),
    };
    let value_field = match entry_desc.get_field_by_number(2) {
        Some(f) => f,
        None => return Ok(()),
    };
    let key_kind = key_field.kind();
    let value_kind = value_field.kind();

    // Sort by key for canonical output. Cheap relative to allocation
    // of the map itself, and required for deterministic textproto/JSON.
    let mut sorted: Vec<(&MapKey, &Value)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    for (k, v) in sorted {
        let key_value = map_key_to_value(k);
        let inner = encoded_singular_len(1, &key_kind, &key_value)
            + encoded_singular_len(2, &value_kind, v);
        Tag::new(field.number(), WireType::LengthDelimited).encode(buf);
        encode_varint(inner as u64, buf);
        encode_singular(1, &key_kind, &key_value, buf)?;
        encode_singular(2, &value_kind, v, buf)?;
    }
    Ok(())
}

fn map_key_to_value(key: &MapKey) -> Value {
    match key {
        MapKey::Bool(b) => Value::Bool(*b),
        MapKey::I32(v) => Value::I32(*v),
        MapKey::I64(v) => Value::I64(*v),
        MapKey::U32(v) => Value::U32(*v),
        MapKey::U64(v) => Value::U64(*v),
        MapKey::String(s) => Value::String(s.clone()),
    }
}
