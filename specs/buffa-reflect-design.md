# buffa-reflect — Design

Companion to [buffa-reflect-prd.md](./buffa-reflect-prd.md). Pre-reads: [docs/research/prost-reflect-architecture.md](../docs/research/prost-reflect-architecture.md), [docs/research/buffa-architecture.md](../docs/research/buffa-architecture.md), [docs/research/gap-analysis.md](../docs/research/gap-analysis.md).

This design covers Phase 1 (build-script crate plus minimum-viable runtime). Phase 2 (`DynamicMessage`, JSON/text, conformance) is referenced where it would constrain Phase 1 design choices, otherwise deferred.

---

## 1. Workspace shape

The repo will host three new crates plus the existing `buffa-reflect-core` placeholder. Final layout under this workspace:

```
crates/
  buffa-reflect/                # runtime: DescriptorPool, ReflectMessage, ...
  buffa-reflect-build/          # build-script library (the main deliverable)
  buffa-reflect-derive/         # proc-macro: #[derive(ReflectMessage)]
apps/
  server/                       # can be removed
vendors/
  buffa/                        # submodule
  prost-reflect/                # submodule (reference reading only)
```

`crates/core/` (the existing `buffa-reflect-core` placeholder) is renamed to `crates/buffa-reflect/` — keeping the package name in `Cargo.toml` aligned with the public crate name. The workspace `Cargo.toml` `members` glob `crates/*` continues to pick everything up.

**Naming**: the published crate is `buffa-reflect`. There is no separate `-core`. The PRD audience expects parity with `prost-reflect`'s naming, and "core" adds nothing.

---

## 2. Dependency graph

```
                     +----------------------+
                     |    buffa-descriptor  |   (vendored; FileDescriptorSet types)
                     +----------+-----------+
                                ^
                                | (re-export `descriptor::*` for advanced users)
                                |
+-----------------+       +-----+------+        +-------------------------+
| buffa (runtime) | <---- | buffa-     | <----- | buffa-reflect-derive    |
|  Message trait  |       | reflect    |        |   #[derive(ReflectMsg)] |
+-----------------+       +------+-----+        +-------------------------+
                                 ^
                                 | (compile-time only; build dep)
                                 |
                          +------+------+         +------------------+
                          | buffa-      | ------> | buffa-build      |
                          | reflect-    |         |  (vendored crate |
                          | build       |         |   today; pin via |
                          +-------------+         |   path = ../...) |
                                                  +------------------+
```

Hard rules:

