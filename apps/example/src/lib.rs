//! Generated proto types and embedded descriptor-set bytes for the
//! `buffa-reflect` example.
//!
//! Sharing the includes through a library lets the binary in
//! `src/main.rs` and every `examples/*.rs` see the same `Library`,
//! `library::Book`, view types, and `FILE_DESCRIPTOR_SET_BYTES`
//! constant.

#![allow(
    missing_docs,
    non_camel_case_types,
    clippy::derivable_impls,
    clippy::doc_lazy_continuation,
    clippy::module_inception,
    clippy::uninlined_format_args,
    reason = "buffa codegen preserves protobuf names verbatim and emits straightforward impls; \
              the lint set is tuned for hand-written code, not generated code."
)]

/// Embedded descriptor-set bytes, populated by `buffa-reflect-build`.
pub const FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

buffa::include_proto!("acme.api.v1");

include!(concat!(env!("OUT_DIR"), "/_reflect_views.rs"));
