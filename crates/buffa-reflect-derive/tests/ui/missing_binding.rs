use buffa_reflect_derive::ReflectMessage;

#[derive(ReflectMessage)]
#[buffa_reflect(message_name = "acme.api.v1.User")]
struct User;

fn main() {}
