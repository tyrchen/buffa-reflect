# buffa-reflect

Runtime reflection for the [`buffa`](https://crates.io/crates/buffa)
protobuf implementation. Built as a parallel to
[`prost-reflect`](https://crates.io/crates/prost-reflect):

| crate | what it gives you |
| --- | --- |
| `buffa-reflect` | `DescriptorPool`, `MessageDescriptor`, `FieldDescriptor`, `EnumDescriptor`, `OneofDescriptor`, `ReflectMessage` |
| `buffa-reflect-derive` | `#[derive(ReflectMessage)]` proc-macro (re-exported through the `derive` feature on the runtime crate) |
| `buffa-reflect-build` | `build.rs` integration that drives `protoc` / `buf`, emits `OUT_DIR/file_descriptor_set.bin`, and decorates every generated message with the derive |

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
```

```rust,ignore
// anywhere
use buffa_reflect::ReflectMessage;

let book = library::Book::default();
let descriptor = book.descriptor();
for field in descriptor.fields() {
    println!("{} #{} {:?}", field.name(), field.number(), field.kind());
}
```

A complete worked example lives under [`apps/example/`](apps/example/).

## Phase 1 scope

This release covers the build-script + minimum-viable runtime so consumers
can introspect generated messages and look fields up by name / number /
JSON name. Phase 2 (`DynamicMessage`, JSON / textproto transcoding,
`grpc.reflection.v1` shim) is tracked separately under
[`specs/`](specs/index.md).

## Workspace layout

```
crates/
  buffa-reflect/        # runtime descriptor pool + handles
  buffa-reflect-derive/ # proc-macro
  buffa-reflect-build/  # build.rs library
apps/
  example/              # end-to-end demo (proto → derive → reflection walk)
```

## License

Distributed under the terms of MIT. See [LICENSE](LICENSE.md).

Copyright 2026 Tyr Chen
