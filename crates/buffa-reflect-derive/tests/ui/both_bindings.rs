use buffa_reflect_derive::ReflectMessage;

#[derive(ReflectMessage)]
#[buffa_reflect(descriptor_pool = "crate::POOL")]
#[buffa_reflect(file_descriptor_set_bytes = "crate::FDS")]
#[buffa_reflect(message_name = "acme.api.v1.User")]
struct User;

fn main() {}
