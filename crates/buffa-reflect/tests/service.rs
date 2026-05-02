//! Tests for `ServiceDescriptor` / `MethodDescriptor`.

#![allow(missing_docs)]

use buffa::Message as _;
use buffa_descriptor::generated::descriptor::{
    DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
    MethodDescriptorProto, ServiceDescriptorProto,
    field_descriptor_proto::{Label, Type},
};
use buffa_reflect::DescriptorPool;

fn field(name: &str, number: i32) -> FieldDescriptorProto {
    FieldDescriptorProto {
        name: Some(name.into()),
        number: Some(number),
        label: Some(Label::LABEL_OPTIONAL),
        r#type: Some(Type::TYPE_STRING),
        ..Default::default()
    }
}

fn build_pool() -> DescriptorPool {
    let req = DescriptorProto {
        name: Some("HelloRequest".into()),
        field: vec![field("name", 1)],
        ..Default::default()
    };
    let resp = DescriptorProto {
        name: Some("HelloReply".into()),
        field: vec![field("message", 1)],
        ..Default::default()
    };
    let svc = ServiceDescriptorProto {
        name: Some("Greeter".into()),
        method: vec![
            MethodDescriptorProto {
                name: Some("SayHello".into()),
                input_type: Some(".acme.HelloRequest".into()),
                output_type: Some(".acme.HelloReply".into()),
                ..Default::default()
            },
            MethodDescriptorProto {
                name: Some("StreamHello".into()),
                input_type: Some(".acme.HelloRequest".into()),
                output_type: Some(".acme.HelloReply".into()),
                client_streaming: Some(true),
                server_streaming: Some(true),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let file = FileDescriptorProto {
        name: Some("acme/greet.proto".into()),
        package: Some("acme".into()),
        syntax: Some("proto3".into()),
        message_type: vec![req, resp],
        service: vec![svc],
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

#[test]
fn test_should_resolve_service_by_full_name() {
    let pool = build_pool();
    let svc = pool.get_service_by_name("acme.Greeter").unwrap();
    assert_eq!(svc.full_name(), "acme.Greeter");
    assert_eq!(svc.name(), "Greeter");
    assert_eq!(svc.parent_file().name(), "acme/greet.proto");
}

#[test]
fn test_should_iterate_methods() {
    let pool = build_pool();
    let svc = pool.get_service_by_name("acme.Greeter").unwrap();
    let names: Vec<String> = svc.methods().map(|m| m.name().to_string()).collect();
    assert_eq!(names, vec!["SayHello", "StreamHello"]);
}

#[test]
fn test_method_should_resolve_input_output_messages() {
    let pool = build_pool();
    let svc = pool.get_service_by_name("acme.Greeter").unwrap();
    let m = svc.methods().next().unwrap();
    assert_eq!(m.input().full_name(), "acme.HelloRequest");
    assert_eq!(m.output().full_name(), "acme.HelloReply");
    assert!(!m.is_client_streaming());
    assert!(!m.is_server_streaming());

    let stream = svc.methods().nth(1).unwrap();
    assert!(stream.is_client_streaming());
    assert!(stream.is_server_streaming());
}

#[test]
fn test_pool_iterates_all_services() {
    let pool = build_pool();
    let names: Vec<String> = pool.services().map(|s| s.full_name().to_string()).collect();
    assert_eq!(names, vec!["acme.Greeter"]);
}

#[test]
fn test_file_lists_its_services() {
    let pool = build_pool();
    let file = pool.get_file_by_name("acme/greet.proto").unwrap();
    let names: Vec<String> = file.services().map(|s| s.full_name().to_string()).collect();
    assert_eq!(names, vec!["acme.Greeter"]);
}
