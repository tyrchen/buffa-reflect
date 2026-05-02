//! Well-known type JSON deserializers.

use std::{collections::HashMap, fmt};

use buffa::bytes::Bytes;
use serde::{
    Deserialize as _,
    de::{Deserializer, Error as _, MapAccess, SeqAccess, Visitor},
};

use crate::{
    dynamic::{
        message::DynamicMessage,
        serde::{DeserializeOptions, case::lower_camel_to_snake},
        value::{MapKey, Value},
    },
    message::MessageDescriptor,
};

pub(super) fn deserialize_wkt<'de, D: Deserializer<'de>>(
    full_name: &str,
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    match full_name {
        "google.protobuf.Empty" => deserialize_empty(descriptor, deserializer),
        "google.protobuf.Timestamp" => deserialize_timestamp(descriptor, deserializer),
        "google.protobuf.Duration" => deserialize_duration(descriptor, deserializer),
        "google.protobuf.FieldMask" => deserialize_field_mask(descriptor, deserializer),
        "google.protobuf.Struct" => deserialize_struct(descriptor, deserializer, options),
        "google.protobuf.ListValue" => deserialize_list_value(descriptor, deserializer, options),
        "google.protobuf.Value" => deserialize_value(descriptor, deserializer, options),
        "google.protobuf.Any" => deserialize_any(descriptor, deserializer, options),
        "google.protobuf.BoolValue"
        | "google.protobuf.StringValue"
        | "google.protobuf.BytesValue"
        | "google.protobuf.Int32Value"
        | "google.protobuf.Int64Value"
        | "google.protobuf.UInt32Value"
        | "google.protobuf.UInt64Value"
        | "google.protobuf.FloatValue"
        | "google.protobuf.DoubleValue" => {
            deserialize_scalar_wrapper(descriptor, deserializer, options)
        }
        _ => super::deserialize_plain_message(descriptor, deserializer, options),
    }
}

fn deserialize_empty<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
) -> Result<DynamicMessage, D::Error> {
    let _ignored: serde::de::IgnoredAny = serde::Deserialize::deserialize(deserializer)?;
    Ok(DynamicMessage::new(descriptor.clone()))
}

fn deserialize_timestamp<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
) -> Result<DynamicMessage, D::Error> {
    let s = String::deserialize(deserializer)?;
    let dt = chrono::DateTime::parse_from_rfc3339(&s)
        .map_err(|e| D::Error::custom(format!("invalid timestamp `{s}`: {e}")))?
        .with_timezone(&chrono::Utc);
    let mut msg = DynamicMessage::new(descriptor.clone());
    msg.set_field_by_name("seconds", Value::I64(dt.timestamp()));
    msg.set_field_by_name("nanos", Value::I32(dt.timestamp_subsec_nanos() as i32));
    Ok(msg)
}

fn deserialize_duration<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
) -> Result<DynamicMessage, D::Error> {
    let s = String::deserialize(deserializer)?;
    let s = s
        .strip_suffix('s')
        .ok_or_else(|| D::Error::custom("duration missing trailing `s`"))?;
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1i64, r),
        None => (1i64, s),
    };
    let (secs_str, nanos_str) = match rest.split_once('.') {
        Some((s, n)) => (s, n),
        None => (rest, ""),
    };
    let secs: i64 = secs_str
        .parse()
        .map_err(|e| D::Error::custom(format!("invalid duration seconds: {e}")))?;
    let nanos = if nanos_str.is_empty() {
        0
    } else {
        let mut padded = String::from(nanos_str);
        while padded.len() < 9 {
            padded.push('0');
        }
        padded.truncate(9);
        let n: i64 = padded
            .parse()
            .map_err(|e| D::Error::custom(format!("invalid duration nanos: {e}")))?;
        n as i32
    };
    let mut msg = DynamicMessage::new(descriptor.clone());
    msg.set_field_by_name("seconds", Value::I64(sign * secs));
    msg.set_field_by_name("nanos", Value::I32(sign as i32 * nanos));
    Ok(msg)
}

fn deserialize_field_mask<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
) -> Result<DynamicMessage, D::Error> {
    let s = String::deserialize(deserializer)?;
    let paths: Vec<Value> = s
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|p| Value::String(lower_camel_to_snake(p)))
        .collect();
    let mut msg = DynamicMessage::new(descriptor.clone());
    msg.set_field_by_name("paths", Value::List(paths));
    Ok(msg)
}

fn deserialize_struct<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    let map = deserializer.deserialize_map(StructVisitor {
        descriptor: descriptor.clone(),
        options: options.clone(),
    })?;
    Ok(map)
}

struct StructVisitor {
    descriptor: MessageDescriptor,
    options: DeserializeOptions,
}

impl<'de> Visitor<'de> for StructVisitor {
    type Value = DynamicMessage;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON object for google.protobuf.Struct")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let pool = self.descriptor.parent_file().parent_pool();
        let value_d = pool
            .get_message_by_name("google.protobuf.Value")
            .ok_or_else(|| A::Error::custom("Struct value type not in pool"))?;
        let mut entries: HashMap<MapKey, Value> = HashMap::new();
        while let Some(key) = access.next_key::<String>()? {
            let v = access.next_value_seed(ValueSeed {
                descriptor: &value_d,
                options: &self.options,
            })?;
            entries.insert(MapKey::String(key), Value::Message(v));
        }
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("fields", Value::Map(entries));
        Ok(msg)
    }
}

fn deserialize_list_value<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    deserializer.deserialize_seq(ListValueVisitor {
        descriptor: descriptor.clone(),
        options: options.clone(),
    })
}

