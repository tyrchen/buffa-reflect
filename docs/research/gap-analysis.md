# Gap Analysis — porting prost-reflect's pattern to buffa

This document maps prost-reflect concepts to the buffa equivalents that must exist for a `buffa-reflect` crate to be useful, and resolves the integration questions before the design spec.

---

## 1. Concept-to-concept mapping

| prost-reflect | buffa-reflect (proposed) | Status today |
| --- | --- | --- |
| `prost::Message` runtime trait | `buffa::Message` / `buffa::MessageView<'a>` | ✅ exists |
| `prost-types::FileDescriptorSet` | `buffa_descriptor::generated::descriptor::FileDescriptorSet` | ✅ exists, fully buffa-native |
| `DescriptorPool` | `buffa_reflect::DescriptorPool` | ❌ build me |
| `MessageDescriptor` / `FieldDescriptor` / `EnumDescriptor` / `OneofDescriptor` / `ServiceDescriptor` / `MethodDescriptor` / `ExtensionDescriptor` | same names under `buffa_reflect` | ❌ build me |
| `ReflectMessage::descriptor() -> MessageDescriptor` | `buffa_reflect::ReflectMessage::descriptor()` | ❌ build me |
| `#[derive(ReflectMessage)]` + `#[prost_reflect(...)]` | `#[derive(ReflectMessage)]` + `#[buffa_reflect(...)]` | ❌ build me (separate proc-macro crate) |
| `prost-reflect-build::Builder` | `buffa-reflect-build::Builder` | ❌ build me — primary deliverable |
| `OUT_DIR/file_descriptor_set.bin` | same path, same wire format (`google.protobuf.FileDescriptorSet` proto bytes) | ❌ emit me |
| `DynamicMessage` (encode/decode/get/set/JSON/text) | `buffa_reflect::DynamicMessage` | ❌ deferred to phase 2 |
| Global pool (`DescriptorPool::global()`) | optional `buffa_reflect::global()` helper | ❌ phase 2 / nice to have |

The good news: buffa already has the descriptor model, so the runtime crate's "structural" cost is mostly indexing + lookup, not reinventing protobuf descriptor types.

---

## 2. Integration strategy for the build-script crate

prost-reflect-build relies on prost-build's `skip_protoc_run()` to compile twice cheaply. buffa-build does not have that hook; its `Config` is consume-by-move terminating in `compile(self)`. But buffa-build *does* expose `Config::descriptor_set(path)` (`buffa-build/src/lib.rs:468`), which short-circuits the protoc invocation and reads the FDS from disk. That gives us a clean orchestration:

```text
buffa-reflect-build flow
========================
1. Resolve OUT_DIR (or override via Builder::out_dir).
2. Pick a descriptor source (Protoc | Buf | Precompiled), default Protoc.
3. Invoke protoc/buf ourselves to write OUT_DIR/file_descriptor_set.bin.
   - Mirrors prost-reflect-build's "capture once" step.
   - For Precompiled, we just read the user's file and copy it to OUT_DIR.
4. Decode the FDS using buffa_descriptor types.
5. For every message in the FDS, append three buffa_build::Config::type_attribute entries:
       #[derive(::buffa_reflect::ReflectMessage)]
       #[buffa_reflect(message_name = "package.MyMessage")]
       #[buffa_reflect(file_descriptor_set_bytes = "crate::FILE_DESCRIPTOR_SET_BYTES")]
       (or descriptor_pool = "crate::DESCRIPTOR_POOL", per Builder mode)
6. Drive buffa_build::Config with .descriptor_set(path) and .type_attribute(...) entries,
   then call .compile() — this re-uses the FDS we already wrote and runs codegen once.
7. Done. The artifact (.bin) and the generated *.rs files are now both in OUT_DIR.
```

This avoids any upstream changes to buffa-build. If we later decide we want a `Builder::pre_generate_hook` in buffa-build to skip the protoc round-trip (single-pass), that's a small follow-up PR — but it is **not** on the critical path.

---

## 3. The runtime contract

To keep the surface minimal we want **exactly one trait** generated code touches:

```rust
pub trait ReflectMessage: buffa::Message {
    fn descriptor(&self) -> MessageDescriptor;

    #[cfg(feature = "dynamic")]
    fn transcode_to_dynamic(&self) -> DynamicMessage where Self: Sized { /* ... */ }
}
```

Everything else — `MessageDescriptor::fields()`, `FieldDescriptor::kind()`, etc. — is library code over `buffa-descriptor`'s pre-built types. The trait stays small so the derive output stays small.

---

## 4. Pool resolution model

prost-reflect supports two derive flavors. We mirror that:

**A. User-owned pool (`descriptor_pool = "crate::POOL"`).** Recommended for libraries that want to share a `DescriptorPool` across multiple files / dependencies.

