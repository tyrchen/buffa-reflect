//! End-to-end tests for `DynamicMessage` covering accessors, encode /
//! decode round-trips, oneof / map / list semantics, default values,
//! unknown-field preservation, and recursion limits.
//!
//! Builds tiny in-memory `FileDescriptorSet`s — no protoc required.

#![cfg(feature = "dynamic")]
#![allow(missing_docs)]

use buffa::{Message as _, bytes::Bytes};
use buffa_descriptor::generated::descriptor::{
    DescriptorProto, EnumDescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto,
    FileDescriptorProto, FileDescriptorSet, MessageOptions, OneofDescriptorProto,
    field_descriptor_proto::{Label, Type},
};
use buffa_reflect::{DescriptorPool, DynamicMessage, MapKey, SetFieldError, Value};

// ── helpers ────────────────────────────────────────────────────────────

fn enum_value(name: &str, number: i32) -> EnumValueDescriptorProto {
    EnumValueDescriptorProto {
        name: Some(name.to_string()),
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
        name: Some(name.to_string()),
        number: Some(number),
        label: Some(label),
        r#type: Some(ty),
        type_name: type_name.map(str::to_string),
        ..Default::default()
    }
}

fn map_entry(name: &str, key: Type, value: Type, value_type_name: Option<&str>) -> DescriptorProto {
    DescriptorProto {
        name: Some(name.to_string()),
        field: vec![
            field("key", 1, key, Label::LABEL_OPTIONAL, None),
            field("value", 2, value, Label::LABEL_OPTIONAL, value_type_name),
        ],
        options: Some(MessageOptions {
            map_entry: Some(true),
            ..Default::default()
        })
        .into(),
        ..Default::default()
    }
}

fn build_user_file() -> FileDescriptorProto {
    let role = EnumDescriptorProto {
        name: Some("Role".to_string()),
        value: vec![
            enum_value("ROLE_UNSPECIFIED", 0),
            enum_value("ROLE_ADMIN", 1),
            enum_value("ROLE_USER", 2),
        ],
        ..Default::default()
    };
    let tags_entry = map_entry("TagsEntry", Type::TYPE_STRING, Type::TYPE_STRING, None);
    let mut tags_field = field(
        "tags",
        20,
        Type::TYPE_MESSAGE,
        Label::LABEL_REPEATED,
        Some(".acme.User.TagsEntry"),
    );
    tags_field.json_name = Some("tags".into());

    // Real oneof
    let oneof = OneofDescriptorProto {
        name: Some("contact".to_string()),
        ..Default::default()
    };
    let mut email = field("email", 30, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None);
    email.oneof_index = Some(0);
    let mut phone = field("phone", 31, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None);
    phone.oneof_index = Some(0);

    // Inner message
    let address = DescriptorProto {
        name: Some("Address".to_string()),
        field: vec![
            field("street", 1, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
            field("city", 2, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
        ],
        ..Default::default()
    };

    let user = DescriptorProto {
        name: Some("User".to_string()),
        field: vec![
            field("id", 1, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
            field("count", 2, Type::TYPE_INT32, Label::LABEL_OPTIONAL, None),
            field("balance", 3, Type::TYPE_INT64, Label::LABEL_OPTIONAL, None),
            field("ratio", 4, Type::TYPE_DOUBLE, Label::LABEL_OPTIONAL, None),
            field("active", 5, Type::TYPE_BOOL, Label::LABEL_OPTIONAL, None),
            field("blob", 6, Type::TYPE_BYTES, Label::LABEL_OPTIONAL, None),
            field(
                "role",
                7,
                Type::TYPE_ENUM,
                Label::LABEL_OPTIONAL,
                Some(".acme.Role"),
            ),
            field(
                "address",
                8,
                Type::TYPE_MESSAGE,
                Label::LABEL_OPTIONAL,
                Some(".acme.User.Address"),
            ),
            field("scores", 10, Type::TYPE_INT32, Label::LABEL_REPEATED, None),
            tags_field,
            email,
            phone,
        ],
        nested_type: vec![address, tags_entry],
        oneof_decl: vec![oneof],
        ..Default::default()
    };

    FileDescriptorProto {
        name: Some("acme/user.proto".into()),
        package: Some("acme".into()),
        syntax: Some("proto3".into()),
        message_type: vec![user],
        enum_type: vec![role],
        ..Default::default()
    }
}

fn user_pool() -> DescriptorPool {
    let fds = FileDescriptorSet {
        file: vec![build_user_file()],
        ..Default::default()
    };
    let bytes = fds.encode_to_vec();
    DescriptorPool::decode(&bytes).expect("pool decodes")
}

fn user_descriptor() -> buffa_reflect::MessageDescriptor {
    user_pool()
        .get_message_by_name("acme.User")
        .expect("message exists")
}

// ── M1 / M2 tests: construction, accessors, oneof, list, map ───────────

#[test]
fn test_should_construct_empty_dynamic_message() {
    let d = user_descriptor();
    let m = DynamicMessage::new(d.clone());
    assert!(m.is_empty());
    assert_eq!(m.descriptor(), d);
}

#[test]
fn test_should_round_trip_set_and_get_for_every_scalar_kind() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());

    m.set_field_by_name("id", "alice".into());
    m.set_field_by_name("count", 7i32.into());
    m.set_field_by_name("balance", 1_000_000i64.into());
    m.set_field_by_name("ratio", 1.25f64.into());
    m.set_field_by_name("active", true.into());
    m.set_field_by_name("blob", Bytes::from_static(b"\x01\x02").into());
    m.set_field_by_name("role", Value::EnumNumber(2));

    assert_eq!(m.get_field_by_name("id").unwrap().as_str(), Some("alice"));
    assert_eq!(m.get_field_by_name("count").unwrap().as_i32(), Some(7));
    assert_eq!(
        m.get_field_by_name("balance").unwrap().as_i64(),
        Some(1_000_000)
    );
    assert_eq!(m.get_field_by_name("ratio").unwrap().as_f64(), Some(1.25));
    assert_eq!(m.get_field_by_name("active").unwrap().as_bool(), Some(true));
    assert_eq!(
        m.get_field_by_name("blob").unwrap().as_bytes(),
        Some(&Bytes::from_static(b"\x01\x02"))
    );
    assert_eq!(
        m.get_field_by_name("role").unwrap().as_enum_number(),
        Some(2)
    );
}

#[test]
fn test_should_reject_string_into_int_via_try_set() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    let count = d.get_field_by_name("count").unwrap();
    let err = m
        .try_set_field(&count, Value::String("nope".into()))
        .unwrap_err();
    assert!(matches!(err, SetFieldError::InvalidType { .. }));
}