struct ListValueVisitor {
    descriptor: MessageDescriptor,
    options: DeserializeOptions,
}

impl<'de> Visitor<'de> for ListValueVisitor {
    type Value = DynamicMessage;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON array for google.protobuf.ListValue")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let pool = self.descriptor.parent_file().parent_pool();
        let value_d = pool
            .get_message_by_name("google.protobuf.Value")
            .ok_or_else(|| A::Error::custom("ListValue value type not in pool"))?;
        let mut items = Vec::new();
        while let Some(v) = access.next_element_seed(ValueSeed {
            descriptor: &value_d,
            options: &self.options,
        })? {
            items.push(Value::Message(v));
        }
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("values", Value::List(items));
        Ok(msg)
    }
}

fn deserialize_value<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    deserializer.deserialize_any(ValueVisitor {
        descriptor: descriptor.clone(),
        options: options.clone(),
    })
}

struct ValueVisitor {
    descriptor: MessageDescriptor,
    options: DeserializeOptions,
}

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = DynamicMessage;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON value for google.protobuf.Value")
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("null_value", Value::EnumNumber(0));
        Ok(msg)
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        self.visit_unit()
    }

    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("bool_value", Value::Bool(v));
        Ok(msg)
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        self.visit_f64(v as f64)
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        self.visit_f64(v as f64)
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("number_value", Value::F64(v));
        Ok(msg)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        self.visit_string(v.to_owned())
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("string_value", Value::String(v));
        Ok(msg)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        let pool = self.descriptor.parent_file().parent_pool();
        let list_d = pool
            .get_message_by_name("google.protobuf.ListValue")
            .ok_or_else(|| A::Error::custom("ListValue not in pool"))?;
        let inner = ListValueVisitor {
            descriptor: list_d,
            options: self.options.clone(),
        }
        .visit_seq(access)?;
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("list_value", Value::Message(inner));
        Ok(msg)
    }

    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        let pool = self.descriptor.parent_file().parent_pool();
        let struct_d = pool
            .get_message_by_name("google.protobuf.Struct")
            .ok_or_else(|| A::Error::custom("Struct not in pool"))?;
        let inner = StructVisitor {
            descriptor: struct_d,
            options: self.options.clone(),
        }
        .visit_map(access)?;
        let mut msg = DynamicMessage::new(self.descriptor.clone());
        msg.set_field_by_name("struct_value", Value::Message(inner));
        Ok(msg)
    }
}

struct ValueSeed<'a> {
    descriptor: &'a MessageDescriptor,
    options: &'a DeserializeOptions,
}

impl<'de> serde::de::DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = DynamicMessage;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserialize_value(self.descriptor, deserializer, self.options)
    }
}

fn deserialize_any<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    // Buffer the JSON object first so we can look at @type before
    // dispatching the rest.
    let raw: serde_value::Value = serde::Deserialize::deserialize(deserializer)?;
    let serde_value::Value::Map(mut entries) = raw else {
        return Err(D::Error::custom("Any expects a JSON object"));
    };
    let type_url_key = serde_value::Value::String("@type".to_string());
    let type_url = entries
        .remove(&type_url_key)
        .ok_or_else(|| D::Error::custom("Any missing `@type`"))?;
    let type_url = match type_url {
        serde_value::Value::String(s) => s,
        _ => return Err(D::Error::custom("Any `@type` must be a string")),
    };
    let inner_name = type_url.rsplit('/').next().unwrap_or(&type_url).to_owned();
    let pool = descriptor.parent_file().parent_pool();
    let inner_d = pool.get_message_by_name(&inner_name).ok_or_else(|| {
        D::Error::custom(format!("Any payload type `{inner_name}` not found in pool"))
    })?;

    let inner_msg = if super::super::is_well_known_type(&inner_name) {
        // The remaining map should have a single `value` key that the
        // WKT-specific deserializer understands.
        let value_key = serde_value::Value::String("value".to_string());
        let val = entries
            .remove(&value_key)
            .ok_or_else(|| D::Error::custom("Any of WKT requires `value`"))?;
        let de = serde_value::ValueDeserializer::<serde_value::DeserializerError>::new(val);
        deserialize_wkt(&inner_name, &inner_d, de, options)
            .map_err(|e| D::Error::custom(format!("Any inner WKT decode: {e}")))?
    } else {
        // Re-pack the remaining fields and route through the regular
        // message deserializer.
        let de_value = serde_value::Value::Map(entries);
        let de = serde_value::ValueDeserializer::<serde_value::DeserializerError>::new(de_value);
        super::deserialize_plain_message(&inner_d, de, options)
            .map_err(|e| D::Error::custom(format!("Any inner decode: {e}")))?
    };

    let bytes = inner_msg.encode_to_vec();
    let mut msg = DynamicMessage::new(descriptor.clone());
    msg.set_field_by_name("type_url", Value::String(type_url));
    msg.set_field_by_name("value", Value::Bytes(Bytes::from(bytes)));
    Ok(msg)
}

fn deserialize_scalar_wrapper<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    deserializer: D,
    options: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    let value_field = descriptor
        .get_field_by_name("value")
        .ok_or_else(|| D::Error::custom("scalar wrapper missing `value`"))?;
    let v = super::deserialize_singular_value(&value_field.kind(), deserializer, options)?;
    let mut msg = DynamicMessage::new(descriptor.clone());
    msg.try_set_field(&value_field, v)
        .map_err(D::Error::custom)?;
    Ok(msg)
}
