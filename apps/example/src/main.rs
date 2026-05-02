//! End-to-end demo: walk a buffa-generated message tree by descriptor.
//!
//! Generated buffa code carries no docstrings, so we silence
//! `missing_docs` for the included module tree only — every other public
//! item in this binary still requires a doc comment.

#![allow(
    missing_docs,
    non_camel_case_types,
    clippy::derivable_impls,
    clippy::doc_lazy_continuation,
    clippy::module_inception,
    clippy::uninlined_format_args,
    reason = "buffa codegen preserves protobuf names verbatim (e.g. SCREAMING_SNAKE enum \
              variants) and emits straightforward impls; the lint set is tuned for hand-written \
              code, not generated code."
)]

use buffa_reflect::{Kind, ReflectMessage, ReflectMessageView};

/// Embedded descriptor-set bytes, populated by `buffa-reflect-build`.
pub const FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

buffa::include_proto!("acme.api.v1");

include!(concat!(env!("OUT_DIR"), "/_reflect_views.rs"));

fn main() {
    // Construct a deeply-nested generated message.
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

    // Top-level reflection over the nested type.
    print_descriptor("Book", book.descriptor());

    // Top-level message at the package root.
    let library = Library {
        name: "Bath Public Library".to_string(),
        books: vec![book],
        ..Default::default()
    };
    print_descriptor("Library", library.descriptor());

    // Nested-nested type.
    let excerpt = library::book::Excerpt::default();
    print_descriptor("Excerpt", excerpt.descriptor());

    // View-type reflection: decode the library message as a view and
    // walk its descriptor without ever owning the data.
    let bytes = ::buffa::Message::encode_to_vec(&library);
    let view: __buffa::view::LibraryView<'_> = ::buffa::DecodeOptions::new()
        .decode_view(bytes.as_slice())
        .expect("library decodes as view");
    let view_descriptor = view.descriptor();
    println!("== Library (via view): {} ==", view_descriptor.full_name());
    // Both descriptors describe the same proto message; they may live
    // in distinct pool clones (one per static OnceLock site) but their
    // FQNs and field counts are identical.
    assert_eq!(
        view_descriptor.full_name(),
        library.descriptor().full_name()
    );
    assert_eq!(
        view_descriptor.fields().count(),
        library.descriptor().fields().count()
    );
}

fn print_descriptor(label: &str, descriptor: buffa_reflect::MessageDescriptor) {
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
