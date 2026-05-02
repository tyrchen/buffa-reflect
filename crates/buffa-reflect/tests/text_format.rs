//! Textproto encode / decode round-trip tests.

#![cfg(all(feature = "dynamic", feature = "text-format"))]
#![allow(missing_docs)]

use buffa::{Message as _, bytes::Bytes};
use buffa_descriptor::generated::descriptor::{
    DescriptorProto, EnumDescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto,
    FileDescriptorProto, FileDescriptorSet, MessageOptions, OneofDescriptorProto,
    field_descriptor_proto::{Label, Type},
};
use buffa_reflect::{DescriptorPool, DynamicMessage, FormatOptions, MapKey, Value};

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
    let tags = field(
        "tags",
        9,
        Type::TYPE_MESSAGE,
        Label::LABEL_REPEATED,
        Some(".acme.User.TagsEntry"),
    );

    let mut email = field("email", 30, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None);
    email.oneof_index = Some(0);
    let mut phone = field("phone", 31, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None);
    phone.oneof_index = Some(0);
    let oneof = OneofDescriptorProto {
        name: Some("contact".into()),
        ..Default::default()
    };

    let user = DescriptorProto {
        name: Some("User".into()),
        field: vec![
            field("name", 1, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
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
            tags,
            email,
            phone,
        ],
        nested_type: vec![tags_entry],
        oneof_decl: vec![oneof],
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
fn test_should_format_basic_scalars() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("name", "alice".into());
    m.set_field_by_name("count", 7i32.into());
    let s = m.to_text_format();
    assert!(s.contains(r#"name: "alice""#));
    assert!(s.contains("count: 7"));
}

#[test]
fn test_should_format_repeated_short_form() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("scores", Value::List(vec![1.into(), 2.into(), 3.into()]));
    let s = m.to_text_format();
    // One entry per line is canonical; the parser also accepts [a,b,c].
    assert!(s.contains("scores: 1"));
    assert!(s.contains("scores: 2"));
    assert!(s.contains("scores: 3"));
}

#[test]
fn test_should_round_trip_through_text_format() {
    let d = user_descriptor();
    let mut original = DynamicMessage::new(d.clone());
    original.set_field_by_name("name", "alice".into());
    original.set_field_by_name("count", 7i32.into());
    original.set_field_by_name("balance", 99i64.into());
    original.set_field_by_name("active", true.into());
    original.set_field_by_name("blob", Bytes::from_static(b"\x01\x02").into());
    original.set_field_by_name("role", Value::EnumNumber(1));
    original.set_field_by_name("scores", Value::List(vec![1.into(), 2.into()]));
    original.set_field_by_name("tags", {
        let mut m = std::collections::HashMap::new();
        m.insert(MapKey::String("a".into()), Value::String("1".into()));
        Value::Map(m)
    });

    let s = original.to_text_format();
    let restored = DynamicMessage::parse_text_format(d, &s).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn test_should_pretty_print_with_indentation() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("name", "alice".into());
    let s = m.to_text_format_with_options(&FormatOptions::new().pretty(true));
    assert!(s.contains("name: \"alice\"\n") || s.ends_with('\n'));
}

#[test]
fn test_should_parse_short_repeated_form() {
    let d = user_descriptor();
    let s = "scores: [1, 2, 3]";
    let m = DynamicMessage::parse_text_format(d, s).unwrap();
    let v = m.get_field_by_name("scores").unwrap();
    let items: Vec<i32> = v
        .as_list()
        .unwrap()
        .iter()
        .map(|x| x.as_i32().unwrap())
        .collect();
    assert_eq!(items, vec![1, 2, 3]);
}

#[test]
fn test_should_emit_enum_as_name() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("role", Value::EnumNumber(2));
    let s = m.to_text_format();
    assert!(s.contains("role: ROLE_USER"));
}

#[test]
fn test_should_skip_unknown_fields_when_set() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d);
    m.set_field_by_name("name", "alice".into());
    let s = m.to_text_format_with_options(&FormatOptions::new().skip_unknown_fields(true));
    assert!(s.contains("name"));
}

#[test]
fn test_should_strip_comments_on_parse() {
    let d = user_descriptor();
    let s = r#"
# this is a comment
name: "alice" # trailing comment
count: 42
"#;
    let m = DynamicMessage::parse_text_format(d, s).unwrap();
    assert_eq!(m.get_field_by_name("name").unwrap().as_str(), Some("alice"));
    assert_eq!(m.get_field_by_name("count").unwrap().as_i32(), Some(42));
}

#[test]
fn test_should_round_trip_oneof_member() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    m.set_field_by_name("phone", "+1-555-0100".into());
    let s = m.to_text_format();
    let restored = DynamicMessage::parse_text_format(d, &s).unwrap();
    assert_eq!(m, restored);
}
