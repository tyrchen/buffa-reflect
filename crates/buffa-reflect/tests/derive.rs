//! Integration tests for the `#[derive(ReflectMessage)]` macro.

use buffa::{DecodeError, DefaultInstance, Message, SizeCache, encoding::Tag};
use buffa_descriptor::generated::descriptor::field_descriptor_proto::{Label, Type};
use buffa_descriptor::generated::descriptor::{
    DescriptorProto, FieldDescriptorProto, FileDescriptorProto, FileDescriptorSet,
};
use buffa_reflect::{DescriptorPool, ReflectMessage};
use std::sync::LazyLock;

// ── synthetic generated message (mirrors what buffa codegen would emit) ──

#[derive(Clone, Default, PartialEq)]
#[allow(dead_code)]
struct User {
    pub id: Option<String>,
    pub name: Option<String>,
}

impl DefaultInstance for User {
    fn default_instance() -> &'static Self {
        static V: ::buffa::__private::OnceBox<User> = ::buffa::__private::OnceBox::new();
        V.get_or_init(|| Box::new(Self::default()))
    }
}

impl Message for User {
    fn compute_size(&self, _cache: &mut SizeCache) -> u32 {
        0
    }
    fn write_to(&self, _cache: &mut SizeCache, _buf: &mut impl ::buffa::bytes::BufMut) {}
    fn merge_field(
        &mut self,
        tag: Tag,
        buf: &mut impl ::buffa::bytes::Buf,
        _depth: u32,
    ) -> Result<(), DecodeError> {
        ::buffa::encoding::skip_field(tag, buf)
    }
    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, buffa_reflect::ReflectMessage, Default, PartialEq)]
#[buffa_reflect(file_descriptor_set_bytes = "&FDS_BYTES")]
#[buffa_reflect(message_name = "acme.api.v1.User")]
#[allow(dead_code)]
struct UserBytesForm {
    pub id: Option<String>,
}

impl DefaultInstance for UserBytesForm {
    fn default_instance() -> &'static Self {
        static V: ::buffa::__private::OnceBox<UserBytesForm> = ::buffa::__private::OnceBox::new();
        V.get_or_init(|| Box::new(Self::default()))
    }
}

impl Message for UserBytesForm {
    fn compute_size(&self, _cache: &mut SizeCache) -> u32 {
        0
    }
    fn write_to(&self, _cache: &mut SizeCache, _buf: &mut impl ::buffa::bytes::BufMut) {}
    fn merge_field(
        &mut self,
        tag: Tag,
        buf: &mut impl ::buffa::bytes::Buf,
        _depth: u32,
    ) -> Result<(), DecodeError> {
        ::buffa::encoding::skip_field(tag, buf)
    }
    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, buffa_reflect::ReflectMessage, Default, PartialEq)]
#[buffa_reflect(descriptor_pool = "&*POOL")]
#[buffa_reflect(message_name = "acme.api.v1.User")]
#[allow(dead_code)]
struct UserPoolForm {
    pub id: Option<String>,
}

impl DefaultInstance for UserPoolForm {
    fn default_instance() -> &'static Self {
        static V: ::buffa::__private::OnceBox<UserPoolForm> = ::buffa::__private::OnceBox::new();
        V.get_or_init(|| Box::new(Self::default()))
    }
}

impl Message for UserPoolForm {
    fn compute_size(&self, _cache: &mut SizeCache) -> u32 {
        0
    }
    fn write_to(&self, _cache: &mut SizeCache, _buf: &mut impl ::buffa::bytes::BufMut) {}
    fn merge_field(
        &mut self,
        tag: Tag,
        buf: &mut impl ::buffa::bytes::Buf,
        _depth: u32,
    ) -> Result<(), DecodeError> {
        ::buffa::encoding::skip_field(tag, buf)
    }
    fn clear(&mut self) {
        *self = Self::default();
    }
}