- `buffa-reflect` does **not** depend on `buffa-build` or `buffa-reflect-build` (so it remains usable from crates whose generated code was checked in elsewhere).
- `buffa-reflect-build` depends on `buffa-reflect` only as a `build-dependencies` entry — it doesn't need to link the runtime, only know its public attribute namespace. (The downstream crate links it as a regular dependency.)
- `buffa-reflect-derive` is a `proc-macro = true` crate; it does not depend on `buffa-reflect` (the macro emits paths into the user's `::buffa_reflect`).

For Phase 1, we depend on the buffa workspace via the **vendored submodule** with `path = "../../vendors/buffa/buffa-build"` etc. When a buffa release on crates.io is acceptable upstream, swap to `version = "0.x"` plus an optional `path = "..."` override, mirroring tonic-build's pattern.

---

## 3. `buffa-reflect-build` — the main deliverable

### 3.1 Public API

```rust
//! buffa-reflect-build/src/lib.rs

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Builder {
    file_descriptor_set_path: PathBuf,    // default: $OUT_DIR/file_descriptor_set.bin
    descriptor_pool_expr: Option<String>,
    file_descriptor_set_bytes_expr: Option<String>,
    descriptor_source: DescriptorSource,
    files: Vec<PathBuf>,
    includes: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    extra_type_attributes: Vec<(String, String)>,  // user-supplied passthrough
    extra_field_attributes: Vec<(String, String)>,
    codegen_extra: BuffaConfigKnobs,               // generate_views/json/text/...
}

#[derive(Debug, Clone, Default)]
enum DescriptorSource {
    #[default] Protoc,
    Buf,
    Precompiled(PathBuf),
}

impl Builder {
    pub fn new() -> Self;

    // descriptor wiring -- one of these must be set
    pub fn descriptor_pool(self, expr: impl Into<String>) -> Self;
    pub fn file_descriptor_set_bytes(self, expr: impl Into<String>) -> Self;

    // descriptor-set artifact location override (default: $OUT_DIR/file_descriptor_set.bin)
    pub fn file_descriptor_set_path(self, path: impl Into<PathBuf>) -> Self;

    // input
    pub fn files(self, files: &[impl AsRef<Path>]) -> Self;
    pub fn includes(self, includes: &[impl AsRef<Path>]) -> Self;
    pub fn use_buf(self) -> Self;
    pub fn descriptor_set(self, path: impl Into<PathBuf>) -> Self;
    pub fn out_dir(self, dir: impl Into<PathBuf>) -> Self;

    // codegen passthrough (delegates to buffa_build::Config)
    pub fn type_attribute(self, path: impl Into<String>, attr: impl Into<String>) -> Self;
    pub fn field_attribute(self, path: impl Into<String>, attr: impl Into<String>) -> Self;
    pub fn generate_views(self, enabled: bool) -> Self;
    pub fn generate_json(self, enabled: bool) -> Self;
    pub fn generate_text(self, enabled: bool) -> Self;
    pub fn extern_path(self, proto_path: impl Into<String>, rust_path: impl Into<String>) -> Self;
    pub fn use_bytes_type_in(self, paths: &[impl AsRef<str>]) -> Self;
    // ... mirror the rest of buffa_build::Config knobs the user would otherwise lose access to.

    // terminal
    pub fn compile(self) -> Result<(), Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("descriptor source not configured: call .descriptor_pool() or .file_descriptor_set_bytes()")]
    MissingDescriptorBinding,
    #[error("OUT_DIR not set and no out_dir() configured")]
    MissingOutDir,
    #[error("failed to invoke {tool}: {source}")]
    DescriptorTool { tool: &'static str, #[source] source: std::io::Error },
    #[error("failed to decode FileDescriptorSet: {0}")]
    DecodeFileDescriptorSet(#[source] buffa::DecodeError),
    #[error("buffa-build error: {0}")]
    BuffaBuild(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
```

We **wrap** rather than alias `buffa_build::Config` because we need to inspect & augment its `type_attributes`, and exposing buffa-build's full surface verbatim would prevent us from intercepting the descriptor source (we own the FDS file in `OUT_DIR`).

### 3.2 `compile()` algorithm

```text
fn compile(self):
  1. out_dir = self.out_dir or env(OUT_DIR) or error(MissingOutDir)
  2. fds_path = self.file_descriptor_set_path or out_dir/"file_descriptor_set.bin"
  3. ensure parent(fds_path) exists, mkdir -p

  4. fds_bytes = match self.descriptor_source:
       Protoc       -> run protoc, capture binary into fds_path
       Buf          -> run `buf build --as-file-descriptor-set -o fds_path`
       Precompiled  -> copy(user_path, fds_path)

  5. fds = FileDescriptorSet::decode_from_slice(&fds_bytes)?
       (using buffa_descriptor::generated::descriptor::FileDescriptorSet)

  6. attr_lines = ["#[derive(::buffa_reflect::ReflectMessage)]"]
     match self mode:
       descriptor_pool      -> push #[buffa_reflect(descriptor_pool      = "<expr>")]
       bytes-include        -> push #[buffa_reflect(file_descriptor_set_bytes = "<expr>")]
     for every message walked recursively (incl. nested) through fds:
        full_name = "<package>.<...>.<MessageName>"
        for line in attr_lines: cfg.type_attribute(full_name, line)
        cfg.type_attribute(full_name,
          format!(r#"#[buffa_reflect(message_name = "{full_name}")]"#))

  7. cfg = buffa_build::Config::new()
                 .descriptor_set(fds_path)            # avoid double protoc
                 .files(self.files_proto_relative())  # see below
                 .includes(self.includes)             # ignored by Precompiled source
                 .out_dir(out_dir)
                 .apply_user_passthrough(self)        # type_attribute / generate_*  / ...

  8. cfg.compile()?         # invokes buffa_codegen::generate(...)

  9. # cargo dependency tracking
     for f in self.files: println!("cargo:rerun-if-changed={}", f.display())
     println!("cargo:rerun-if-env-changed=PROTOC")
     println!("cargo:rerun-if-changed={}", fds_path.display())
```

A few implementation notes:

- **`files_proto_relative`**: when descriptor source is `Protoc`, the user passes filesystem paths (e.g., `"proto/foo.proto"`); buffa-build's existing logic strips the longest matching include prefix. When the source is `Buf` or `Precompiled`, the names must already match `FileDescriptorProto.name`. We mirror buffa-build's behavior verbatim by reusing `proto_relative_name` (or a re-exported analogue) — see `buffa-build/src/lib.rs:572-586`.
- **Why we re-run protoc ourselves**: buffa-build's `Protoc` source invokes protoc internally and discards the bytes after decoding. We can't recover them from a `Config` we haven't constructed yet. Cleanest solution: own the protoc invocation, write the bytes to `fds_path`, then hand `fds_path` to buffa-build via `descriptor_set(fds_path)`. Same approach prost-reflect-build takes (it relies on `prost_build::Config::file_descriptor_set_path` + `skip_protoc_run`).
- **`use_buf` mode**: identical orchestration but call `buf build --as-file-descriptor-set -o fds_path` instead of protoc.

### 3.3 Generated decorations

For a message at proto FQN `acme.api.v1.User`, buffa-codegen now sees these prepended `type_attributes` via `buffa_build::Config::type_attribute(".acme.api.v1.User", ...)`:

```rust
#[derive(::buffa_reflect::ReflectMessage)]
#[buffa_reflect(message_name = "acme.api.v1.User")]
#[buffa_reflect(file_descriptor_set_bytes = "crate::FILE_DESCRIPTOR_SET_BYTES")]
#[derive(Clone, PartialEq, Default)]
pub struct User { /* ...buffa fields... */ }
```

Buffa-codegen already validates that user-supplied attributes parse as Rust attributes; nothing else changes about the generated impl_message / view / oneof code. Because `type_attribute` matching is segment-aware, we always pass the full FQN with leading `.`, ensuring no false matches across packages.

### 3.4 build.rs example

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .files(&["proto/acme/api/v1/user.proto"])
        .includes(&["proto/"])
        .compile()?;
    Ok(())
}
```

```rust
// src/lib.rs
buffa::include_proto!("acme.api.v1");

