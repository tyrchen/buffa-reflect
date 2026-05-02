//! Wire-format decode dispatch for [`super::DynamicMessage`].

use std::collections::HashMap;

use buffa::{
    DecodeError,
    bytes::{Buf, Bytes},
    encoding::{Tag, WireType, decode_unknown_field, decode_varint},
};

use crate::{
    dynamic::{
        message::DynamicMessage,
        value::{MapKey, Value},
    },
    field::{FieldDescriptor, Kind},
};

/// Top-level merge entry. Iterates `(tag, payload)` pairs and
/// dispatches per field. `depth` is the remaining recursion budget;
/// it is **not** decremented here — the only decrement happens when
/// entering a sub-message (`read_singular` for `Kind::Message`),
/// matching buffa's typed `merge_length_delimited`.
pub(super) fn merge<B: Buf>(
    msg: &mut DynamicMessage,
    buf: &mut B,
    depth: u32,
) -> Result<(), DecodeError> {
    while buf.has_remaining() {
        let tag = Tag::decode(buf)?;
        let number = tag.field_number();
        let wire = tag.wire_type();
        match msg.descriptor().get_field_by_number(number) {
            Some(field) => merge_field(msg, &field, wire, buf, depth)?,
            None => {
                let unknown = decode_unknown_field(tag, buf, depth)?;
                msg.fields_set_mut().add_unknown(number, unknown);
            }
        }
    }
    Ok(())
}

fn merge_field<B: Buf>(
    msg: &mut DynamicMessage,
    field: &FieldDescriptor,
    wire: WireType,
    buf: &mut B,
    depth: u32,
) -> Result<(), DecodeError> {
    if field.is_map() {
        return merge_map_entry(msg, field, wire, buf, depth);
    }
    if field.is_list() {
        return merge_list(msg, field, wire, buf, depth);
    }
    let v = read_singular(&field.kind(), wire, buf, depth)?;
    msg.set_field(field, v);
    Ok(())
}

fn merge_list<B: Buf>(
    msg: &mut DynamicMessage,
    field: &FieldDescriptor,
    wire: WireType,
    buf: &mut B,
    depth: u32,
) -> Result<(), DecodeError> {
    let kind = field.kind();
    // Packed body = LengthDelimited where the field's natural wire
    // type isn't LengthDelimited.
    if wire == WireType::LengthDelimited && wire_type_for(&kind) != WireType::LengthDelimited {
        let len_u64 = decode_varint(buf)?;
        let len = usize::try_from(len_u64).map_err(|_| DecodeError::MessageTooLarge)?;
        if buf.remaining() < len {
            return Err(DecodeError::UnexpectedEof);
        }
        // Take the packed slice as a sub-buffer.
        let limit = buf.remaining() - len;
        ensure_list_slot(msg, field);
        while buf.remaining() > limit {
            let v = read_singular(&kind, wire_type_for(&kind), buf, depth)?;
            push_list_item(msg, field, v);
        }
        if buf.remaining() != limit {
            return Err(DecodeError::UnexpectedEof);
        }
        return Ok(());
    }

    let v = read_singular(&kind, wire, buf, depth)?;
    ensure_list_slot(msg, field);
    push_list_item(msg, field, v);
    Ok(())
}

fn ensure_list_slot(msg: &mut DynamicMessage, field: &FieldDescriptor) {
    let n = field.number();
    if !msg.fields_set_ref().has_value(n) {
        let empty = Value::List(Vec::new());
        msg.fields_set_mut().set(field, empty);
    }
}

fn push_list_item(msg: &mut DynamicMessage, field: &FieldDescriptor, item: Value) {
    if let Some(Value::List(list)) = msg.fields_set_mut().get_value_mut(field.number()) {
        list.push(item);
    }
}

fn merge_map_entry<B: Buf>(
    msg: &mut DynamicMessage,
    field: &FieldDescriptor,
    wire: WireType,
    buf: &mut B,
    depth: u32,
) -> Result<(), DecodeError> {
    if wire != WireType::LengthDelimited {
        return Err(DecodeError::InvalidWireType(wire as u8 as u32));
    }
    let len_u64 = decode_varint(buf)?;
    let len = usize::try_from(len_u64).map_err(|_| DecodeError::MessageTooLarge)?;
    if buf.remaining() < len {
        return Err(DecodeError::UnexpectedEof);
    }
    let limit = buf.remaining() - len;

    let Kind::Message(entry_desc) = field.kind() else {
        return Err(DecodeError::InvalidWireType(wire as u8 as u32));
    };
    let key_field = entry_desc
        .get_field_by_number(1)
        .ok_or(DecodeError::InvalidWireType(0))?;
    let value_field = entry_desc
        .get_field_by_number(2)
        .ok_or(DecodeError::InvalidWireType(0))?;
    let key_kind = key_field.kind();
    let value_kind = value_field.kind();

    let mut key: Option<Value> = None;
    let mut value: Option<Value> = None;

    while buf.remaining() > limit {
        let inner_tag = Tag::decode(buf)?;
        let inner_wire = inner_tag.wire_type();
        match inner_tag.field_number() {
            1 => key = Some(read_singular(&key_kind, inner_wire, buf, depth)?),
            2 => value = Some(read_singular(&value_kind, inner_wire, buf, depth)?),
            _ => buffa::encoding::skip_field_depth(inner_tag, buf, depth)?,
        }
    }
    if buf.remaining() != limit {
        return Err(DecodeError::UnexpectedEof);
    }

    // Default any missing component (proto2/proto3 maps default both halves).
    let key = key.unwrap_or_else(|| Value::default_value(&key_kind));
    let value = value.unwrap_or_else(|| Value::default_value(&value_kind));
    let mk = value_to_map_key(&key).ok_or(DecodeError::InvalidWireType(0))?;

    ensure_map_slot(msg, field);
    if let Some(Value::Map(m)) = msg.fields_set_mut().get_value_mut(field.number()) {
        m.insert(mk, value);
    }
    Ok(())
}