// Verify the longest-message-name resolution works (mirrors what
// buffa-reflect-build emits for nested messages).
#[derive(Clone, buffa_reflect::ReflectMessage, Default, PartialEq)]
#[buffa_reflect(file_descriptor_set_bytes = "&FDS_BYTES")]
#[buffa_reflect(message_name = "acme.api.v1.User")]
#[buffa_reflect(message_name = "acme.api.v1.User.Profile")]
#[allow(dead_code)]
struct Profile {
    pub bio: Option<String>,
}

impl DefaultInstance for Profile {
    fn default_instance() -> &'static Self {
        static V: ::buffa::__private::OnceBox<Profile> = ::buffa::__private::OnceBox::new();
        V.get_or_init(|| Box::new(Self::default()))
    }
}

impl Message for Profile {
    fn compute_size(&self, _cache: &mut SizeCache) -> u32 {
        0
    }
    fn write_to(&self, _cache: &mut SizeCache, _buf: &mut impl ::buffa::bytes::BufMut) {}
    fn merge_field(
        &mut self,
        tag: Tag,
        buf: &mut impl ::buffa::bytes::Buf,
        _depth: u32,
    ) -> Result<(), DecodeError> {
        ::buffa::encoding::skip_field(tag, buf)
    }
    fn clear(&mut self) {
        *self = Self::default();
    }
}

// ── descriptor fixture ──

fn build_user_fds() -> Vec<u8> {
    let profile = DescriptorProto {
        name: Some("Profile".to_string()),
        field: vec![FieldDescriptorProto {
            name: Some("bio".to_string()),
            number: Some(1),
            label: Some(Label::LABEL_OPTIONAL),
            r#type: Some(Type::TYPE_STRING),
            ..Default::default()
        }],
        ..Default::default()
    };
    let user = DescriptorProto {
        name: Some("User".to_string()),
        field: vec![
            FieldDescriptorProto {
                name: Some("id".to_string()),
                number: Some(1),
                label: Some(Label::LABEL_OPTIONAL),
                r#type: Some(Type::TYPE_STRING),
                ..Default::default()
            },
            FieldDescriptorProto {
                name: Some("name".to_string()),
                number: Some(2),
                label: Some(Label::LABEL_OPTIONAL),
                r#type: Some(Type::TYPE_STRING),
                ..Default::default()
            },
        ],
        nested_type: vec![profile],
        ..Default::default()
    };
    let file = FileDescriptorProto {
        name: Some("acme/api/v1/user.proto".to_string()),
        package: Some("acme.api.v1".to_string()),
        syntax: Some("proto3".to_string()),
        message_type: vec![user],
        ..Default::default()
    };
    FileDescriptorSet {
        file: vec![file],
        ..Default::default()
    }
    .encode_to_vec()
}

static FDS_BYTES: LazyLock<Vec<u8>> = LazyLock::new(build_user_fds);
static POOL: LazyLock<DescriptorPool> =
    LazyLock::new(|| DescriptorPool::decode(&FDS_BYTES).expect("pool decodes"));

#[test]
fn test_should_resolve_descriptor_via_bytes_form() {
    let u = UserBytesForm::default();
    let d = u.descriptor();
    assert_eq!(d.full_name(), "acme.api.v1.User");
    assert_eq!(d.fields().count(), 2);
}

#[test]
fn test_should_resolve_descriptor_via_pool_form() {
    let u = UserPoolForm::default();
    let d = u.descriptor();
    assert_eq!(d.full_name(), "acme.api.v1.User");
}

#[test]
fn test_should_pick_longest_message_name() {
    // Profile carries TWO `message_name` attributes — the macro must pick
    // the longer (= more specific) one, mirroring how buffa-reflect-build
    // emits attributes for nested messages via prefix matching.
    let p = Profile::default();
    let d = p.descriptor();
    assert_eq!(d.full_name(), "acme.api.v1.User.Profile");
}
