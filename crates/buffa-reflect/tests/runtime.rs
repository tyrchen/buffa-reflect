//! Integration tests for the descriptor pool / handle layer.
//!
//! These build a tiny `FileDescriptorSet` by hand from
//! `buffa_descriptor` types (no protoc required) and exercise the lookup
//! and validation paths.

use buffa::Message as _;
use buffa_descriptor::generated::descriptor::{
    DescriptorProto, EnumDescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto,
    FileDescriptorProto, FileDescriptorSet, MessageOptions, OneofDescriptorProto,
    field_descriptor_proto::{Label, Type},
};
use buffa_reflect::{Cardinality, DescriptorError, DescriptorPool, Kind};

fn proto3() -> Option<String> {
    Some("proto3".to_string())
}

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

    // map<string,string> labels — synthetic LabelsEntry message.
    let labels_entry = DescriptorProto {
        name: Some("LabelsEntry".to_string()),
        field: vec![
            field("key", 1, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
            field("value", 2, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
        ],
        options: MessageOptions {
            map_entry: Some(true),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    };

    let user = DescriptorProto {
        name: Some("User".to_string()),
        field: vec![
            field("id", 1, Type::TYPE_STRING, Label::LABEL_OPTIONAL, None),
            field(
                "role",
                2,
                Type::TYPE_ENUM,
                Label::LABEL_OPTIONAL,
                Some(".acme.api.v1.Role"),
            ),
            // labels: map<string, string>
            field(
                "labels",
                3,
                Type::TYPE_MESSAGE,
                Label::LABEL_REPEATED,
                Some(".acme.api.v1.User.LabelsEntry"),
            ),
            // contact oneof: email | phone
            FieldDescriptorProto {
                name: Some("email".to_string()),
                number: Some(4),
                label: Some(Label::LABEL_OPTIONAL),
                r#type: Some(Type::TYPE_STRING),
                oneof_index: Some(0),
                ..Default::default()
            },
            FieldDescriptorProto {
                name: Some("phone".to_string()),
                number: Some(5),
                label: Some(Label::LABEL_OPTIONAL),
                r#type: Some(Type::TYPE_STRING),
                oneof_index: Some(0),
                ..Default::default()
            },
            // proto3 optional `nickname` — synthetic oneof with one member.
            FieldDescriptorProto {
                name: Some("nickname".to_string()),
                number: Some(6),
                label: Some(Label::LABEL_OPTIONAL),
                r#type: Some(Type::TYPE_STRING),
                oneof_index: Some(1),
                proto3_optional: Some(true),
                ..Default::default()
            },
            // packed repeated int32
            field("scores", 7, Type::TYPE_INT32, Label::LABEL_REPEATED, None),
        ],
        nested_type: vec![labels_entry],
        oneof_decl: vec![
            OneofDescriptorProto {
                name: Some("contact".to_string()),
                ..Default::default()
            },
            OneofDescriptorProto {
                name: Some("_nickname".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    FileDescriptorProto {
        name: Some("acme/api/v1/user.proto".to_string()),
        package: Some("acme.api.v1".to_string()),
        syntax: proto3(),
        message_type: vec![user],
        enum_type: vec![role],
        ..Default::default()
    }
}

fn build_user_pool() -> DescriptorPool {
    let fds = FileDescriptorSet {
        file: vec![build_user_file()],
        ..Default::default()
    };
    DescriptorPool::from_file_descriptor_set(fds).expect("pool builds")
}

#[test]
fn test_should_resolve_message_by_full_name() {
    let pool = build_user_pool();
    let user = pool
        .get_message_by_name("acme.api.v1.User")
        .expect("user message present");
    assert_eq!(user.full_name(), "acme.api.v1.User");
    assert_eq!(user.name(), "User");
    assert_eq!(user.parent_file().package(), "acme.api.v1");
    assert_eq!(user.parent_file().syntax(), "proto3");

    // Leading dot is accepted.
    assert!(pool.get_message_by_name(".acme.api.v1.User").is_some());

    // Unknown name returns None instead of panicking.
    assert!(pool.get_message_by_name("acme.api.v1.NotAType").is_none());
}

#[test]
fn test_should_resolve_field_kinds() {
    let pool = build_user_pool();
    let user = pool.get_message_by_name("acme.api.v1.User").unwrap();

    let id = user.get_field_by_name("id").unwrap();
    assert_eq!(id.number(), 1);
    assert_eq!(id.json_name(), "id");
    assert!(matches!(id.kind(), Kind::String));
    assert_eq!(id.cardinality(), Cardinality::Optional);

    let role = user.get_field_by_number(2).unwrap();
    let role_kind = role.kind();
    let Kind::Enum(role_enum) = role_kind else {
        panic!("role should resolve to an enum kind");
    };
    assert_eq!(role_enum.full_name(), "acme.api.v1.Role");

    let labels = user.get_field_by_name("labels").unwrap();
    assert_eq!(labels.cardinality(), Cardinality::Repeated);
    assert!(labels.is_map());

    let scores = user.get_field_by_name("scores").unwrap();
    assert_eq!(scores.cardinality(), Cardinality::Repeated);
    assert!(scores.is_packed(), "proto3 scalars pack by default");
}

#[test]
fn test_should_track_oneofs_and_synthetic_oneofs() {
    let pool = build_user_pool();
    let user = pool.get_message_by_name("acme.api.v1.User").unwrap();
    let oneofs: Vec<_> = user.oneofs().collect();
    assert_eq!(oneofs.len(), 2);
    assert_eq!(oneofs[0].name(), "contact");
    assert!(!oneofs[0].is_synthetic());
    let contact_fields: Vec<_> = oneofs[0].fields().collect();
    assert_eq!(contact_fields.len(), 2);
    assert_eq!(contact_fields[0].name(), "email");

    assert!(oneofs[1].is_synthetic(), "proto3 optional → synthetic");
    let nickname = user.get_field_by_name("nickname").unwrap();
    assert!(nickname.supports_presence());
    let containing = nickname.containing_oneof().unwrap();
    assert!(containing.is_synthetic());
}

#[test]
fn test_should_lookup_by_json_name() {
    // Add a field with snake_case so we exercise the camelCase derivation.
    let mut file = build_user_file();
    let user = &mut file.message_type[0];
    user.field.push(field(
        "given_name",
        9,
        Type::TYPE_STRING,
        Label::LABEL_OPTIONAL,
        None,
    ));
    let pool = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    })
    .unwrap();
    let user = pool.get_message_by_name("acme.api.v1.User").unwrap();
    let by_snake = user.get_field_by_name("given_name").unwrap();
    let by_json = user.get_field_by_json_name("givenName").unwrap();
    assert_eq!(by_snake.number(), by_json.number());
    assert_eq!(by_json.json_name(), "givenName");
}

#[test]
fn test_should_round_trip_through_decode() {
    let original = build_user_pool();
    let bytes = FileDescriptorSet {
        file: original
            .files()
            .map(|f| f.descriptor_proto().clone())
            .collect(),
        ..Default::default()
    }
    .encode_to_vec();
    let decoded = DescriptorPool::decode(&bytes).expect("decode succeeds");
    let user = decoded.get_message_by_name("acme.api.v1.User").unwrap();
    assert_eq!(user.fields().count(), 7);
}

#[test]
fn test_should_reject_dangling_type_name() {
    let mut file = build_user_file();
    file.message_type[0].field.push(field(
        "missing_ref",
        99,
        Type::TYPE_MESSAGE,
        Label::LABEL_OPTIONAL,
        Some(".acme.api.v1.NoSuchType"),
    ));
    let err = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    })
    .expect_err("dangling type_name must error");
    match err {
        DescriptorError::UnresolvedType { type_name, .. } => {
            assert_eq!(type_name, ".acme.api.v1.NoSuchType");
        }
        other => panic!("expected UnresolvedType, got {other:?}"),
    }
}

#[test]
fn test_should_reject_duplicate_type_definition() {
    let mut file = build_user_file();
    let dup = file.message_type[0].clone();
    file.message_type.push(dup);
    let err = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    })
    .expect_err("duplicate type must error");
    assert!(matches!(err, DescriptorError::DuplicateType(_)));
}

