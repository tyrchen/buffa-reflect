//! `serde::Deserialize` driver for [`crate::DynamicMessage`].

mod wkt;

use std::collections::HashMap;
use std::fmt;

use base64::Engine as _;
use buffa::bytes::Bytes;
use serde::de::{Deserializer, Error as _, MapAccess, SeqAccess, Visitor};

use crate::dynamic::message::DynamicMessage;
use crate::dynamic::serde::DeserializeOptions;
use crate::dynamic::value::{MapKey, Value};
use crate::field::{FieldDescriptor, Kind};
use crate::message::MessageDescriptor;

pub(crate) fn deserialize_message<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    let full_name = descriptor.full_name().to_string();
    if super::is_well_known_type(&full_name) {
        return wkt::deserialize_wkt(&full_name, descriptor, deserializer, options);
    }
    deserialize_plain_message(descriptor, deserializer, options)
}

pub(super) fn deserialize_plain_message<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    deserializer.deserialize_map(MessageVisitor {
        descriptor: descriptor.clone(),
        options: options.clone(),
    })
}

struct MessageVisitor {
    descriptor: MessageDescriptor,
    options: DeserializeOptions,
}

impl<'de> Visitor<'de> for MessageVisitor {
    type Value = DynamicMessage;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON object for {}", self.descriptor.full_name())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        while let Some(key) = access.next_key::<String>()? {
            let field = self
                .descriptor
                .get_field_by_json_name(&key)
                .or_else(|| self.descriptor.get_field_by_name(&key));
            match field {
                Some(fd) => {
                    let v = access.next_value_seed(FieldSeed {
                        field: &fd,
                        options: &self.options,
                    })?;
                    msg.try_set_field(&fd, v).map_err(|e| {
                        A::Error::custom(format!("invalid value for `{}`: {e}", fd.full_name()))
                    })?;
                }
                None => {
                    if self.options.deny_unknown_fields {
                        return Err(A::Error::unknown_field(&key, &[]));
                    }
                    let _ignored: serde::de::IgnoredAny = access.next_value()?;
                }
            }
        }
        Ok(msg)
    }
}

pub(super) struct FieldSeed<'a> {
    pub(super) field: &'a FieldDescriptor,
    pub(super) options: &'a DeserializeOptions,
}

impl<'de> serde::de::DeserializeSeed<'de> for FieldSeed<'_> {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        if self.field.is_map() {
            return deserialize_map_value(self.field, deserializer, self.options);
        }
        if self.field.is_list() {
            return deserialize_list_value(self.field, deserializer, self.options);
        }
        deserialize_singular_value(&self.field.kind(), deserializer, self.options)
    }
}

pub(super) fn deserialize_singular_value<'de, D: Deserializer<'de>>(
    kind: &Kind,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<Value, D::Error> {
    match kind {
        Kind::Bool => deserializer.deserialize_any(SingularVisitor::Bool),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => {
            deserializer.deserialize_any(SingularVisitor::I32)
        }
        Kind::Uint32 | Kind::Fixed32 => deserializer.deserialize_any(SingularVisitor::U32),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => {
            deserializer.deserialize_any(SingularVisitor::I64)
        }
        Kind::Uint64 | Kind::Fixed64 => deserializer.deserialize_any(SingularVisitor::U64),
        Kind::Float => deserializer.deserialize_any(SingularVisitor::F32),
        Kind::Double => deserializer.deserialize_any(SingularVisitor::F64),
        Kind::String => deserializer.deserialize_string(SingularVisitor::Str),
        Kind::Bytes => deserializer.deserialize_string(SingularVisitor::Bytes),
        Kind::Enum(enum_d) => {
            let visitor = EnumVisitor {
                enum_d: enum_d.clone(),
            };
            deserializer.deserialize_any(visitor)
        }
        Kind::Message(d) => deserialize_message(d, deserializer, options).map(Value::Message),
    }
}

enum SingularVisitor {
    Bool,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Str,
    Bytes,
}

impl<'de> Visitor<'de> for SingularVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scalar value")
    }

    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
        match self {
            Self::Bool => Ok(Value::Bool(v)),
            _ => Err(E::custom("unexpected bool")),
        }
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        match self {
            Self::I32 => i32::try_from(v)
                .map(Value::I32)
                .map_err(|_| E::custom("int32 out of range")),
            Self::U32 => u32::try_from(v)
                .map(Value::U32)
                .map_err(|_| E::custom("uint32 out of range")),
            Self::I64 => Ok(Value::I64(v)),
            Self::U64 => u64::try_from(v)
                .map(Value::U64)
                .map_err(|_| E::custom("uint64 out of range")),
            Self::F32 => Ok(Value::F32(v as f32)),
            Self::F64 => Ok(Value::F64(v as f64)),
            _ => Err(E::custom("unexpected integer")),
        }
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        match self {
            Self::I32 => i32::try_from(v)
                .map(Value::I32)
                .map_err(|_| E::custom("int32 out of range")),
            Self::U32 => u32::try_from(v)
                .map(Value::U32)
                .map_err(|_| E::custom("uint32 out of range")),
            Self::I64 => i64::try_from(v)
                .map(Value::I64)
                .map_err(|_| E::custom("int64 out of range")),
            Self::U64 => Ok(Value::U64(v)),
            Self::F32 => Ok(Value::F32(v as f32)),
            Self::F64 => Ok(Value::F64(v as f64)),
            _ => Err(E::custom("unexpected unsigned")),
        }
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        match self {
            Self::F32 => Ok(Value::F32(v as f32)),
            Self::F64 => Ok(Value::F64(v)),
            Self::I32 => Ok(Value::I32(v as i32)),
            Self::I64 => Ok(Value::I64(v as i64)),
            Self::U32 => Ok(Value::U32(v as u32)),
            Self::U64 => Ok(Value::U64(v as u64)),
            _ => Err(E::custom("unexpected float")),
        }
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.visit_string(v.to_owned())
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        match self {
            Self::Str => Ok(Value::String(v)),
            Self::Bytes => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(v.as_bytes())
                    .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(v.as_bytes()))
                    .map_err(|e| E::custom(format!("invalid base64: {e}")))?;
                Ok(Value::Bytes(Bytes::from(bytes)))
            }
            Self::I32 => v
                .parse::<i32>()
                .map(Value::I32)
                .map_err(|e| E::custom(format!("invalid int32 string: {e}"))),
            Self::U32 => v
                .parse::<u32>()
                .map(Value::U32)
                .map_err(|e| E::custom(format!("invalid uint32 string: {e}"))),
            Self::I64 => v
                .parse::<i64>()
                .map(Value::I64)
                .map_err(|e| E::custom(format!("invalid int64 string: {e}"))),
            Self::U64 => v
                .parse::<u64>()
                .map(Value::U64)
                .map_err(|e| E::custom(format!("invalid uint64 string: {e}"))),
            Self::F32 | Self::F64 => {
                let f = match v.as_str() {
                    "NaN" => f64::NAN,
                    "Infinity" => f64::INFINITY,
                    "-Infinity" => f64::NEG_INFINITY,
                    other => other
                        .parse::<f64>()
                        .map_err(|e| E::custom(format!("invalid float string: {e}")))?,
                };
                Ok(if matches!(self, Self::F32) {
                    Value::F32(f as f32)
                } else {
                    Value::F64(f)
                })
            }
            Self::Bool => match v.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                other => Err(E::custom(format!("invalid bool string `{other}`"))),
            },
        }
    }
}

