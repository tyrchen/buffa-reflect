# buffa-reflect

Runtime reflection for the [`buffa`](https://crates.io/crates/buffa)
protobuf implementation. Designed as a near-drop-in for
[`prost-reflect`](https://crates.io/crates/prost-reflect):

| crate | what it gives you |
| --- | --- |
| `buffa-reflect`        | `DescriptorPool`, `MessageDescriptor`, `FieldDescriptor`, `EnumDescriptor`, `OneofDescriptor`, `ReflectMessage`, `ReflectMessageView`, `DynamicMessage`, proto3 JSON (serde), textproto |
| `buffa-reflect-derive` | `#[derive(ReflectMessage)]` proc-macro (re-exported by the runtime crate's `derive` feature) |
| `buffa-reflect-build`  | `build.rs` integration that runs `protoc`/`buf`, emits `OUT_DIR/file_descriptor_set.bin`, decorates every generated message with the derive, and (optionally) `impl ReflectMessageView` for every borrowed `*View<'a>` |

Phase 1 (descriptor pool + build script) and Phase 2 (`DynamicMessage`,
JSON, textproto, gRPC server reflection, view reflection) are both
shipped — see [`specs/`](specs/index.md) for the design history.

## Cargo features (`buffa-reflect`)

| feature       | default | what it pulls in |
| ------------- | ------- | ---------------- |
| `derive`      | yes     | the `#[derive(ReflectMessage)]` re-export |
| `dynamic`     | yes     | `DynamicMessage`, `Value`, `MapKey`, `transcode_to_dynamic` |
| `serde`       | no      | proto3 canonical JSON (`serde::Serialize` + `DeserializeSeed`) |
| `text-format` | no      | textproto encode (`to_text_format`) and parse (`parse_text_format`) |

```toml
buffa-reflect = { version = "0.1", features = ["serde", "text-format"] }
```

## Quick start

```toml
# Cargo.toml
[dependencies]
buffa = "0.4"
buffa-reflect = "0.1"

[build-dependencies]
buffa-reflect-build = "0.1"
```

```rust,ignore
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .files(&["proto/acme/api/v1/library.proto"])
        .includes(&["proto/"])
        .compile()?;
    Ok(())
}
```

```rust,ignore
// src/lib.rs
pub const FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

buffa::include_proto!("acme.api.v1");

// One `impl ReflectMessageView` block per generated `*View<'a>`,
// auto-emitted because `file_descriptor_set_bytes(..)` is configured.
// Pass `.generate_view_reflection(false)` on the builder to opt out.
include!(concat!(env!("OUT_DIR"), "/_reflect_views.rs"));
```

## Phase 1 — walk descriptors

Every generated message implements `ReflectMessage`:

```rust,ignore
use buffa_reflect::{Kind, ReflectMessage};

let book = library::Book::default();
let descriptor = book.descriptor();
for field in descriptor.fields() {
    println!(
        "#{:<2} {:<20} {:?}",
        field.number(),
        field.name(),
        field.cardinality(),
    );
    if let Kind::Enum(e) = field.kind() {
        for value in e.values() {
            println!("    = {} = {}", value.name(), value.number());
        }
    }
}
```

Lookups by JSON name, proto name, or tag number are available on both
`MessageDescriptor` and `DescriptorPool`:

```rust,ignore
let pool = buffa_reflect::DescriptorPool::decode(FILE_DESCRIPTOR_SET_BYTES)?;
let library_desc = pool.get_message_by_name("acme.api.v1.Library").unwrap();
let books = library_desc.get_field_by_name("books").unwrap();
assert!(books.is_list());
```

## Phase 2 — `DynamicMessage`

`DynamicMessage` lets one binary read, mutate, and re-encode a message
of *any* shape that lives in the pool — without knowing the static Rust
type:

```rust,ignore
use buffa_reflect::{DescriptorPool, DynamicMessage, MapKey, Value};

let pool = DescriptorPool::decode(FILE_DESCRIPTOR_SET_BYTES)?;
let descriptor = pool.get_message_by_name("acme.api.v1.Library").unwrap();

// Decode arbitrary wire bytes against a runtime descriptor.
let mut dyn_msg = DynamicMessage::decode(descriptor.clone(), wire_bytes)?;

// Inspect.
for (field, value) in dyn_msg.fields() {
    println!("{} = {value:?}", field.name());
}

// Mutate by name or by tag number; both forms are dual.
dyn_msg.set_field_by_name("name", Value::String("Pump Room Library".into()));
if let Some(Value::Map(tags)) = dyn_msg.get_field_by_name_mut("tags") {
    tags.insert(MapKey::String("opened".into()), Value::String("1769".into()));
}

// Validate without panicking.
let bad = dyn_msg.try_set_field_by_name("name", Value::I32(42));
assert!(matches!(bad, Err(buffa_reflect::SetFieldError::InvalidType { .. })));

// Re-encode (preserves unknown fields, deterministic tag-number order).
let bytes = dyn_msg.encode_to_vec();
```

`DynamicMessage` is symmetric with the typed types via
`transcode_to`/`transcode_from`:

```rust,ignore
use buffa::Message as _;
use buffa_reflect::ReflectMessage as _;

let typed: Library = /* ... */;

// typed → dynamic (one wire round-trip; specialised to clone for
// `DynamicMessage` itself).
let dyn_msg = typed.transcode_to_dynamic();

// dynamic → typed.
let back: Library = dyn_msg.transcode_to()?;
assert_eq!(typed.encode_to_vec(), back.encode_to_vec());
```

Runnable: `cargo run -p buffa-reflect-example --example dynamic_message`.

## Phase 2 — proto3 canonical JSON (`serde`)

Enable the `serde` feature.

```rust,ignore
use buffa_reflect::{DeserializeOptions, DynamicMessage, SerializeOptions};
use serde::de::DeserializeSeed as _;

// Serialize: `DynamicMessage: serde::Serialize`.
let json = serde_json::to_string_pretty(&dyn_msg)?;

// Deserialize: `MessageDescriptor: DeserializeSeed<'de>` — the
// descriptor itself is the seed, no helper struct needed.
let mut de = serde_json::de::Deserializer::from_str(&json);
let parsed: DynamicMessage = descriptor.clone().deserialize(&mut de)?;
assert_eq!(dyn_msg, parsed);

// Knobs match prost-reflect for cross-ecosystem familiarity.
let mut buf = Vec::new();
let mut ser = serde_json::Serializer::new(&mut buf);
dyn_msg.serialize_with_options(
    &mut ser,
    &SerializeOptions::new()
        .stringify_64_bit_integers(false)
        .use_proto_field_name(true)
        .use_enum_numbers(true)
        .skip_default_fields(false),
)?;

// Strict mode rejects unknown fields (default ignores per the proto3
// JSON spec).
let strict = DynamicMessage::deserialize_with_options(
    descriptor.clone(),
    &mut serde_json::de::Deserializer::from_str(r#"{ "futureField": 1 }"#),
    &DeserializeOptions::new().deny_unknown_fields(true),
);
assert!(strict.is_err());
```

Runnable: `cargo run -p buffa-reflect-example --example json`.

## Phase 2 — textproto (`text-format`)

Enable the `text-format` feature.

```rust,ignore
use buffa_reflect::{DynamicMessage, FormatOptions};

// Default: single-line, machine-friendly (matches `protoc --decode`).
let compact = dyn_msg.to_text_format();

// Multi-line, indented; drop fields equal to the proto default.
let pretty = dyn_msg.to_text_format_with_options(
    &FormatOptions::new().pretty(true).skip_default_fields(true),
);

// Round-trip back into a `DynamicMessage` of the same descriptor.
let parsed = DynamicMessage::parse_text_format(descriptor.clone(), &pretty)?;
assert_eq!(dyn_msg, parsed);
```

Runnable: `cargo run -p buffa-reflect-example --example text_format`.

## Phase 2 — view-type reflection

The auto-generated `_reflect_views.rs` include adds `ReflectMessageView`
to every borrowed `*View<'a>`. Reflection works on the zero-copy decode
path without owning the data:

```rust,ignore
use buffa_reflect::{Kind, ReflectMessageView};

let view: __buffa::view::LibraryView<'_> =
    buffa::DecodeOptions::new().decode_view(wire_bytes)?;
let descriptor = view.descriptor();
println!("{}: {} fields", descriptor.full_name(), descriptor.fields().len());

// Generic helpers can take any `ReflectMessageView<'_>`.
fn count_message_fields<'a, V: ReflectMessageView<'a>>(v: &V) -> usize {
    v.descriptor()
        .fields()
        .filter(|f| matches!(f.kind(), Kind::Message(_)))
        .count()
}
```

Runnable: `cargo run -p buffa-reflect-example --example view_reflection`.

## Phase 2 — gRPC server reflection

[`examples/grpc-reflection/`](examples/grpc-reflection/) ships
`buffa-grpc-reflection`, a drop-in for `tonic-reflection` backed by a
`DescriptorPool`. It lives outside the workspace to keep `tonic` /
`prost` out of the parent `Cargo.lock`.

```rust,ignore
let pool = buffa_reflect::DescriptorPool::decode(FILE_DESCRIPTOR_SET_BYTES)?;
let (v1, v1alpha) = buffa_grpc_reflection::Builder::from_pool(pool).build();

tonic::transport::Server::builder()
    .add_service(v1)        // grpc.reflection.v1.ServerReflection
    .add_service(v1alpha)   // grpc.reflection.v1alpha.ServerReflection
    .add_service(my_service)
    .serve(addr).await?;
```

Once the server is up, `grpcurl localhost:50051 list` enumerates every
service in the pool.

## End-to-end demos

[`apps/example/`](apps/example/) compiles a small `library.proto` with
nested messages, oneofs, maps, and an enum, then exercises every Phase 2
code path through cargo examples. From the workspace root:

```sh
cargo run -p buffa-reflect-example                          # Phase 1: descriptor walk
cargo run -p buffa-reflect-example --example dynamic_message
cargo run -p buffa-reflect-example --example json
cargo run -p buffa-reflect-example --example text_format
cargo run -p buffa-reflect-example --example view_reflection
```

[`examples/equivalence/`](examples/equivalence/) cross-checks
descriptor parsing against `prost-reflect` over the same
`FileDescriptorSet`.

## Workspace layout

```
crates/
  buffa-reflect/        # runtime descriptor pool, DynamicMessage, JSON, textproto
  buffa-reflect-derive/ # proc-macro
  buffa-reflect-build/  # build.rs library
apps/
  example/              # end-to-end demos referenced by this README
examples/               # leaf workspaces (kept out of the main Cargo.lock)
  equivalence/          # parser equivalence vs. prost-reflect
  grpc-reflection/      # buffa-grpc-reflection (tonic gRPC server reflection)
specs/                  # PRDs, design docs, impl plans
docs/research/          # background research used to scope each phase
```

## License

Distributed under the terms of MIT. See [LICENSE](LICENSE.md).

Copyright 2026 Tyr Chen
