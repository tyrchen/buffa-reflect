//! Proto3 JSON serialization tests for `DynamicMessage`.

#![cfg(all(feature = "dynamic", feature = "serde"))]
#![allow(missing_docs)]

use buffa::Message as _;
use buffa::bytes::Bytes;
use buffa_descriptor::generated::descriptor::{
    DescriptorProto, EnumDescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto,
    FileDescriptorProto, FileDescriptorSet, MessageOptions,
    field_descriptor_proto::{Label, Type},
};
use buffa_reflect::{DescriptorPool, DynamicMessage, MapKey, SerializeOptions, Value};
use serde::de::DeserializeSeed;

fn enum_value(name: &str, number: i32) -> EnumValueDescriptorProto {
    EnumValueDescriptorProto {
        name: Some(name.into()),
        number: Some(number),
        ..Default::default()
    }
}

fn field(
    name: &str,
    number: i32,
    ty: Type,
    label: Label,
    type_name: Option<&str>,
) -> FieldDescriptorProto {
    FieldDescriptorProto {
        name: Some(name.into()),
        number: Some(number),
        label: Some(label),
        r#type: Some(ty),
        type_name: type_name.map(str::to_string),
        ..Default::default()
    }
}

fn build_pool() -> DescriptorPool {
    let role = EnumDescriptorProto {
        name: Some("Role".into()),
        value: vec![
            enum_value("ROLE_UNSPECIFIED", 0),
            enum_value("ROLE_ADMIN", 1),
            enum_value("ROLE_USER", 2),
        ],
        ..Default::default()
    };
    let tags_entry = DescriptorProto {
        name: Some("TagsEntry".into()),
        field: vec![
            field("key", 1, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
            field("value", 2, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
        ],
        options: Some(MessageOptions {
            map_entry: Some(true),
            ..Default::default()
        })
        .into(),
        ..Default::default()
    };
    let mut tags_field = field(
        "tags",
        9,
        Type::TYPE_MESSAGE,
        Label::LABEL_REPEATED,
        Some(".acme.User.TagsEntry"),
    );
    tags_field.json_name = Some("tags".into());

    let user = DescriptorProto {
        name: Some("User".into()),
        field: vec![
            field("user_id", 1, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
            field("count", 2, Type::TYPE_INT32, Label::LABEL_OPTIONAL, None),
            field("balance", 3, Type::TYPE_INT64, Label::LABEL_OPTIONAL, None),
            field("active", 4, Type::TYPE_BOOL, Label::LABEL_OPTIONAL, None),
            field("blob", 5, Type::TYPE_BYTES, Label::LABEL_OPTIONAL, None),
            field(
                "role",
                6,
                Type::TYPE_ENUM,
                Label::LABEL_OPTIONAL,
                Some(".acme.Role"),
            ),
            field("scores", 7, Type::TYPE_INT32, Label::LABEL_REPEATED, None),
            field("ratio", 8, Type::TYPE_DOUBLE, Label::LABEL_OPTIONAL, None),
            tags_field,
        ],
        nested_type: vec![tags_entry],
        ..Default::default()
    };
    let file = FileDescriptorProto {
        name: Some("acme/user.proto".into()),
        package: Some("acme".into()),
        syntax: Some("proto3".into()),
        message_type: vec![user],
        enum_type: vec![role],
        ..Default::default()
    };
    DescriptorPool::decode(
        &FileDescriptorSet {
            file: vec![file],
            ..Default::default()
        }
        .encode_to_vec(),
    )
    .unwrap()
}

fn user_descriptor() -> buffa_reflect::MessageDescriptor {
    build_pool().get_message_by_name("acme.User").unwrap()
}

#[test]
fn test_should_serialize_with_lower_camel_case_keys() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("user_id", "alice".into());
    m.set_field_by_name("count", 7i32.into());
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""userId":"alice""#));
    assert!(s.contains(r#""count":7"#));
}

#[test]
fn test_should_stringify_int64_by_default() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("balance", 1_000_000i64.into());
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""balance":"1000000""#));
}

#[test]
fn test_should_emit_int64_as_number_when_disabled() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("balance", 42i64.into());
    let mut out = Vec::new();
    let mut s = serde_json::Serializer::new(&mut out);
    let opts = SerializeOptions::new().stringify_64_bit_integers(false);
    m.serialize_with_options(&mut s, &opts).unwrap();
    let json = String::from_utf8(out).unwrap();
    assert!(json.contains(r#""balance":42"#));
}

#[test]
fn test_should_emit_bytes_as_base64() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("blob", Bytes::from_static(b"hello").into());
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""blob":"aGVsbG8=""#));
}

#[test]
fn test_should_emit_enum_as_name() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("role", Value::EnumNumber(2));
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""role":"ROLE_USER""#));
}

#[test]
fn test_should_emit_enum_as_number_when_requested() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("role", Value::EnumNumber(2));
    let mut out = Vec::new();
    let mut s = serde_json::Serializer::new(&mut out);
    let opts = SerializeOptions::new().use_enum_numbers(true);
    m.serialize_with_options(&mut s, &opts).unwrap();
    let json = String::from_utf8(out).unwrap();
    assert!(json.contains(r#""role":2"#));
}

#[test]
fn test_should_skip_default_fields_by_default() {
    let d = user_descriptor();
    let m = DynamicMessage::new(d);
    let s = serde_json::to_string(&m).unwrap();
    assert_eq!(s, "{}");
}

#[test]
fn test_should_round_trip_through_json() {
    let d = user_descriptor();
    let mut original = DynamicMessage::new(d.clone());
    original.set_field_by_name("user_id", "alice".into());
    original.set_field_by_name("count", 7i32.into());
    original.set_field_by_name("balance", 100i64.into());
    original.set_field_by_name("active", true.into());
    original.set_field_by_name("blob", Bytes::from_static(b"hi").into());
    original.set_field_by_name("role", Value::EnumNumber(1));
    original.set_field_by_name("scores", Value::List(vec![1.into(), 2.into(), 3.into()]));
    original.set_field_by_name("ratio", 1.5f64.into());
    original.set_field_by_name("tags", {
        let mut map = std::collections::HashMap::new();
        map.insert(MapKey::String("a".into()), Value::String("1".into()));
        Value::Map(map)
    });

    let json = serde_json::to_string(&original).unwrap();
    let mut de = serde_json::Deserializer::from_str(&json);
    let restored = d.clone().deserialize(&mut de).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn test_should_accept_proto_field_name_on_deserialize() {
    let d = user_descriptor();
    let json = r#"{"user_id": "alice"}"#;
    let mut de = serde_json::Deserializer::from_str(json);
    let restored = d.deserialize(&mut de).unwrap();
    assert_eq!(
        restored.get_field_by_name("user_id").unwrap().as_str(),
        Some("alice")
    );
}

#[test]
fn test_should_emit_special_floats_as_strings() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("ratio", f64::NAN.into());
    let s = serde_json::to_string(&m).unwrap();
    assert!(s.contains(r#""ratio":"NaN""#));
}

#[test]
fn test_should_reject_unknown_field_when_deny_set() {
    let d = user_descriptor();
    let json = r#"{"unknownField": 1}"#;
    let mut de = serde_json::Deserializer::from_str(json);
    let opts = buffa_reflect::DeserializeOptions::new().deny_unknown_fields(true);
    let err =
        buffa_reflect::DynamicMessage::deserialize_with_options(d, &mut de, &opts).unwrap_err();
    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn test_should_silently_drop_unknown_field_by_default() {
    let d = user_descriptor();
    let json = r#"{"unknownField": 1, "userId": "alice"}"#;
    let mut de = serde_json::Deserializer::from_str(json);
    let m = d.deserialize(&mut de).unwrap();
    assert_eq!(
        m.get_field_by_name("user_id").unwrap().as_str(),
        Some("alice")
    );
}
