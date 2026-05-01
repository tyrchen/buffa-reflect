# prost-reflect — Architecture Research

Source: `vendors/prost-reflect/` (submodule of `andrewhickman/prost-reflect`).

This document is a working reference for the prost-reflect implementation. We use it as the canonical model for what "runtime reflection over generated protobuf code" looks like in Rust. The aim is to extract the **shape** of its solution (crate split, traits, build-time orchestration, on-disk artifact, codegen hooks), not to copy code.

---

## 1. Workspace layout

Five crates in the workspace (see `vendors/prost-reflect/Cargo.toml`):

| Crate | Role |
| --- | --- |
| `prost-reflect` | Runtime: `DescriptorPool`, `MessageDescriptor`, `FieldDescriptor`, `DynamicMessage`, `ReflectMessage` trait, optional serde/text-format. |
| `prost-reflect-build` | Build-script helper. Runs prost-build with a captured `FileDescriptorSet`, then re-runs it injecting `#[derive(ReflectMessage)]` + `#[prost_reflect(...)]` attributes onto every message. |
| `prost-reflect-derive` | `#[derive(ReflectMessage)]` proc macro. Emits an `impl ReflectMessage` that resolves a `MessageDescriptor` from a configured pool. |
| `prost-reflect-tests` | Round-trip + descriptor-API + JSON + text-format tests. Build script uses `prost-reflect-build`. |
| `prost-reflect-conformance-tests` | Runs the upstream protobuf conformance suite through `DynamicMessage`. |

MSRV in `rust-toolchain.toml` and crate `Cargo.toml`s: **Rust 1.82**. Pins `prost = "0.14"`, `prost-types = "0.14"`.

---

## 2. Runtime data model (`prost-reflect`)

### DescriptorPool

`prost-reflect/src/descriptor/mod.rs:130-210` — the pool is `Arc<DescriptorPoolInner>` so cloning is O(1).

```rust
pub struct DescriptorPool { inner: Arc<DescriptorPoolInner> }

struct DescriptorPoolInner {
    names:      HashMap<Box<str>, Definition>, // FQN → typed index
    file_names: HashMap<Box<str>, FileIndex>,
    files:      Vec<FileDescriptorInner>,
    messages:   Vec<MessageDescriptorInner>,
    enums:      Vec<EnumDescriptorInner>,
    extensions: Vec<ExtensionDescriptorInner>,
    services:   Vec<ServiceDescriptorInner>,
}
```

Public descriptor handles are tiny `(pool, index)` pairs (cheap to clone, comparable by index):

```rust
pub struct FileDescriptor    { pool: DescriptorPool, index: FileIndex }
pub struct MessageDescriptor { pool: DescriptorPool, index: MessageIndex }
pub struct FieldDescriptor   { message: MessageDescriptor, index: FieldIndex }
pub struct EnumDescriptor    { pool: DescriptorPool, index: EnumIndex }
pub struct OneofDescriptor   { message: MessageDescriptor, index: OneofIndex }
pub struct ServiceDescriptor { pool: DescriptorPool, index: ServiceIndex }
pub struct MethodDescriptor  { service: ServiceDescriptor, index: MethodIndex }
```

Internal records hold cached identity (`full_name`, `name`, parent, source location), structural data (fields, oneofs, enum values), and pre-built lookup maps (`field_numbers: BTreeMap<u32, FieldIndex>`, `field_names: HashMap<Box<str>, FieldIndex>`, `field_json_names: HashMap<Box<str>, FieldIndex>`).

### Public API surface

Re-exports from `prost-reflect/src/lib.rs`:

- Descriptor: `Cardinality`, `DescriptorError`, `DescriptorPool`, `EnumDescriptor`, `EnumValueDescriptor`, `ExtensionDescriptor`, `FieldDescriptor`, `FileDescriptor`, `Kind`, `MessageDescriptor`, `MethodDescriptor`, `OneofDescriptor`, `ServiceDescriptor`, `Syntax`.
- Dynamic: `DynamicMessage`, `MapKey`, `SetFieldError`, `UnknownField`, `Value`.
- Trait: `ReflectMessage`.
- Re-exports: `prost`, `prost::bytes`, `prost_types`.
- Behind `derive`: `prost_reflect_derive::ReflectMessage`.
- Behind `serde`: `DeserializeOptions`, `SerializeOptions`.
- Behind `text-format`: `text_format` module.