#[test]
fn test_should_synthesize_default_when_field_unset() {
    let d = user_descriptor();
    let m = DynamicMessage::new(d.clone());
    assert!(!m.has_field_by_name("count"));
    let v = m.get_field_by_name("count").unwrap();
    assert_eq!(v.as_i32(), Some(0));
}

#[test]
fn test_oneof_should_clear_sibling_when_other_member_set() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());

    m.set_field_by_name("email", "alice@example.com".into());
    assert!(m.has_field_by_name("email"));
    assert!(!m.has_field_by_name("phone"));

    m.set_field_by_name("phone", "+1-555-0100".into());
    assert!(!m.has_field_by_name("email"));
    assert!(m.has_field_by_name("phone"));

    m.clear_field_by_name("phone");
    assert!(!m.has_field_by_name("phone"));
}

#[test]
fn test_repeated_field_should_append_via_get_field_mut() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    let scores = d.get_field_by_name("scores").unwrap();
    let v = m.get_field_mut(&scores);
    if let Value::List(l) = v {
        l.push(Value::I32(1));
        l.push(Value::I32(2));
        l.push(Value::I32(3));
    }
    let v = m.get_field_by_name("scores").unwrap();
    assert_eq!(
        v.as_list()
            .unwrap()
            .iter()
            .map(|x| x.as_i32().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[test]
fn test_map_field_should_round_trip_string_to_string() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    let tags = d.get_field_by_name("tags").unwrap();
    let v = m.get_field_mut(&tags);
    if let Value::Map(map) = v {
        map.insert(MapKey::String("a".into()), Value::String("1".into()));
        map.insert(MapKey::String("b".into()), Value::String("2".into()));
    }
    let v = m.get_field_by_name("tags").unwrap();
    let map = v.as_map().unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!(
        map.get(&MapKey::String("a".into())).unwrap().as_str(),
        Some("1")
    );
}

// ── M4 / M5 tests: encode/decode round-trips ───────────────────────────

#[test]
fn test_should_round_trip_through_wire_for_every_scalar_kind() {
    let d = user_descriptor();
    let mut original = DynamicMessage::new(d.clone());
    original.set_field_by_name("id", "alice".into());
    original.set_field_by_name("count", 7i32.into());
    original.set_field_by_name("balance", 1_000_000i64.into());
    original.set_field_by_name("ratio", 1.25f64.into());
    original.set_field_by_name("active", true.into());
    original.set_field_by_name("blob", Bytes::from_static(b"\x00\x01\x02").into());
    original.set_field_by_name("role", Value::EnumNumber(1));

    let bytes = original.encode_to_vec();
    let restored = DynamicMessage::decode(d, bytes.as_slice()).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn test_should_round_trip_repeated_packed_scalar() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    m.set_field_by_name("scores", Value::List(vec![1.into(), 2.into(), 3.into()]));
    let bytes = m.encode_to_vec();
    let m2 = DynamicMessage::decode(d, bytes.as_slice()).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn test_should_round_trip_map_field() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    m.set_field_by_name("tags", {
        let mut map = std::collections::HashMap::new();
        map.insert(MapKey::String("city".into()), Value::String("Bath".into()));
        map.insert(
            MapKey::String("region".into()),
            Value::String("Somerset".into()),
        );
        Value::Map(map)
    });
    let bytes = m.encode_to_vec();
    let m2 = DynamicMessage::decode(d, bytes.as_slice()).unwrap();
    assert_eq!(m, m2);
}

#[test]
fn test_should_round_trip_nested_message() {
    let pool = user_pool();
    let user_d = pool.get_message_by_name("acme.User").unwrap();
    let address_d = pool.get_message_by_name("acme.User.Address").unwrap();

    let mut address = DynamicMessage::new(address_d);
    address.set_field_by_name("street", "1 Royal Crescent".into());
    address.set_field_by_name("city", "Bath".into());

    let mut user = DynamicMessage::new(user_d.clone());
    user.set_field_by_name("id", "alice".into());
    user.set_field_by_name("address", Value::Message(address));

    let bytes = user.encode_to_vec();
    let restored = DynamicMessage::decode(user_d, bytes.as_slice()).unwrap();
    assert_eq!(user, restored);
}

#[test]
fn test_should_preserve_unknown_fields_round_trip() {
    let d = user_descriptor();
    // Build wire bytes containing a field that doesn't exist on the descriptor.
    // Field 100 (varint) with value 42.
    let mut wire = Vec::new();
    use buffa::encoding::{Tag, WireType, encode_varint};
    Tag::new(1, WireType::LengthDelimited).encode(&mut wire);
    encode_varint(5, &mut wire);
    wire.extend_from_slice(b"alice");
    Tag::new(100, WireType::Varint).encode(&mut wire);
    encode_varint(42, &mut wire);

    let m = DynamicMessage::decode(d, wire.as_slice()).unwrap();
    assert_eq!(m.unknown_fields().count(), 1);
    let bytes = m.encode_to_vec();
    assert_eq!(bytes, wire);
}

#[test]
fn test_should_round_trip_oneof_member() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    m.set_field_by_name("phone", "+1-555-0100".into());
    let bytes = m.encode_to_vec();
    let restored = DynamicMessage::decode(d, bytes.as_slice()).unwrap();
    assert_eq!(m, restored);
    assert!(restored.has_field_by_name("phone"));
    assert!(!restored.has_field_by_name("email"));
}

