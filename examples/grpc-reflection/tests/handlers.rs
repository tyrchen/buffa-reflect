//! Unit tests for the reflection request handlers.

#![allow(missing_docs)]

use buffa::Message as _;
use buffa_descriptor::generated::descriptor::{
    DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
    MethodDescriptorProto, ServiceDescriptorProto,
    field_descriptor_proto::{Label, Type},
};
use buffa_grpc_reflection::ReflectionService;
use buffa_grpc_reflection::proto::v1::{
    ServerReflectionRequest, server_reflection_request::MessageRequest,
    server_reflection_response::MessageResponse,
};

fn field(name: &str, number: i32) -> FieldDescriptorProto {
    FieldDescriptorProto {
        name: Some(name.into()),
        number: Some(number),
        label: Some(Label::LABEL_OPTIONAL),
        r#type: Some(Type::TYPE_STRING),
        ..Default::default()
    }
}

fn build_pool() -> buffa_reflect::DescriptorPool {
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
        method: vec![MethodDescriptorProto {
            name: Some("SayHello".into()),
            input_type: Some(".acme.HelloRequest".into()),
            output_type: Some(".acme.HelloReply".into()),
            ..Default::default()
        }],
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
    buffa_reflect::DescriptorPool::decode(
        &FileDescriptorSet {
            file: vec![file],
            ..Default::default()
        }
        .encode_to_vec(),
    )
    .unwrap()
}

fn dispatch(svc: &ReflectionService, req: MessageRequest) -> MessageResponse {
    svc.handle_one(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(req),
    })
    .message_response
    .expect("response carries body")
}

#[test]
fn test_list_services_returns_pool_services() {
    let svc = ReflectionService::new(build_pool(), None);
    let MessageResponse::ListServicesResponse(list) =
        dispatch(&svc, MessageRequest::ListServices(String::new()))
    else {
        panic!("expected list response");
    };
    let names: Vec<String> = list.service.into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["acme.Greeter"]);
}

#[test]
fn test_file_by_filename_returns_encoded_proto() {
    let svc = ReflectionService::new(build_pool(), None);
    let MessageResponse::FileDescriptorResponse(r) = dispatch(
        &svc,
        MessageRequest::FileByFilename("acme/greet.proto".into()),
    ) else {
        panic!("expected file response");
    };
    assert_eq!(r.file_descriptor_proto.len(), 1);
    assert!(!r.file_descriptor_proto[0].is_empty());
}

#[test]
fn test_file_containing_symbol_resolves_message() {
    let svc = ReflectionService::new(build_pool(), None);
    let MessageResponse::FileDescriptorResponse(r) = dispatch(
        &svc,
        MessageRequest::FileContainingSymbol("acme.HelloRequest".into()),
    ) else {
        panic!("expected file response");
    };
    assert_eq!(r.file_descriptor_proto.len(), 1);
}

#[test]
fn test_file_containing_symbol_resolves_service() {
    let svc = ReflectionService::new(build_pool(), None);
    let MessageResponse::FileDescriptorResponse(r) = dispatch(
        &svc,
        MessageRequest::FileContainingSymbol("acme.Greeter".into()),
    ) else {
        panic!("expected file response");
    };
    assert_eq!(r.file_descriptor_proto.len(), 1);
}

#[test]
fn test_unknown_symbol_returns_error() {
    let svc = ReflectionService::new(build_pool(), None);
    let MessageResponse::ErrorResponse(e) = dispatch(
        &svc,
        MessageRequest::FileContainingSymbol("nope".into()),
    ) else {
        panic!("expected error response");
    };
    assert!(!e.error_message.is_empty());
}

#[test]
fn test_advertised_services_overrides_pool() {
    let svc = ReflectionService::new(build_pool(), Some(vec!["custom.Service".into()]));
    let MessageResponse::ListServicesResponse(list) =
        dispatch(&svc, MessageRequest::ListServices(String::new()))
    else {
        panic!("expected list response");
    };
    let names: Vec<String> = list.service.into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["custom.Service"]);
}

#[test]
fn test_v1alpha_list_services_matches_v1() {
    use buffa_grpc_reflection::ReflectionServiceV1Alpha;
    use buffa_grpc_reflection::proto::v1alpha::{
        ServerReflectionRequest, server_reflection_request::MessageRequest,
        server_reflection_response::MessageResponse,
    };
    let svc = ReflectionServiceV1Alpha::new(build_pool(), None);
    let resp = svc.handle_one(ServerReflectionRequest {
        host: String::new(),
        message_request: Some(MessageRequest::ListServices(String::new())),
    });
    let MessageResponse::ListServicesResponse(list) = resp.message_response.unwrap() else {
        panic!("expected list response");
    };
    let names: Vec<String> = list.service.into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["acme.Greeter"]);
}

#[test]
fn test_file_containing_extension_returns_not_found() {
    use buffa_grpc_reflection::proto::v1::ExtensionRequest;
    let svc = ReflectionService::new(build_pool(), None);
    let MessageResponse::ErrorResponse(e) = dispatch(
        &svc,
        MessageRequest::FileContainingExtension(ExtensionRequest {
            containing_type: "acme.HelloRequest".into(),
            extension_number: 1000,
        }),
    ) else {
        panic!("expected NOT_FOUND error response");
    };
    assert_eq!(e.error_code, 5);
    assert!(e.error_message.contains("not found"));
}