fn ensure_map_slot(msg: &mut DynamicMessage, field: &FieldDescriptor) {
    let n = field.number();
    if !msg.fields_set_ref().has_value(n) {
        msg.fields_set_mut().set(field, Value::Map(HashMap::new()));
    }
}

fn value_to_map_key(value: &Value) -> Option<MapKey> {
    Some(match value {
        Value::Bool(b) => MapKey::Bool(*b),
        Value::I32(v) => MapKey::I32(*v),
        Value::I64(v) => MapKey::I64(*v),
        Value::U32(v) => MapKey::U32(*v),
        Value::U64(v) => MapKey::U64(*v),
        Value::String(s) => MapKey::String(s.clone()),
        _ => return None,
    })
}

fn read_singular<B: Buf>(
    kind: &Kind,
    wire: WireType,
    buf: &mut B,
    depth: u32,
) -> Result<Value, DecodeError> {
    match (kind, wire) {
        (Kind::Bool, WireType::Varint) => Ok(Value::Bool(decode_varint(buf)? != 0)),
        (Kind::Int32, WireType::Varint) => Ok(Value::I32(decode_varint(buf)? as u32 as i32)),
        (Kind::Int64, WireType::Varint) => Ok(Value::I64(decode_varint(buf)? as i64)),
        (Kind::Uint32, WireType::Varint) => Ok(Value::U32(decode_varint(buf)? as u32)),
        (Kind::Uint64, WireType::Varint) => Ok(Value::U64(decode_varint(buf)?)),
        (Kind::Sint32, WireType::Varint) => {
            let raw = decode_varint(buf)?;
            Ok(Value::I32(unzigzag32(raw as u32)))
        }
        (Kind::Sint64, WireType::Varint) => {
            let raw = decode_varint(buf)?;
            Ok(Value::I64(unzigzag64(raw)))
        }
        (Kind::Enum(_), WireType::Varint) => {
            let raw = decode_varint(buf)?;
            Ok(Value::EnumNumber(raw as u32 as i32))
        }
        (Kind::Fixed64, WireType::Fixed64) => {
            ensure(buf, 8)?;
            Ok(Value::U64(buf.get_u64_le()))
        }
        (Kind::Sfixed64, WireType::Fixed64) => {
            ensure(buf, 8)?;
            Ok(Value::I64(buf.get_i64_le()))
        }
        (Kind::Double, WireType::Fixed64) => {
            ensure(buf, 8)?;
            Ok(Value::F64(buf.get_f64_le()))
        }
        (Kind::Fixed32, WireType::Fixed32) => {
            ensure(buf, 4)?;
            Ok(Value::U32(buf.get_u32_le()))
        }
        (Kind::Sfixed32, WireType::Fixed32) => {
            ensure(buf, 4)?;
            Ok(Value::I32(buf.get_i32_le()))
        }
        (Kind::Float, WireType::Fixed32) => {
            ensure(buf, 4)?;
            Ok(Value::F32(buf.get_f32_le()))
        }
        (Kind::String, WireType::LengthDelimited) => {
            let bytes = read_length_delimited(buf)?;
            String::from_utf8(bytes.to_vec())
                .map(Value::String)
                .map_err(|_| DecodeError::InvalidUtf8)
        }
        (Kind::Bytes, WireType::LengthDelimited) => Ok(Value::Bytes(read_length_delimited(buf)?)),
        (Kind::Message(d), WireType::LengthDelimited) => {
            let len_u64 = decode_varint(buf)?;
            let len = usize::try_from(len_u64).map_err(|_| DecodeError::MessageTooLarge)?;
            if buf.remaining() < len {
                return Err(DecodeError::UnexpectedEof);
            }
            let limit = buf.remaining() - len;
            let mut inner = DynamicMessage::new(d.clone());
            // recurse — depth was already decremented at the outer
            // merge entry.
            let depth = depth
                .checked_sub(1)
                .ok_or(DecodeError::RecursionLimitExceeded)?;
            while buf.remaining() > limit {
                let tag = Tag::decode(buf)?;
                let wire = tag.wire_type();
                match d.get_field_by_number(tag.field_number()) {
                    Some(f) => merge_field(&mut inner, &f, wire, buf, depth)?,
                    None => {
                        let unknown = decode_unknown_field(tag, buf, depth)?;
                        inner
                            .fields_set_mut()
                            .add_unknown(tag.field_number(), unknown);
                    }
                }
            }
            if buf.remaining() != limit {
                return Err(DecodeError::UnexpectedEof);
            }
            Ok(Value::Message(inner))
        }
        (_, _) => Err(DecodeError::InvalidWireType(wire as u8 as u32)),
    }
}

fn ensure<B: Buf>(buf: &B, n: usize) -> Result<(), DecodeError> {
    if buf.remaining() < n {
        Err(DecodeError::UnexpectedEof)
    } else {
        Ok(())
    }
}

fn read_length_delimited<B: Buf>(buf: &mut B) -> Result<Bytes, DecodeError> {
    let len_u64 = decode_varint(buf)?;
    let len = usize::try_from(len_u64).map_err(|_| DecodeError::MessageTooLarge)?;
    if buf.remaining() < len {
        return Err(DecodeError::UnexpectedEof);
    }
    let mut out = vec![0u8; len];
    buf.copy_to_slice(&mut out);
    Ok(Bytes::from(out))
}

#[inline]
fn unzigzag32(v: u32) -> i32 {
    ((v >> 1) as i32) ^ -((v & 1) as i32)
}

#[inline]
fn unzigzag64(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
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
