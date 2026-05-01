# buffa-reflect — PRD

## Problem

[buffa](https://github.com/anthropics/buffa) is a pure-Rust protobuf implementation. It generates fast, idiomatic, editions-aware Rust code from `.proto` files via `protoc`/`buf`. What it does **not** offer today is **runtime reflection** — the ability to walk a generated message's schema, look up fields by name or number, build a message from descriptors alone, or generically transcode between formats.

The buffa README acknowledges this gap explicitly:

> Runtime reflection (`DynamicMessage`, descriptor-driven introspection) — planned for a future release. The descriptor types are now available in `buffa-descriptor` as a first step.

The closest reference in the prost ecosystem is [`prost-reflect`](https://github.com/andrewhickman/prost-reflect), which provides exactly this layer: a `DescriptorPool`, descriptor handles, a one-method `ReflectMessage` trait, and a `prost-reflect-build` build-script helper that wires it all into a downstream crate's `build.rs`.

The first concrete milestone toward closing this gap on buffa is **a build-script crate that produces serialized descriptors alongside generated code, and decorates the generated code so a future runtime layer can resolve them**. Without that artifact, no runtime reflection layer is reachable from a downstream consumer.

## Users

- **Service authors** wiring buffa into a production gRPC/Connect service who need observability features that depend on schema introspection (proto-aware logging, schema diff in CI, generic field-level tracing).
- **Tooling authors** who want to write a CLI that consumes any buffa-compiled crate and dumps messages generically (e.g., a `bufdump` analogue).
- **Internal Anthropic teams** using buffa today and asking for prost-reflect parity for migration ergonomics.
- **Library authors** publishing buffa-generated SDKs who want their consumers to opt into reflection without forking the SDK.

## Goals (Phase 1)

1. Ship `buffa-reflect-build`: a build-script library that, given a set of `.proto` files (or a precompiled `FileDescriptorSet`), drives `buffa-build` and additionally:
   - emits `OUT_DIR/file_descriptor_set.bin` (raw `google.protobuf.FileDescriptorSet` bytes — wire-compatible with `protoc --descriptor_set_out`);
   - injects `#[derive(buffa_reflect::ReflectMessage)]` and `#[buffa_reflect(...)]` attributes on every generated message;
   - is invocable from `build.rs` in 5–10 lines and works with both `protoc`, `buf`, and a precompiled descriptor set.
2. Ship a runtime crate `buffa-reflect` whose **minimum viable** surface is enough to make the artifact useful: `DescriptorPool`, `MessageDescriptor` (with `fields()`, `full_name()`, `name()`, `parent_file()`, `get_field_by_name`, `get_field_by_number`), `FieldDescriptor` (with `name`, `full_name`, `number`, `json_name`, `kind`, `cardinality`, `containing_oneof`), `EnumDescriptor`, `OneofDescriptor`, and the `ReflectMessage` trait.
3. Ship `buffa-reflect-derive` providing `#[derive(ReflectMessage)]` with two configuration shapes mirroring prost-reflect: `descriptor_pool = "..."` and `file_descriptor_set_bytes = "..."`.
4. Documented end-to-end example in `apps/server` (or a new example crate) that wires all three crates together.

## Non-goals (Phase 1)

- `DynamicMessage` (encode/decode-by-descriptor, get/set by name, JSON, textproto). This is real work and gets its own spec under Phase 2.
- Service/method reflection beyond what's free from the descriptor pool — explicitly: no gRPC server-reflection (`grpc.reflection.v1`) shim in this phase.
- Upstream changes to buffa-build. We orchestrate from the outside; if a small upstream hook would simplify Phase 2 we open a separate PR there.
- A `no_std` story for `buffa-reflect`. The descriptor model is `alloc`-friendly but Phase 1 ships `std`-only.
- Hot-path optimization. Correctness and ergonomics first; benchmarks come with Phase 2.

## Success criteria

- A consumer crate's `build.rs` can produce reflective generated code with a single chained call:
  ```rust
  buffa_reflect_build::Builder::new()
      .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
      .files(&["proto/foo.proto"])
      .includes(&["proto/"])
      .compile()?;
  ```
- For any `m: impl ReflectMessage`, `m.descriptor().full_name() == "<package>.<MessageName>"`, and `m.descriptor().fields().count() == <expected>`.
- The resulting `file_descriptor_set.bin` decodes round-trip-equivalent through both buffa's own `FileDescriptorSet::decode_from_slice` and the upstream `protoc --decode google.protobuf.FileDescriptorSet`.
- All three crates pass `cargo build`, `cargo test`, `cargo +nightly fmt --check`, and `cargo clippy -- -D warnings`.
- Documentation: every public item carries a `///` doc comment; the build-script crate's lib.rs has a working `# Example` block.

## Out-of-scope risks the spec must surface

- Any change in buffa's generated-code shape (e.g., the field on a message that holds unknown fields — `__buffa_unknown_fields`) is observable to downstream macros. Pinning the buffa version range in `buffa-reflect-derive`'s `Cargo.toml` mitigates the worst case.
- The `type_attribute` injection mechanism is the only seam we have without upstream changes. If buffa ever wants to forbid duplicate derives or the `#[buffa_reflect(...)]` attribute namespace, we'd have to coordinate. This is unlikely (the same pattern is supported in prost), but is the design's main coupling point with upstream.
