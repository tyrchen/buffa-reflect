//! `DynamicMessage` walkthrough — decode by descriptor, mutate fields,
//! transcode to and from the typed Rust struct, all without any
//! schema-specific code.

use buffa::Message as _;
use buffa_reflect::{DescriptorPool, DynamicMessage, MapKey, ReflectMessage, SetFieldError, Value};
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
        tags: [
            ("city".to_string(), "Bath".to_string()),
            ("region".to_string(), "Somerset".to_string()),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let wire = typed.encode_to_vec();

    // 1. Decode arbitrary wire bytes against a runtime descriptor.
    let mut dyn_msg = DynamicMessage::decode(descriptor.clone(), wire.as_slice())?;
    println!("decoded {} field(s):", dyn_msg.fields().count());
    for (field, value) in dyn_msg.fields() {
        println!("  • {} = {:?}", field.name(), value);
    }

    // 2. Field access by name and by tag number — both forms are dual.
    let name = dyn_msg
        .get_field_by_name("name")
        .expect("`name` exists on Library");
    assert!(matches!(name.as_ref(), Value::String(s) if s == "Bath Public Library"));

    let books = dyn_msg.get_field_by_number(2).expect("`books` is field #2");
    if let Value::List(items) = books.as_ref() {
        println!("books in pool: {}", items.len());
    }

    // 3. Mutation: rename the library and append a tag entry.
    dyn_msg.set_field_by_name("name", Value::String("Pump Room Library".into()));

    let tags = dyn_msg.get_field_by_name_mut("tags").expect("tags");
    if let Value::Map(map) = tags {
        map.insert(
            MapKey::String("opened".into()),
            Value::String("1769".into()),
        );
    }

    // 4. Validating mutation surfaces a typed error rather than panicking.
    let bad = dyn_msg.try_set_field_by_name("name", Value::I32(42));
    assert!(matches!(bad, Err(SetFieldError::InvalidType { .. })));

    // 5. Transcode back to the typed struct via the wire format.
    let mutated: Library = dyn_msg.transcode_to()?;
    assert_eq!(mutated.name, "Pump Room Library");
    assert_eq!(mutated.tags.get("opened").map(String::as_str), Some("1769"));

    // 6. transcode_to_dynamic on the typed side is the symmetric move.
    let round_trip = mutated.transcode_to_dynamic();
    assert_eq!(round_trip.encode_to_vec(), dyn_msg.encode_to_vec());

    println!("\nfinal wire size: {} bytes", dyn_msg.encoded_len());
    println!("transcode round-trip: ok");
    Ok(())
}