#[test]
fn test_recursion_limit_should_reject_deep_message() {
    // Build a self-recursive Node descriptor in proto2 (so message fields
    // are optional with explicit presence).
    let node_proto = DescriptorProto {
        name: Some("Node".into()),
        field: vec![field(
            "next",
            1,
            Type::TYPE_MESSAGE,
            Label::LABEL_OPTIONAL,
            Some(".cycles.Node"),
        )],
        ..Default::default()
    };
    let file = FileDescriptorProto {
        name: Some("cycles.proto".into()),
        package: Some("cycles".into()),
        syntax: Some("proto2".into()),
        message_type: vec![node_proto],
        ..Default::default()
    };
    let pool = DescriptorPool::decode(
        &FileDescriptorSet {
            file: vec![file],
            ..Default::default()
        }
        .encode_to_vec(),
    )
    .unwrap();
    let node_d = pool.get_message_by_name("cycles.Node").unwrap();

    // Hand-build a 200-deep wire stream: 200 nested length-delimited tags
    // for field 1.
    let mut deepest = Vec::new();
    for _ in 0..200 {
        let inner_len = deepest.len();
        let mut frame = Vec::new();
        use buffa::encoding::{Tag, WireType, encode_varint};
        Tag::new(1, WireType::LengthDelimited).encode(&mut frame);
        encode_varint(inner_len as u64, &mut frame);
        frame.extend_from_slice(&deepest);
        deepest = frame;
    }
    let opts = buffa::DecodeOptions::new().with_recursion_limit(50);
    let err = DynamicMessage::decode_with_options(node_d.clone(), deepest.as_slice(), opts);
    assert!(matches!(
        err,
        Err(buffa::DecodeError::RecursionLimitExceeded)
    ));
}

