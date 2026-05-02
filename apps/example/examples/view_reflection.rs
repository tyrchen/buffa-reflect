//! View-type reflection: introspect a borrowed `*View<'a>` decoded
//! straight off the wire, with no owned-message allocation.

use buffa::Message as _;
use buffa_reflect::{Kind, ReflectMessage, ReflectMessageView};
use buffa_reflect_example::{__buffa, Genre, Library, library};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let owned = Library {
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
    let wire = owned.encode_to_vec();

    // Zero-copy decode: `LibraryView<'_>` borrows directly from `wire`.
    let view: __buffa::view::LibraryView<'_> =
        ::buffa::DecodeOptions::new().decode_view(wire.as_slice())?;

    // The view exposes the same `MessageDescriptor` as the owned form.
    let view_desc = view.descriptor();
    let owned_desc = owned.descriptor();
    assert_eq!(view_desc.full_name(), owned_desc.full_name());
    assert_eq!(view_desc.fields().count(), owned_desc.fields().count());

    println!("== {} (via view) ==", view_desc.full_name());
    for field in view_desc.fields() {
        let kind = match field.kind() {
            Kind::Message(m) => format!("message<{}>", m.full_name()),
            Kind::Enum(e) => format!("enum<{}>", e.full_name()),
            other => format!("{other:?}"),
        };
        println!(
            "  #{:<2} {:<14} {:<10?} {kind}",
            field.number(),
            field.name(),
            field.cardinality(),
        );
    }

    // Generic helpers can take any `ReflectMessageView<'_>` and
    // introspect without owning the data.
    fn count_message_fields<'a, V: ReflectMessageView<'a>>(view: &V) -> usize {
        view.descriptor()
            .fields()
            .filter(|f| matches!(f.kind(), Kind::Message(_)))
            .count()
    }
    println!(
        "\nmessage-typed fields on Library: {}",
        count_message_fields(&view)
    );
    Ok(())
}
