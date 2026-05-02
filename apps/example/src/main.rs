//! End-to-end demo: walk a buffa-generated message tree by descriptor.
//!
//! For Phase 2 demos (DynamicMessage, JSON, textproto, view reflection)
//! see `cargo run --example <name> -p buffa-reflect-example`.

use buffa_reflect::{Kind, MessageDescriptor, ReflectMessage, ReflectMessageView};
use buffa_reflect_example::{Genre, Library, library};

fn main() {
    let book = library::Book {
        id: "b-001".to_string(),
        title: "Pride and Prejudice".to_string(),
        authors: vec!["Jane Austen".to_string()],
        genre: ::buffa::EnumValue::Known(Genre::GENRE_FICTION),
        excerpts: vec![library::book::Excerpt {
            page: 1,
            text: "It is a truth universally acknowledged…".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    };
    print_descriptor("Book", book.descriptor());

    let library_msg = Library {
        name: "Bath Public Library".to_string(),
        books: vec![book],
        ..Default::default()
    };
    print_descriptor("Library", library_msg.descriptor());

    print_descriptor("Excerpt", library::book::Excerpt::default().descriptor());

    let bytes = ::buffa::Message::encode_to_vec(&library_msg);
    let view: buffa_reflect_example::__buffa::view::LibraryView<'_> = ::buffa::DecodeOptions::new()
        .decode_view(bytes.as_slice())
        .expect("library decodes as view");
    let view_descriptor = view.descriptor();
    println!("== Library (via view): {} ==", view_descriptor.full_name());
    assert_eq!(
        view_descriptor.full_name(),
        library_msg.descriptor().full_name()
    );
    assert_eq!(
        view_descriptor.fields().count(),
        library_msg.descriptor().fields().count()
    );
}

fn print_descriptor(label: &str, descriptor: MessageDescriptor) {
    println!("== {label}: {} ==", descriptor.full_name());
    for field in descriptor.fields() {
        let kind_repr = match field.kind() {
            Kind::Message(m) => format!("message<{}>", m.full_name()),
            Kind::Enum(e) => format!("enum<{}>", e.full_name()),
            other => format!("{other:?}"),
        };
        let oneof = field
            .containing_oneof()
            .map(|o| {
                format!(
                    " ⊂ oneof {}{}",
                    o.name(),
                    if o.is_synthetic() { " (synthetic)" } else { "" }
                )
            })
            .unwrap_or_default();
        println!(
            "  #{:<2} {:<22} {:<10?} {}{}",
            field.number(),
            field.name(),
            field.cardinality(),
            kind_repr,
            oneof,
        );
    }
    println!();
}
