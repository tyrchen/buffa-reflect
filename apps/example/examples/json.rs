//! Proto3 canonical JSON via the `serde` feature.
//!
//! Demonstrates the serialize / deserialize-with-seed pattern, the
//! options surface (`SerializeOptions`, `DeserializeOptions`), and a
//! round-trip through a typed Rust value.

use buffa::Message as _;
use buffa_reflect::{DescriptorPool, DeserializeOptions, DynamicMessage, SerializeOptions};
use buffa_reflect_example::{__buffa, FILE_DESCRIPTOR_SET_BYTES, Genre, Library, library};
use serde::de::DeserializeSeed as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = DescriptorPool::decode(FILE_DESCRIPTOR_SET_BYTES)?;
    let descriptor = pool
        .get_message_by_name("acme.api.v1.Library")
        .expect("Library is in the pool");

    let typed = Library {
        name: "Bath Public Library".to_string(),
        books: vec![library::Book {
            id: "b-001".to_string(),
            title: "Pride and Prejudice".to_string(),
            authors: vec!["Jane Austen".to_string(), "—Anonymous editor".to_string()],
            genre: ::buffa::EnumValue::Known(Genre::GENRE_FICTION),
            availability: Some(__buffa::oneof::library::book::Availability::InStock(7)),
            ..Default::default()
        }],
        ..Default::default()
    };
    // Build the DynamicMessage against the descriptor we want to share
    // between the original and the parsed copy below — the typed value's
    // own `descriptor()` lives in a separately-cached pool clone.
    let dyn_msg = DynamicMessage::decode(descriptor.clone(), typed.encode_to_vec().as_slice())?;

    // Default serialization follows the proto3 JSON canonical form:
    // camelCase field names, enum names as strings, default fields
    // omitted, 64-bit integers stringified.
    let canonical = serde_json::to_string_pretty(&dyn_msg)?;
    println!("== canonical JSON ==\n{canonical}\n");

    // Round-trip via the `DeserializeSeed` impl on `MessageDescriptor`.
    let mut de = serde_json::de::Deserializer::from_str(&canonical);
    let parsed: DynamicMessage = descriptor.clone().deserialize(&mut de)?;
    assert_eq!(dyn_msg, parsed);

    // Knobs match prost-reflect for cross-ecosystem familiarity.
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::pretty(&mut buf);
    dyn_msg.serialize_with_options(
        &mut ser,
        &SerializeOptions::new()
            .stringify_64_bit_integers(false)
            .use_proto_field_name(true)
            .use_enum_numbers(true)
            .skip_default_fields(false),
    )?;
    println!(
        "== with proto names + numeric enums + defaults ==\n{}\n",
        String::from_utf8_lossy(&buf)
    );

    // Strict deserialization rejects unknown fields. The default ignores
    // them per the proto3 JSON spec.
    let extra = r#"{ "name": "x", "futureField": 7 }"#;
    let mut de = serde_json::de::Deserializer::from_str(extra);
    let strict = DynamicMessage::deserialize_with_options(
        descriptor.clone(),
        &mut de,
        &DeserializeOptions::new().deny_unknown_fields(true),
    );
    println!(
        "strict deserialize rejected unknown field: {}",
        strict.is_err()
    );

    // Round-trip back to the typed Rust struct.
    let back: Library = parsed.transcode_to()?;
    assert_eq!(back.encode_to_vec(), typed.encode_to_vec());

    println!("\nJSON ↔ DynamicMessage ↔ typed: round-trip ok");
    Ok(())
}
