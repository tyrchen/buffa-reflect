use buffa_reflect_derive::ReflectMessage;

#[derive(ReflectMessage)]
#[buffa_reflect(file_descriptor_set_bytes = 42)]
#[buffa_reflect(message_name = "acme.api.v1.User")]
struct User;

fn main() {}
