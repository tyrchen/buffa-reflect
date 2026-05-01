use buffa_reflect_derive::ReflectMessage;

#[derive(ReflectMessage)]
#[buffa_reflect(file_descriptor_set_bytes = "crate::FDS")]
struct User;

fn main() {}