pub const FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));
```

That's the entire downstream user surface. No additional types to construct, no `Lazy` / `OnceLock` for the user to write; the bytes form delegates pool initialization to the derive's expansion.

For users who want a shared, library-owned pool:

```rust
// build.rs
buffa_reflect_build::Builder::new()
    .descriptor_pool("crate::DESCRIPTOR_POOL")
    .files(&["proto/acme/api/v1/user.proto"])
    .includes(&["proto/"])
    .compile()?;
```

```rust
// src/lib.rs
use std::sync::LazyLock;

buffa::include_proto!("acme.api.v1");

const FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

pub static DESCRIPTOR_POOL: LazyLock<buffa_reflect::DescriptorPool> = LazyLock::new(|| {
    buffa_reflect::DescriptorPool::decode(FILE_DESCRIPTOR_SET_BYTES)
        .expect("buffa-reflect: invalid FileDescriptorSet")
});
```

---

## 4. `buffa-reflect-derive` — the proc-macro crate

### 4.1 Surface

```rust
#[proc_macro_derive(ReflectMessage, attributes(buffa_reflect))]
pub fn derive_reflect_message(input: TokenStream) -> TokenStream;
```

Recognized attributes (parsed with `syn::meta::ParseNestedMeta`):

| key                         | value                                                                                                                      | required                                               | meaning                                      |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ | -------------------------------------------- |
| `descriptor_pool`           | quoted Rust expression that evaluates to `&'static buffa_reflect::DescriptorPool` (e.g., `LazyLock<DescriptorPool>` deref) | one of `descriptor_pool` / `file_descriptor_set_bytes` | resolution path A                            |
| `file_descriptor_set_bytes` | quoted Rust expression that evaluates to `&'static [u8]`                                                                   | one of two                                             | resolution path B (lazy global pool seeding) |
| `message_name`              | quoted FQN, e.g., `"acme.api.v1.User"`                                                                                     | always                                                 | argument to `get_message_by_name`            |