struct EnumVisitor {
    enum_d: crate::EnumDescriptor,
}

impl<'de> Visitor<'de> for EnumVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "enum value (string or number)")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match self.enum_d.values().find(|x| x.name() == v) {
            Some(val) => Ok(Value::EnumNumber(val.number())),
            None => Err(E::custom(format!(
                "unknown enum variant `{v}` for `{}`",
                self.enum_d.full_name()
            ))),
        }
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        self.visit_str(&v)
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Value::EnumNumber(v as i32))
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Value::EnumNumber(v as i32))
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Ok(Value::EnumNumber(v as i32))
    }
}

fn deserialize_list_value<'de, D: Deserializer<'de>>(
    field: &FieldDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<Value, D::Error> {
    deserializer.deserialize_seq(ListVisitor {
        kind: field.kind(),
        options: options.clone(),
    })
}

struct ListVisitor {
    kind: Kind,
    options: DeserializeOptions,
}

impl<'de> Visitor<'de> for ListVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::new();
        while let Some(v) = access.next_element_seed(ScalarSeed {
            kind: &self.kind,
            options: &self.options,
        })? {
            items.push(v);
        }
        Ok(Value::List(items))
    }
}

fn deserialize_map_value<'de, D: Deserializer<'de>>(
    field: &FieldDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<Value, D::Error> {
    let (key_kind, value_kind) = crate::dynamic::value::map_entry_kinds(field)
        .ok_or_else(|| D::Error::custom("map field without map-entry descriptor"))?;
    deserializer.deserialize_map(MapVisitor {
        key_kind,
        value_kind,
        options: options.clone(),
    })
}

struct MapVisitor {
    key_kind: Kind,
    value_kind: Kind,
    options: DeserializeOptions,
}

impl<'de> Visitor<'de> for MapVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON object as map")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut out = HashMap::new();
        while let Some(key) = access.next_key::<String>()? {
            let mk = parse_map_key(&key, &self.key_kind)
                .ok_or_else(|| A::Error::custom(format!("invalid map key `{key}`")))?;
            let v = access.next_value_seed(ScalarSeed {
                kind: &self.value_kind,
                options: &self.options,
            })?;
            out.insert(mk, v);
        }
        Ok(Value::Map(out))
    }
}

fn parse_map_key(s: &str, kind: &Kind) -> Option<MapKey> {
    Some(match kind {
        Kind::String => MapKey::String(s.to_owned()),
        Kind::Bool => MapKey::Bool(s == "true"),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => MapKey::I32(s.parse().ok()?),
        Kind::Uint32 | Kind::Fixed32 => MapKey::U32(s.parse().ok()?),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => MapKey::I64(s.parse().ok()?),
        Kind::Uint64 | Kind::Fixed64 => MapKey::U64(s.parse().ok()?),
        _ => return None,
    })
}

pub(super) struct ScalarSeed<'a> {
    pub(super) kind: &'a Kind,
    pub(super) options: &'a DeserializeOptions,
}

impl<'de> serde::de::DeserializeSeed<'de> for ScalarSeed<'_> {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserialize_singular_value(self.kind, deserializer, self.options)
    }
}
