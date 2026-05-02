//! Textproto encode / decode via the `text-format` feature.

use buffa::Message as _;
use buffa_reflect::{DescriptorPool, DynamicMessage, FormatOptions};
use buffa_reflect_example::{FILE_DESCRIPTOR_SET_BYTES, Genre, Library, library};

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
            authors: vec!["Jane Austen".to_string()],
            genre: ::buffa::EnumValue::Known(Genre::GENRE_FICTION),
            ..Default::default()
        }],
        ..Default::default()
    };
    let dyn_msg = DynamicMessage::decode(descriptor.clone(), typed.encode_to_vec().as_slice())?;

    // Single-line, machine-friendly output is the default; compact
    // matches `protoc --decode` for the same wire bytes.
    let compact = dyn_msg.to_text_format();
    println!("== compact ==\n{compact}\n");

    // Multi-line, indented output for humans.
    let pretty = dyn_msg
        .to_text_format_with_options(&FormatOptions::new().pretty(true).skip_default_fields(true));
    println!("== pretty ==\n{pretty}");

    // Parse back and confirm structural equality.
    let reparsed = DynamicMessage::parse_text_format(descriptor.clone(), &pretty)?;
    assert_eq!(dyn_msg, reparsed);

    // Wire round-trip from textproto matches the typed encoder.
    let back: Library = reparsed.transcode_to()?;
    assert_eq!(back.encode_to_vec(), typed.encode_to_vec());

    println!("textproto ↔ DynamicMessage ↔ typed: round-trip ok");
    Ok(())
}