### 4.2 Expansions

Pool form (preferred):

```rust
impl ::buffa_reflect::ReflectMessage for #ident {
    fn descriptor(&self) -> ::buffa_reflect::MessageDescriptor {
        #pool_expr
            .get_message_by_name(#message_name)
            .expect(concat!("buffa-reflect: descriptor for `", #message_name, "` not found"))
    }
}
```

Bytes form (lazy global init):

```rust
impl ::buffa_reflect::ReflectMessage for #ident {
    fn descriptor(&self) -> ::buffa_reflect::MessageDescriptor {
        static INIT: ::std::sync::OnceLock<::buffa_reflect::DescriptorPool> =
            ::std::sync::OnceLock::new();
        let pool = INIT.get_or_init(|| {
            ::buffa_reflect::DescriptorPool::decode(#bytes_expr)
                .expect("buffa-reflect: invalid FileDescriptorSet")
        });
        pool.get_message_by_name(#message_name)
            .expect(concat!("buffa-reflect: descriptor for `", #message_name, "` not found"))
    }
}
```

We use `OnceLock<DescriptorPool>` rather than prost-reflect's `Once` + global mutable pool because:

- The derive can run for messages from multiple `.proto` files in the same crate; each derive will use its own static, so even if two derives reference different bytes (unlikely in this repo's usage but possible across re-exports) they are independent.
- It avoids touching mutable global state and the `Mutex` it would require.
- The same `decode` cost is paid at most once per `(generated module × bytes constant)` pair.

If we later want a true cross-crate global pool, we add `buffa_reflect::global()` as a separate API and a new attribute alias — non-breaking.

### 4.3 Diagnostics

Compile-time errors we raise from the macro (using `syn::Error::new_spanned`):

- `MissingDescriptorBinding` — neither `descriptor_pool` nor `file_descriptor_set_bytes` was given.
- `BothDescriptorBindings` — both given.
- `MissingMessageName` — no `message_name`.
- `BadAttributeShape` — value isn't a Rust expression / string literal as expected.

The macro never panics at proc-macro expansion time; all faults are reported as syn errors with helpful spans.

---

## 5. `buffa-reflect` — runtime crate

Phase 1 surface: enough to make a generated message tell you about itself.

### 5.1 Module layout

```
crates/buffa-reflect/src/
  lib.rs           # re-exports
  pool.rs          # DescriptorPool + DescriptorPoolInner
  file.rs          # FileDescriptor + inner
  message.rs       # MessageDescriptor + inner
  field.rs         # FieldDescriptor + Kind + Cardinality + inner
  enumeration.rs   # EnumDescriptor + EnumValueDescriptor + inner
  oneof.rs         # OneofDescriptor + inner
  reflect.rs       # ReflectMessage trait
  error.rs         # DescriptorError
```

Phase 2 will add `dynamic.rs`, `service.rs`, `extension.rs`. We leave the module names reserved.

### 5.2 Public types

```rust
// lib.rs re-exports
pub use crate::pool::DescriptorPool;
pub use crate::file::FileDescriptor;
pub use crate::message::MessageDescriptor;
pub use crate::field::{FieldDescriptor, Kind, Cardinality};
pub use crate::enumeration::{EnumDescriptor, EnumValueDescriptor};
pub use crate::oneof::OneofDescriptor;
pub use crate::reflect::ReflectMessage;
pub use crate::error::DescriptorError;

#[cfg(feature = "derive")]
pub use buffa_reflect_derive::ReflectMessage;
```

`DescriptorPool` follows the prost-reflect arch verbatim:

```rust
#[derive(Clone, Default)]
pub struct DescriptorPool { inner: ::std::sync::Arc<DescriptorPoolInner> }

impl DescriptorPool {
    pub fn new() -> Self;
    pub fn decode(bytes: &[u8]) -> Result<Self, DescriptorError>;
    pub fn from_file_descriptor_set(
        fds: buffa_descriptor::generated::descriptor::FileDescriptorSet,
    ) -> Result<Self, DescriptorError>;
    pub fn add_file_descriptor_set(
        &mut self,
        fds: buffa_descriptor::generated::descriptor::FileDescriptorSet,
    ) -> Result<(), DescriptorError>;

    pub fn files(&self) -> impl Iterator<Item = FileDescriptor> + '_;
    pub fn all_messages(&self) -> impl Iterator<Item = MessageDescriptor> + '_;
    pub fn all_enums(&self) -> impl Iterator<Item = EnumDescriptor> + '_;

    pub fn get_message_by_name(&self, full_name: &str) -> Option<MessageDescriptor>;
    pub fn get_enum_by_name(&self, full_name: &str) -> Option<EnumDescriptor>;
    pub fn get_file_by_name(&self, name: &str) -> Option<FileDescriptor>;
}
```

```rust
struct DescriptorPoolInner {
    names:      hashbrown::HashMap<Box<str>, Definition>,  // FQN -> kind+index
    file_names: hashbrown::HashMap<Box<str>, FileIndex>,
    files:      Vec<FileDescriptorInner>,
    messages:   Vec<MessageDescriptorInner>,
    enums:      Vec<EnumDescriptorInner>,
}

enum Definition {
    Message(MessageIndex),
    Enum(EnumIndex),
    // future: Service, Method, Extension
}
```

`MessageDescriptor` is `(DescriptorPool, MessageIndex)`. `FieldDescriptor` is `(MessageDescriptor, FieldIndex)`. We do **not** publicly expose the inner records; descriptor handles are 16-byte cheap-clone values.

```rust
impl MessageDescriptor {
    pub fn full_name(&self) -> &str;
    pub fn name(&self) -> &str;
    pub fn parent_file(&self) -> FileDescriptor;
    pub fn parent_message(&self) -> Option<MessageDescriptor>;
    pub fn fields(&self) -> impl ExactSizeIterator<Item = FieldDescriptor> + '_;
    pub fn oneofs(&self) -> impl ExactSizeIterator<Item = OneofDescriptor> + '_;
    pub fn get_field_by_name(&self, name: &str) -> Option<FieldDescriptor>;
    pub fn get_field_by_json_name(&self, name: &str) -> Option<FieldDescriptor>;
    pub fn get_field_by_number(&self, number: u32) -> Option<FieldDescriptor>;
    /// Raw access for advanced users; useful when you want
    /// proto2 default values, source-code info, etc.
    pub fn descriptor_proto(&self) -> &buffa_descriptor::generated::descriptor::DescriptorProto;
}

impl FieldDescriptor {
    pub fn name(&self) -> &str;
    pub fn full_name(&self) -> &str;             // "<msg.full_name>.<name>"
    pub fn json_name(&self) -> &str;
    pub fn number(&self) -> u32;
    pub fn kind(&self) -> Kind;                   // scalar / message / enum / map
    pub fn cardinality(&self) -> Cardinality;     // Optional / Required / Repeated
    pub fn supports_presence(&self) -> bool;
    pub fn is_packed(&self) -> bool;
    pub fn containing_oneof(&self) -> Option<OneofDescriptor>;
    pub fn parent_message(&self) -> MessageDescriptor;
    pub fn descriptor_proto(&self) -> &buffa_descriptor::generated::descriptor::FieldDescriptorProto;
}

#[non_exhaustive]
pub enum Kind {
    Double, Float,
    Int32, Int64, Uint32, Uint64, Sint32, Sint64,
    Fixed32, Fixed64, Sfixed32, Sfixed64,
    Bool, String, Bytes,
    Message(MessageDescriptor),
    Enum(EnumDescriptor),
}

#[non_exhaustive]
pub enum Cardinality { Optional, Required, Repeated }
```

`Kind::Message`/`Kind::Enum` carry a *resolved* descriptor handle, not just a string. Resolution happens at pool-build time so `kind()` is O(1) at the use site.

### 5.3 The `ReflectMessage` trait

```rust
// reflect.rs
pub trait ReflectMessage: ::buffa::Message {
    /// The descriptor for this message's type.
    fn descriptor(&self) -> MessageDescriptor;
}
```

That's the entire trait in Phase 1. `transcode_to_dynamic` and friends arrive in Phase 2 with `DynamicMessage`.

### 5.4 Errors

`DescriptorError` is `#[non_exhaustive]`. Phase 1 ships these variants
(see `crates/buffa-reflect/src/error.rs` for the canonical declaration —
reproduced here as a sketch, not a syntactic spec):

- `Decode(#[from] buffa::DecodeError)` — wire-level decode failure.
- `MissingFileName` — a `FileDescriptorProto.name` was unset.
- `MissingName { location: String }` — a nested message / enum / field /
  oneof had no `name`.
- `UnresolvedType { field: String, type_name: String }` — a field's
  `type_name` does not resolve in the pool. (Struct variant rather than
  the tuple shape an earlier draft suggested; the extra context is needed
  for actionable error messages.)
- `DuplicateType(String)` — two types share the same FQN.
- `DuplicateFile(String)` — two `FileDescriptorProto`s share the same
  `name`.
- `InvalidFieldNumber { message, number, max }` — out-of-range or
  reserved-range field number.
- `MissingFieldType` / `MissingTypeName { kind }` — `type_name` and
  `type` are inconsistently set on a field.
- `InvalidOneofIndex { field, index, count }` — a `oneof_index` exceeds
  the message's `oneof_decl` count.
- `Proto3EnumMissingZero(String)` — proto3 enum has no `0` variant.
- `Proto3RequiredField { field: String }` — proto3 disallows `required`.
- `Validation(String)` — generic catch-all for invariants that don't
  warrant their own variant yet.

### 5.5 Validation done at pool build time

We mirror prost-reflect's contract:

- name resolution (`type_name` lookups produce a resolved index, with cross-file imports honored);
- field number range `1..=536_870_911`, reserved `19_000..=19_999` rejected (matches `protoc`'s `kFirstReservedNumber`/`kLastReservedNumber`);
- duplicate FQN detection (per file *and* across files);
- enum default-value resolves to a declared variant;
- proto3-syntactic rules (no required fields; enum-zero present) — only when `syntax == "proto3"`. In editions, defer to descriptor-supplied features without re-checking.

We do **not** validate option types in Phase 1; that's deferred with `DynamicMessage`.

### 5.6 Concurrency / clone semantics

`DescriptorPool` is `Clone + Send + Sync` and clones are O(1) (Arc-shared inner). Descriptor handles (`MessageDescriptor`, etc.) are `Clone + Send + Sync` carrying an owned `DescriptorPool`. Comparison is index-based: handles from different pools are never equal even if they refer to the "same" type.

### 5.7 MSRV and feature flags

- **MSRV**: Rust 2024, latest stable (this workspace pins `edition = "2024"`).
- **Features**:
  - `derive` (default on) — re-exports `buffa_reflect_derive::ReflectMessage`.
  - `std` (default on) — placeholder for future `no_std` work.
- Dependencies (workspace-pinned):
  - `buffa = "0.x"` (path = "../../vendors/buffa/buffa")
  - `buffa-descriptor = "0.x"` (path = "../../vendors/buffa/buffa-descriptor")
  - `thiserror = "2"`
  - `hashbrown = "0.15"` (deterministic faster maps; matches buffa's own deps)
  - `buffa-reflect-derive = { path = "../buffa-reflect-derive", optional = true }` behind `derive`

---

## 6. End-to-end happy path

1. Consumer's `Cargo.toml`:
   ```toml
   [dependencies]
   buffa = "0.x"
   buffa-reflect = "0.x"

   [build-dependencies]
   buffa-reflect-build = "0.x"
   ```
2. `build.rs` wires Builder → compile (5 lines).
3. `src/lib.rs` includes the generated code via `buffa::include_proto!(...)` and exposes `FILE_DESCRIPTOR_SET_BYTES = include_bytes!(...)`.
4. Application code:
   ```rust
   use buffa_reflect::ReflectMessage;
   let user = acme::api::v1::User::default();
   let d = user.descriptor();
   for field in d.fields() {
       println!("{} = #{} ({:?})", field.name(), field.number(), field.kind());
   }
   ```

No additional macros, no special traits, no manual pool wiring outside the `LazyLock` (and only when the user opts into the pool form).

---

## 7. Risks & mitigations

| Risk                                                                                                  | Mitigation                                                                                                                                                                                                                |
| ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Buffa-build behavior changes (e.g., `descriptor_set` precompiled mode regressing) break us silently.  | Pin buffa via path-dependency until a stable release range exists; integration tests in `crates/buffa-reflect-build/tests` exercise all three descriptor-source modes end-to-end.                                         |
| Generated `#[buffa_reflect(...)]` attribute namespace collides with a future buffa codegen attribute. | The `buffa_reflect` namespace is unique by design; keep it documented as our reservation.                                                                                                                                 |
| Macro-emitted `OnceLock` interacts badly with users who initialize their pool manually.               | Document both forms as mutually exclusive per derive site; the user picks one in Builder configuration and the derive emits exactly that path.                                                                            |
| Buffa's `MessageView<'a>` types — should they implement `ReflectMessage`?                             | Yes, eventually; Phase 1 emits the derive only on owned types (which is what `type_attribute` decorates) and we leave views to Phase 2. The trait bound is `: buffa::Message` so adding view impls later is non-breaking. |
| Large descriptor sets: `decode + index` cost on first `descriptor()` call.                            | Same cost as prost-reflect; for very large schemas the pool form lets the user share a single decoded pool. We'll document the tradeoff.                                                                                  |

---

## 8. Out of scope here, captured for later

- `DynamicMessage`, `Value`, `MapKey`, `set_field_by_name`, `get_field_by_name`. These need a faithful re-implementation of buffa's `compute_size` / `write_to` / `merge_field` driven by descriptors and `Value`s. Ships in its own spec.
- `serde::{Serialize, Deserialize}` for `DynamicMessage` (canonical proto3 JSON mapping). Phase 2.
- Textproto support (`buffa::text::TextFormat` integration). Phase 2.
- gRPC server-reflection (`grpc.reflection.v1`) shim — straightforward once Phase 1 lands, but lives outside this workspace as a small adapter crate.
- Upstream patch to buffa-build adding a `pre_codegen_hook` that lets us inject `type_attribute` entries from a descriptor walk *without* duplicate orchestration. Not blocking but nice for Phase 2.