```rust
static FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

pub static DESCRIPTOR_POOL: LazyLock<buffa_reflect::DescriptorPool> = LazyLock::new(|| {
    buffa_reflect::DescriptorPool::decode(FILE_DESCRIPTOR_SET_BYTES)
        .expect("invalid FileDescriptorSet")
});
```

Derive expansion:
```rust
impl buffa_reflect::ReflectMessage for MyMessage {
    fn descriptor(&self) -> buffa_reflect::MessageDescriptor {
        crate::DESCRIPTOR_POOL
            .get_message_by_name("package.MyMessage")
            .expect("descriptor for `package.MyMessage` not found")
    }
}
```

**B. Bytes form (`file_descriptor_set_bytes = "crate::BYTES"`).** Convenience for single-crate use; the derive lazily seeds a global pool.

Either form ends up calling `DescriptorPool::get_message_by_name(...)` — there is exactly one resolution path.

---

## 5. Crate boundary decisions

We split into three crates, matching prost-reflect's split:

- `buffa-reflect` — runtime. `DescriptorPool`, descriptor handles, `ReflectMessage` trait, optional `DynamicMessage`. No proc-macros (so it can be `no_std`-friendly later).
- `buffa-reflect-build` — build-script library. `Builder` API, runs protoc/buf (or accepts a precompiled FDS), drives `buffa-build::Config` with the right `type_attribute` entries, copies the FDS to `OUT_DIR/file_descriptor_set.bin`. **This is the deliverable the user asked for.**
- `buffa-reflect-derive` — `#[derive(ReflectMessage)]` + the `#[buffa_reflect(...)]` attribute parser.

This three-crate split:
- mirrors prost-reflect, so anyone arriving from there has zero learning curve;
- keeps the `proc-macro = true` blast radius isolated;
- allows `buffa-reflect` to compile independent of `buffa-build` (e.g., when consuming generated code that was checked in via the BSR plugin packager).

---

## 6. Phasing

Phase 1 (this spec — what the user asked for):

1. `buffa-reflect-build` Builder + Protoc/Buf/Precompiled descriptor source selectors.
2. Emit `OUT_DIR/file_descriptor_set.bin`.
3. Inject `#[derive(ReflectMessage)]` + `#[buffa_reflect(...)]` attributes on every message.
4. Minimum viable `buffa-reflect`: `DescriptorPool::decode`, `get_message_by_name`, `MessageDescriptor::full_name/name/fields()`, `FieldDescriptor::name/number/json_name/kind/cardinality`, `EnumDescriptor::values()`. Enough for "given `m: impl ReflectMessage`, walk its schema".
5. `buffa-reflect-derive` with both `descriptor_pool` and `file_descriptor_set_bytes` modes.
6. Tests: a small `.proto` fixture, a doctest showing the build-script wiring, integration tests asserting that `m.descriptor().full_name() == "..."` and that `descriptor.fields()` matches expectation.

Phase 2 (later, separate spec):

7. `DynamicMessage` (Value / MapKey, encode-via-descriptor, decode-via-descriptor).
8. Optional `serde::{Serialize, Deserialize}` for `DynamicMessage` (canonical proto3 JSON).
9. Optional `text-format` parser/printer.
10. Optional global pool helper.
11. Conformance suite via `DynamicMessage`.

Phase 1 is the smallest standalone deliverable. With it, downstream code can do `m.descriptor().fields().for_each(|f| ...)` — useful for logging, observability, schema-aware diff, generic CLI dump tools — even before `DynamicMessage` lands.

---

## 7. Risks & open questions resolved by the spec

| Question | Resolution |
| --- | --- |
| Should `buffa-reflect`'s `MessageDescriptor` reuse `buffa_descriptor::DescriptorProto` or wrap it? | Wrap. Same approach as prost-reflect — the public handle is `(pool, index)` and we own the indexed inner records; the raw `DescriptorProto` is preserved on `FileDescriptor::raw()` for advanced users. |
| Do we patch buffa-build to skip the duplicate protoc invocation? | No, not on the critical path. `buffa-build::Config::descriptor_set(path)` already gives us a single-codegen path; we run protoc/buf once ourselves. Optional follow-up upstream PR. |
| Where does the `.bin` file live? | `$OUT_DIR/file_descriptor_set.bin`, identical to prost-reflect-build, so muscle memory transfers. Configurable via `Builder::file_descriptor_set_path`. |
| What is the wire format of the artifact? | Raw `google.protobuf.FileDescriptorSet` proto bytes — same as `protoc --descriptor_set_out`. Decoded at runtime by `buffa_descriptor::generated::descriptor::FileDescriptorSet::decode_from_slice`. |
| `no_std` story for `buffa-reflect`? | `DescriptorPool` requires `alloc`; we feature-gate `std`-only convenience (env helpers, the optional global pool). The descriptor model itself is `alloc`-friendly. Phase-1 nice-to-have, not blocking. |
| Stability of generated attributes (`#[buffa_reflect(...)]`)? | Treated as semver-public API of `buffa-reflect-derive`. Keep the attribute names stable across the crate's 0.x line. |