#[test]
fn test_unknown_enum_number_should_round_trip() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    m.set_field_by_name("role", Value::EnumNumber(999));
    let bytes = m.encode_to_vec();
    let restored = DynamicMessage::decode(d, bytes.as_slice()).unwrap();
    assert_eq!(
        restored.get_field_by_name("role").unwrap().as_enum_number(),
        Some(999)
    );
}

#[test]
fn test_send_sync_is_satisfied() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DynamicMessage>();
    assert_send_sync::<Value>();
    assert_send_sync::<MapKey>();
    assert_send_sync::<SetFieldError>();
}

// ── M3 default-value parser ────────────────────────────────────────────

#[test]
fn test_proto2_default_value_should_be_eagerly_parsed() {
    let count_with_default = FieldDescriptorProto {
        default_value: Some("42".to_string()),
        ..field("count", 1, Type::TYPE_INT32, Label::LABEL_OPTIONAL, None)
    };
    let msg = DescriptorProto {
        name: Some("Foo".into()),
        field: vec![count_with_default],
        ..Default::default()
    };
    let file = FileDescriptorProto {
        name: Some("foo.proto".into()),
        package: Some("acme".into()),
        syntax: Some("proto2".into()),
        message_type: vec![msg],
        ..Default::default()
    };
    let pool = DescriptorPool::decode(
        &FileDescriptorSet {
            file: vec![file],
            ..Default::default()
        }
        .encode_to_vec(),
    )
    .unwrap();
    let foo = pool.get_message_by_name("acme.Foo").unwrap();
    let m = DynamicMessage::new(foo.clone());
    let v = m.get_field_by_name("count").unwrap();
    assert_eq!(v.as_i32(), Some(42));
}

#[test]
fn test_invalid_default_value_should_surface_as_pool_error() {
    let bad = FieldDescriptorProto {
        default_value: Some("not-a-number".into()),
        ..field("count", 1, Type::TYPE_INT32, Label::LABEL_OPTIONAL, None)
    };
    let msg = DescriptorProto {
        name: Some("Foo".into()),
        field: vec![bad],
        ..Default::default()
    };
    let file = FileDescriptorProto {
        name: Some("bad.proto".into()),
        package: Some("acme".into()),
        syntax: Some("proto2".into()),
        message_type: vec![msg],
        ..Default::default()
    };
    let err = DescriptorPool::decode(
        &FileDescriptorSet {
            file: vec![file],
            ..Default::default()
        }
        .encode_to_vec(),
    )
    .unwrap_err();
    assert!(matches!(
        err,
        buffa_reflect::DescriptorError::InvalidDefaultValue { .. }
    ));
}

// ── M6 transcode ───────────────────────────────────────────────────────

#[test]
fn test_dynamic_transcode_to_dynamic_should_clone() {
    let d = user_descriptor();
    let mut m = DynamicMessage::new(d.clone());
    m.set_field_by_name("id", "alice".into());
    let m2 = m.transcode_to_dynamic();
    assert_eq!(m, m2);
}

#[test]
fn test_dynamic_message_should_implement_reflect_message() {
    let d = user_descriptor();
    let m = DynamicMessage::new(d.clone());
    // Generic call site treating `&DynamicMessage` as a generic
    // `T: ReflectMessage` — confirms the trait impl resolves and that
    // descriptor() returns the same descriptor we constructed it with.
    fn require_reflect<T: buffa_reflect::ReflectMessage>(
        t: &T,
    ) -> buffa_reflect::MessageDescriptor {
        t.descriptor()
    }
    assert_eq!(require_reflect(&m), d);
}
