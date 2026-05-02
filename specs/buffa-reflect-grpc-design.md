# Phase 2d — gRPC server reflection (`grpc.reflection.v1`)

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). **Depends only on Phase 1**, so this can ship at any time without waiting on `DynamicMessage`.

This is the smallest of the Phase 2 deliverables and the highest-leverage in production: any gRPC service that exposes `grpc.reflection.v1` becomes discoverable to `grpcurl`, Postman, the `buf curl` CLI, and IDE plugins.

---

## 1. Goals

- A service implementation of `grpc.reflection.v1.ServerReflection` backed by a `buffa_reflect::DescriptorPool`.
- Mirror [`tonic-reflection`](https://docs.rs/tonic-reflection)'s API shape — `Builder::configure().with_file_descriptor_set(bytes).build()` — so anyone migrating from tonic+prost finds the same affordances.
- Ship as a **separate crate** (not a default member of this workspace), depending on `buffa-reflect` + `tonic`. Same packaging pattern as `tonic-reflection`.
- Support both the `v1` and `v1alpha` services (the field most servers still expose during the spec migration).

## 2. Non-goals

- A standalone reflection daemon. We provide the service impl; users wire it into their existing tonic server.
- A non-tonic transport adapter. tonic is the de-facto Rust gRPC; if other transports want this, they can fork the trivial dispatch logic.
- Protocol-level changes vs. the canonical reflection proto.

---

## 3. Crate placement

New crate **outside this workspace**: `buffa-grpc-reflection` lives in `examples/grpc-reflection/` (leaf workspace) initially, and graduates to its own published crate when stable.

Why outside the workspace: the dep on `tonic` ≈ 6 transitive crates and a build-time prost dep (tonic generates from prost-built protos). Keeping it isolated means the main `buffa-reflect` workspace's Cargo.lock stays small.

Cargo.toml:

```toml
[package]
name = "buffa-grpc-reflection"
description = "grpc.reflection.v1 service backed by a buffa-reflect DescriptorPool."
publish = false   # until stable
version = "0.1.0"
edition = "2024"

# Leaf workspace.
[workspace]

[dependencies]
buffa-reflect = { path = "../../crates/buffa-reflect" }
tonic = "0.13"
tokio-stream = "0.1"
prost = "0.14"  # Required by tonic's generated reflection types.

[build-dependencies]
tonic-build = "0.13"
```

Build script generates the `grpc.reflection.v1.{ServerReflection, ServerReflectionRequest, ...}` types from the official proto file (vendored under `proto/`).

---

## 4. Public surface

```rust
// crates/buffa-grpc-reflection/src/lib.rs (sketch)
pub use crate::builder::Builder;
pub use crate::service::ReflectionServiceImpl;

pub mod proto {
    // Generated from grpc.reflection.v1.proto and v1alpha.
    pub mod v1 { tonic::include_proto!("grpc.reflection.v1"); }
    pub mod v1alpha { tonic::include_proto!("grpc.reflection.v1alpha"); }
}
```

```rust
pub struct Builder {
    pool: ::buffa_reflect::DescriptorPool,
    /// Service names to advertise via `ListServices`. Defaults to every
    /// service in `pool`.
    advertised_services: Option<Vec<String>>,
}

impl Builder {
    pub fn from_pool(pool: ::buffa_reflect::DescriptorPool) -> Self;

    /// Convenience constructor: decode a `FileDescriptorSet` and build a
    /// pool from it. Equivalent to
    /// `from_pool(DescriptorPool::decode(bytes)?)`.
    pub fn from_file_descriptor_set_bytes(bytes: &[u8])
        -> Result<Self, ::buffa_reflect::DescriptorError>;

    /// Limit advertised services. Default: every `service` in the pool.
    pub fn advertise_services(mut self, names: impl IntoIterator<Item = String>) -> Self;

    /// Build both v1 and v1alpha tonic services.
    pub fn build(self)
        -> Result<(crate::proto::v1::server_reflection_server::ServerReflectionServer<...>,
                   crate::proto::v1alpha::server_reflection_server::ServerReflectionServer<...>),
                  BuilderError>;

    /// Build only the v1 service (most modern clients).
    pub fn build_v1(self) -> Result<crate::proto::v1::ServerReflectionServer<...>, BuilderError>;
}
```

Usage from a tonic server:

```rust
let pool = buffa_reflect::DescriptorPool::decode(MY_FDS_BYTES)?;
let (refl_v1, refl_v1alpha) = buffa_grpc_reflection::Builder::from_pool(pool).build()?;

tonic::transport::Server::builder()
    .add_service(refl_v1)
    .add_service(refl_v1alpha)
    .add_service(my_grpc_service)
    .serve(addr)
    .await?;
```

## 5. Service request handling

`grpc.reflection.v1.ServerReflectionRequest` is a oneof with these arms; each maps to a small handler:

| Request | Handler |
| --- | --- |
| `file_by_filename(string)` | `pool.get_file_by_name(...)` → encode the file's raw `FileDescriptorProto` into `FileDescriptorResponse.file_descriptor_proto`. |
| `file_containing_symbol(string)` | Resolve symbol via `pool.get_message_by_name(...)`/`get_enum_by_name(...)`/etc.; return its parent file's encoded `FileDescriptorProto`. |
| `file_containing_extension({type, number})` | Walk extensions in pool; return owning file. (Extensions are read-only via `descriptor_proto()` in Phase 1; sufficient for v1 spec.) |
| `all_extension_numbers_of_type(string)` | Walk extensions; return all extension numbers extending the named message. |
| `list_services` | Iterate every `ServiceDescriptor` in the pool; return `ServiceResponse { name }` for each. |

Streaming request/response: each request gets exactly one response, but the channel stays open so a single connection can issue many requests. Tonic's `streaming` API handles this naturally.

`v1alpha` is bit-for-bit the same proto with a different package; we share the handler logic and emit two service registrations.

---

## 6. Phase 1 dependency: services in the descriptor pool

Phase 1 ships `MessageDescriptor`, `FieldDescriptor`, `EnumDescriptor`, `OneofDescriptor`, `FileDescriptor`. It does **not** ship `ServiceDescriptor` or `MethodDescriptor`. Phase 2d adds:

```rust
// crates/buffa-reflect/src/service.rs (Phase 2d-driven)
pub struct ServiceDescriptor { /* (DescriptorPool, ServiceIndex) */ }
pub struct MethodDescriptor { /* (ServiceDescriptor, MethodIndex) */ }

impl DescriptorPool {
    pub fn services(&self) -> impl Iterator<Item = ServiceDescriptor> + '_;
    pub fn get_service_by_name(&self, full_name: &str) -> Option<ServiceDescriptor>;
}

impl ServiceDescriptor {
    pub fn name(&self) -> &str;
    pub fn full_name(&self) -> &str;
    pub fn parent_file(&self) -> FileDescriptor;
    pub fn methods(&self) -> impl Iterator<Item = MethodDescriptor> + '_;
    pub fn descriptor_proto(&self) -> &::buffa_descriptor::generated::descriptor::ServiceDescriptorProto;
}

impl MethodDescriptor {
    pub fn name(&self) -> &str;
    pub fn full_name(&self) -> &str;
    pub fn input(&self) -> MessageDescriptor;
    pub fn output(&self) -> MessageDescriptor;
    pub fn is_client_streaming(&self) -> bool;
    pub fn is_server_streaming(&self) -> bool;
}
```

These slot into `PoolInner` alongside existing tables (`services: Vec<ServiceEntry>`, `service_names: HashMap<Box<str>, ServiceIndex>`). The pool builder (Phase 1's `pool_build.rs`) gains a third pass that walks `FileDescriptorProto::service` and resolves `input_type` / `output_type` to existing message indices.

This is a Phase 1 amend: ~150 LOC, isolated, doesn't change any existing API. Sneak it into the Phase 2d delivery.

---

## 7. Implementation effort

- **2 days**: build the gRPC service handlers + builder + tonic codegen wiring.
- **1 day**: extend `buffa-reflect` with `ServiceDescriptor` / `MethodDescriptor` (the small Phase 1 amend).
- **1 day**: integration test against `grpcurl` running in CI (Docker), exercising every request type.

Total: ~4 days.

---

## 8. Testing

- Unit tests: each request handler with a small in-memory pool.
- Integration test: spin up a tonic server with the reflection service, run `grpcurl -plaintext localhost:PORT list` and assert the output matches the pool's service list. Run under `cargo test -- --ignored` in CI.
- Fuzz-light: malformed reflection requests do not panic the server.

---

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| `v1alpha` vs `v1` divergence (the alpha proto has extra response fields some clients depend on). | Implement both side-by-side; share handler logic. |
| `file_containing_extension` for proto2 requires walking every file's `extension` field. | Pre-index at pool-build time: `extensions_by_extendee: HashMap<MessageIndex, Vec<(FileIndex, FieldIndex)>>`. O(1) at request time. |
| `tonic` version churn between major releases. | Pin `tonic = "0.13"` (the version when the spec lands); bump on coordinated tonic release. |
| Generated tonic code emits proto types via `prost`; consumers using `buffa` typed code may now have *both* prost and buffa in their dep tree. | Acceptable for this niche service; the reflection messages themselves are tiny. Document the dep cost. |

---

## 10. Acceptance for Phase 2d

- A consumer can copy the snippet from §4 and have a discoverable gRPC server with no extra setup.
- `grpcurl` integration test passes.
- `ServiceDescriptor` / `MethodDescriptor` are documented and used in at least one example.
- The `buffa-grpc-reflection` crate publishes cleanly (cargo publish --dry-run succeeds).