#[test]
fn test_should_reject_invalid_field_number() {
    let mut file = build_user_file();
    file.message_type[0].field.push(field(
        "bad",
        19_500,
        Type::TYPE_STRING,
        Label::LABEL_OPTIONAL,
        None,
    ));
    let err = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    })
    .expect_err("reserved field number must error");
    assert!(matches!(err, DescriptorError::InvalidFieldNumber { .. }));
}

#[test]
fn test_should_reject_proto3_required_field() {
    let mut file = build_user_file();
    file.message_type[0].field.push(field(
        "must_have",
        80,
        Type::TYPE_STRING,
        Label::LABEL_REQUIRED,
        None,
    ));
    let err = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    })
    .expect_err("proto3 disallows required");
    assert!(matches!(err, DescriptorError::Proto3RequiredField { .. }));
}

#[test]
fn test_should_reject_duplicate_file() {
    let file = build_user_file();
    let dup = file.clone();
    let err = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file, dup],
        ..Default::default()
    })
    .expect_err("duplicate file must error");
    assert!(matches!(err, DescriptorError::DuplicateFile(_)));
}

#[test]
fn test_should_reject_proto3_enum_without_zero() {
    let bad = EnumDescriptorProto {
        name: Some("Bad".to_string()),
        value: vec![enum_value("BAD_ONE", 1)],
        ..Default::default()
    };
    let file = FileDescriptorProto {
        name: Some("acme/v1/bad.proto".to_string()),
        package: Some("acme.v1".to_string()),
        syntax: proto3(),
        enum_type: vec![bad],
        ..Default::default()
    };
    let err = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    })
    .expect_err("proto3 enum needs value 0");
    assert!(matches!(err, DescriptorError::Proto3EnumMissingZero(_)));
}

#[test]
fn test_should_resolve_relative_type_names() {
    // Field uses a relative `Role` reference (no leading dot) — needs the
    // C++ scoping resolver to find the sibling enum.
    let mut file = build_user_file();
    file.message_type[0].field.push(FieldDescriptorProto {
        name: Some("relative_role".to_string()),
        number: Some(50),
        label: Some(Label::LABEL_OPTIONAL),
        r#type: Some(Type::TYPE_ENUM),
        type_name: Some("Role".to_string()),
        ..Default::default()
    });
    let pool = DescriptorPool::from_file_descriptor_set(FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    })
    .unwrap();
    let user = pool.get_message_by_name("acme.api.v1.User").unwrap();
    let f = user.get_field_by_name("relative_role").unwrap();
    let Kind::Enum(e) = f.kind() else {
        panic!("expected enum kind");
    };
    assert_eq!(e.full_name(), "acme.api.v1.Role");
}

#[test]
fn test_should_iterate_all_messages_including_nested() {
    let pool = build_user_pool();
    let names: Vec<_> = pool
        .all_messages()
        .map(|m| m.full_name().to_string())
        .collect();
    assert!(names.contains(&"acme.api.v1.User".to_string()));
    assert!(names.contains(&"acme.api.v1.User.LabelsEntry".to_string()));
}