### Loading descriptors

`prost-reflect/src/descriptor/api.rs:159-320`. The encoded input is the standard `google.protobuf.FileDescriptorSet` proto (raw bytes — no compression, no base64).

Construction APIs:

- `DescriptorPool::new()` — empty.
- `DescriptorPool::from_file_descriptor_set(FileDescriptorSet)` — from a decoded set.
- `DescriptorPool::decode<B: Buf>(bytes)` — decode-then-build. The common path used by generated code.
- `add_file_descriptor_set`, `add_file_descriptor_protos`, `add_file_descriptor_proto`, `decode_file_descriptor_set`, `decode_file_descriptor_proto` — incremental.
- Global pool: `DescriptorPool::global()`, `decode_global_file_descriptor_set`, `add_global_file_descriptor_proto` (`prost-reflect/src/descriptor/global.rs`). Pre-seeded with WKTs.

Build-time validation (`prost-reflect/src/descriptor/build/mod.rs`): name resolution across imports, field-number ranges (1..=536_870_911 minus reserved 19_000..=20_000), proto3 enum-zero rule, no proto3 required, default-value validity, extension-range membership.

### DynamicMessage

`prost-reflect/src/dynamic/mod.rs`. Sparse field set keyed by `FieldDescriptor`, value is `enum Value { Bool, I32, I64, U32, U64, F32, F64, String, Bytes, EnumNumber, Message(DynamicMessage), List(Vec<Value>), Map(HashMap<MapKey, Value>) }`. Implements `prost::Message` so the same `encode_to_vec` / `decode` path works on a runtime-typed message — encoding consults the descriptor for wire type and packing rules.

### ReflectMessage trait

`prost-reflect/src/reflect/mod.rs:9-28`:

```rust
pub trait ReflectMessage: Message {
    fn descriptor(&self) -> MessageDescriptor;
    fn transcode_to_dynamic(&self) -> DynamicMessage where Self: Sized { /* ... */ }
}
```

This is the **only** runtime contract bridging a generated prost struct to reflection. The derive emits a one-line resolver into a configured pool.

---

## 3. Derive (`prost-reflect-derive`)

`prost-reflect-derive/src/lib.rs:43-135`. The macro accepts three attributes:

- `#[prost_reflect(descriptor_pool = "path::to::POOL")]` — `POOL` evaluates to a `&DescriptorPool` (typically a `Lazy`/`OnceLock` static).
- `#[prost_reflect(file_descriptor_set_bytes = "path::to::BYTES")]` — `BYTES` is a `&'static [u8]`. The expansion lazily seeds the **global** pool via `std::sync::Once`.
- `#[prost_reflect(message_name = "package.MyMessage")]` — required.

Expansion (descriptor-pool form):

```rust
impl ::prost_reflect::ReflectMessage for MyMessage {
    fn descriptor(&self) -> ::prost_reflect::MessageDescriptor {
        crate::DESCRIPTOR_POOL
            .get_message_by_name("package.MyMessage")
            .expect("descriptor for `package.MyMessage` not found")
    }
}
```

Bytes form expands to `static INIT: Once = Once::new(); INIT.call_once(|| DescriptorPool::decode_global_file_descriptor_set(BYTES).unwrap()); DescriptorPool::global().get_message_by_name(...)`.

---

## 4. Build-time integration (`prost-reflect-build`)

`prost-reflect-build/src/lib.rs:141-199`. The `Builder` is small:

```rust
pub struct Builder {
    file_descriptor_set_path: PathBuf,        // default: $OUT_DIR/file_descriptor_set.bin
    descriptor_pool_expr: Option<String>,
    file_descriptor_set_bytes_expr: Option<String>,
}
```

User picks one of `descriptor_pool(...)` or `file_descriptor_set_bytes(...)`.

The orchestration is the key recipe:

1. Call `prost_build::Config::file_descriptor_set_path(path).compile_protos(...)` once. This invokes `protoc`, writes the FDS to disk, and runs prost-build. `Config` is consumed so a fresh one is used per compile.
2. Read FDS bytes and decode into a `DescriptorPool` purely to enumerate messages.
3. For every `message.full_name()`, append three `Config::type_attribute` entries:
   - `#[derive(::prost_reflect::ReflectMessage)]`
   - `#[prost_reflect(message_name = "package.MyMessage")]`
   - either `#[prost_reflect(descriptor_pool = "...")]` or `#[prost_reflect(file_descriptor_set_bytes = "...")]`
4. Call `prost_build::Config::skip_protoc_run().compile_protos(...)` again to regenerate code with the attributes injected. (`skip_protoc_run` reuses the on-disk FDS instead of re-running `protoc`.)

Artifacts in `OUT_DIR`:

- `file_descriptor_set.bin` — raw `FileDescriptorSet` proto bytes.
- `<package>.rs` files — prost-build output, now decorated with `#[derive(ReflectMessage)]`.

The user is responsible for one extra line in `lib.rs` to expose the bytes or the pool:

```rust
// option A — bytes form
const FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

// option B — pool form
static DESCRIPTOR_POOL: Lazy<DescriptorPool> = Lazy::new(|| {
    DescriptorPool::decode(
        include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin")).as_ref(),
    ).unwrap()
});
```

---

## 5. Generated code shape

For `prost-reflect-tests/src/test.proto` (`message Scalars { ... }`), build emits something like:

```rust
#[derive(Clone, PartialEq, ::prost::Message, ::prost_reflect::ReflectMessage)]
#[prost_reflect(file_descriptor_set_bytes = "crate::DESCRIPTOR_POOL_BYTES",
                message_name = "test.Scalars")]
pub struct Scalars {
    #[prost(double, tag = "1")]   pub double: f64,
    #[prost(string, tag = "14")]  pub string: ::prost::alloc::string::String,
    /* ... */
}
```

That is the whole runtime hook — one derive + two attributes. Everything else (field iteration, JSON, dynamic encode/decode) goes through the `MessageDescriptor` returned by the trait.

---

## 6. Tests and conformance

- `prost-reflect-tests/build.rs` shows the canonical `Builder` wiring (`file_descriptor_set_bytes("crate::DESCRIPTOR_POOL_BYTES")` + `compile_protos_with_config`). Fixtures: `Scalars`, `ScalarArrays`, `ComplexType`, `WellKnownTypes`, plus extension/option/import variants.
- `prost-reflect-conformance-tests/` runs the upstream `conformance_test_runner` against a binary that round-trips through `DynamicMessage`. Known-failure list lives next to the binary.
- `prost-reflect-tests/src/{decode,desc,json,text_format}.rs` exercise the four major reflective paths (binary round-trip, descriptor introspection, canonical JSON, textproto).

---

## 7. What we keep, what we drop

What the buffa equivalent should keep:

- **Single small runtime trait** (`ReflectMessage::descriptor`) as the only generated-code/runtime contract.
- **Descriptor pool with cheap (Arc) clones and indexed handles**.
- **One descriptor artifact in `OUT_DIR`** (`file_descriptor_set.bin`).
- **Build-script helper that runs the codegen pipeline twice (or equivalently): once to capture the FDS, then again with attributes injected on every message**.
- **Two configuration shapes** for descriptor source (bytes-include vs. user-owned static pool) — they serve different lifetime/dedup tradeoffs.

What is prost-specific and we don't need to copy verbatim:

- The `DynamicMessage` ↔ `prost::Message` interop trick. Buffa has its own `Message` trait and a parallel `MessageView<'a>`, so a future `DynamicMessage` design is a separate workstream.
- Global mutable pool (`DescriptorPool::global` + `OnceLock<Mutex<…>>`). Useful but optional; we can start with a per-crate static pool.
- `prost-reflect-build`'s "run prost-build twice" workaround. Buffa-build's `Config::descriptor_set(path)` already accepts a precompiled FDS, so we get the same effect by orchestrating protoc/buf ourselves once and then driving buffa-build with the precomputed bytes.
