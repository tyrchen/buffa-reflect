# buffa-reflect Phase 2a — `DynamicMessage` Design

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). This document covers **only** Phase 2a — the `DynamicMessage` type, its `Value` model, the wire encode/decode dispatch, and the new methods on `ReflectMessage`. JSON, textproto, gRPC reflection, and view reflection have their own specs and depend on this one.

Pre-reads:
- [buffa-reflect Phase 1 design](./buffa-reflect-design.md) — descriptor pool, handles, validation
- [docs/research/buffa-architecture.md](../docs/research/buffa-architecture.md) — buffa's wire-encoding surface
- prost-reflect's `DynamicMessage` source for shape inspiration (see `vendors/prost-reflect/`)

---

## 1. Surface area at a glance

```rust
// crates/buffa-reflect/src/lib.rs
#[cfg(feature = "dynamic")]
pub use crate::dynamic::{DynamicError, DynamicMessage, MapKey, SetFieldError, Value};
```

```rust
// crates/buffa-reflect/src/reflect.rs (Phase 2 extension)
pub trait ReflectMessage: ::buffa::Message {
    fn descriptor(&self) -> MessageDescriptor;

    /// Phase 2 — encode `self` and decode it as a `DynamicMessage` against
    /// `self.descriptor()`. Default implementation is a single round-trip
    /// through `buffa::Message::encode_to_vec`. Hand-written specializations
    /// can short-circuit when allocator-free traversal is needed; we don't
    /// ship one, but the trait shape doesn't preclude one.
    #[cfg(feature = "dynamic")]
    fn transcode_to_dynamic(&self) -> DynamicMessage {
        let descriptor = self.descriptor();
        let bytes = ::buffa::Message::encode_to_vec(self);
        DynamicMessage::decode(descriptor, &bytes)
            .expect("buffa-reflect: round-trip transcode_to_dynamic must succeed for a self-encoded message")
    }

    /// Phase 2 — encode `dyn_msg` and decode as `Self`. The dynamic
    /// message's descriptor is required to match `Self`'s; mismatch
    /// produces `DynamicError::TypeMismatch`.
    #[cfg(feature = "dynamic")]
    fn from_dynamic(dyn_msg: &DynamicMessage) -> Result<Self, DynamicError>
    where
        Self: ::buffa::Message + Default,
    {
        // Default impl: validate FQN, encode, decode.
    }
}
```

```rust
// crates/buffa-reflect/src/dynamic.rs (Phase 2 — new module)
pub struct DynamicMessage { /* … */ }

impl DynamicMessage {
    pub fn new(descriptor: MessageDescriptor) -> Self;
    pub fn descriptor(&self) -> &MessageDescriptor;
    pub fn parent_pool(&self) -> DescriptorPool;

    pub fn decode(descriptor: MessageDescriptor, bytes: &[u8]) -> Result<Self, DynamicError>;
    pub fn decode_with_options(
        descriptor: MessageDescriptor,
        bytes: &[u8],
        opts: ::buffa::DecodeOptions,
    ) -> Result<Self, DynamicError>;
    pub fn merge_from_slice(&mut self, bytes: &[u8]) -> Result<(), DynamicError>;

    pub fn encode_to_vec(&self) -> Vec<u8>;
    pub fn encode_to_bytes(&self) -> ::buffa::bytes::Bytes;
    pub fn compute_size(&self) -> u32;
    pub fn write_to<B: ::buffa::bytes::BufMut>(&self, buf: &mut B);

    // ── inspection ────────────────────────────────────────────
    pub fn fields(&self) -> impl Iterator<Item = (FieldDescriptor, Cow<'_, Value>)> + '_;
    pub fn populated_fields(&self) -> impl Iterator<Item = (FieldDescriptor, &Value)> + '_;

    pub fn has_field(&self, field: &FieldDescriptor) -> bool;
    pub fn has_field_by_name(&self, name: &str) -> bool;
    pub fn has_field_by_number(&self, number: u32) -> bool;

    pub fn get_field(&self, field: &FieldDescriptor) -> Cow<'_, Value>;
    pub fn get_field_by_name(&self, name: &str) -> Option<Cow<'_, Value>>;
    pub fn get_field_by_number(&self, number: u32) -> Option<Cow<'_, Value>>;

    pub fn get_field_mut(&mut self, field: &FieldDescriptor) -> &mut Value;

    // ── mutation ──────────────────────────────────────────────
    pub fn set_field(
        &mut self,
        field: &FieldDescriptor,
        value: Value,
    ) -> Result<(), SetFieldError>;
    pub fn set_field_by_name(
        &mut self,
        name: &str,
        value: Value,
    ) -> Result<(), SetFieldError>;
    pub fn set_field_by_number(
        &mut self,
        number: u32,
        value: Value,
    ) -> Result<(), SetFieldError>;

    pub fn clear_field(&mut self, field: &FieldDescriptor);
    pub fn clear_field_by_name(&mut self, name: &str) -> bool;
    pub fn clear_field_by_number(&mut self, number: u32) -> bool;

    // ── unknown fields ────────────────────────────────────────
    pub fn unknown_fields(&self) -> &::buffa::UnknownFields;
    pub fn unknown_fields_mut(&mut self) -> &mut ::buffa::UnknownFields;
}
```

```rust
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    Bool(bool),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    String(String),
    Bytes(::buffa::bytes::Bytes),
    /// Enum value as a number — may be `Unknown` from a forward-compat decode.
    EnumNumber(i32),
    Message(DynamicMessage),
    List(Vec<Value>),
    Map(BTreeMap<MapKey, Value>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[non_exhaustive]
pub enum MapKey {
    Bool(bool),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    String(String),
}
```

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DynamicError {
    #[error("decoding {full_name}: {source}")]
    Decode {
        full_name: String,
        #[source]
        source: ::buffa::DecodeError,
    },
    #[error(
        "transcode mismatch: expected `{expected}`, got `{actual}`"
    )]
    TypeMismatch { expected: String, actual: String },
    #[error("dynamic message validation: {0}")]
    Validation(String),
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SetFieldError {
    #[error("no field named `{0}` in `{1}`")]
    UnknownField(String, String),
    #[error("no field with number {0} in `{1}`")]
    UnknownNumber(u32, String),
    #[error(
        "type mismatch on `{full_name}`: field expects {expected}, got {actual}"
    )]
    TypeMismatch {
        full_name: String,
        expected: &'static str,
        actual: &'static str,
    },
    #[error("enum number {value} is not a declared variant of `{enum_name}`")]
    InvalidEnumNumber { enum_name: String, value: i32 },
}
```

That's the public surface. Everything else in this document is the *why* behind these signatures and the *how* of the implementation.

---

## 2. Internal storage

```rust
pub struct DynamicMessage {
    descriptor: MessageDescriptor,
    /// Slots aligned to `descriptor.fields()` position. `None` means
    /// "no value present"; for repeated and map fields the slot is
    /// always `Some(Value::List(_))` / `Some(Value::Map(_))` once the
    /// field has been touched (we eagerly initialise on first decode or
    /// first mutation).
    slots: Vec<Option<Value>>,
    /// One entry per oneof. For an inactive oneof, the value is `None`.
    /// For an active oneof, the value is the field-position (within the
    /// owning message's field list) of the active member.
    oneof_active: Vec<Option<u32>>,
    /// Bytes the descriptor doesn't account for; preserved for
    /// round-trip fidelity, identical convention to buffa's generated
    /// `__buffa_unknown_fields` storage.
    unknown_fields: ::buffa::UnknownFields,
}
```

### Why `Vec<Option<Value>>` instead of `HashMap<u32, Value>`

prost-reflect stores fields in a `BTreeMap<u32, Value>`. We diverge:

- Position-indexed access is O(1) and cache-friendly (contiguous storage).
- The descriptor already enforces that field positions are dense (they're array indices into `MessageEntry::fields`). Storage cost per slot is `size_of::<Option<Value>>()` ≈ 32 bytes — for messages with N fields, the total is `32 * N` bytes regardless of how many fields are populated, vs `48–96 bytes per populated field` for a HashMap. The break-even is around 30% population; **almost every real proto message exceeds 30% populated fields on the wire**, so we win on the common case.
- For **very wide messages** (e.g. `google.protobuf.FileOptions` with 50+ fields, of which a typical proto sets 0–2): the overhead is real but small (~1.6 KB). Acceptable; if profiling later shows it matters, we can switch to a sparse-when-large adaptive scheme.
- Lookups by name/number go through `MessageDescriptor::get_field_by_*` (already O(1) via `by_name`/`by_number` HashMaps in `MessageEntry`) → field's `index` → direct `slots[index]` access. Two hops, both O(1).

### Why a separate `oneof_active` vector

Setting a field that belongs to a oneof must **clear all other members of that oneof**. Without a side index we'd have to iterate `descriptor.oneofs()[oi].fields()` on every set. With it, we read `oneof_active[oi]` (Some(prev_field_pos)), then `slots[prev_field_pos] = None`, then write the new value. Two array writes vs O(K) where K is oneof arity.

### Defaulting: how `get_field` synthesizes values

`get_field` returns a `Cow<'_, Value>`. When the slot is populated, we return `Cow::Borrowed(&Value)`. When unset, we synthesize the proto-default and return `Cow::Owned`:

| Field shape | Default synthesized when slot is `None` |
| --- | --- |
| singular scalar | `Value::I32(0)` / `Value::Bool(false)` / `Value::String(String::new())` etc. |
| singular enum | `Value::EnumNumber(0)` |
| singular message | `Value::Message(DynamicMessage::new(submessage_descriptor))` |
| repeated scalar | `Value::List(Vec::new())` |
| map | `Value::Map(BTreeMap::new())` |

Defaults for **proto2** singular fields with explicit `default_value = ...` honor the descriptor's value (parsed lazily via `FieldDescriptorProto::default_value`).

### `populated_fields` vs `fields`

- `fields()` — every field in the descriptor, paired with its current value (synthesized defaults for unset fields). Useful for serialization / "show me everything."
- `populated_fields()` — only fields with explicit values present (`has_field` semantics). Useful for "show me what was actually set" (proto3 omits-default-on-encode is the most common case).

---

## 3. Wire encode

We mirror the buffa codec contract: a two-pass `compute_size` → `write_to`. The dispatch is descriptor-driven.

```rust
impl DynamicMessage {
    pub fn compute_size(&self) -> u32 {
        let mut cache = ::buffa::SizeCache::new();
        compute_message_size(self, &mut cache)
    }

    pub fn encode_to_vec(&self) -> Vec<u8> {
        let mut cache = ::buffa::SizeCache::new();
        let size = compute_message_size(self, &mut cache);
        let mut buf = Vec::with_capacity(size as usize);
        write_message_to(self, &mut cache, &mut buf);
        buf
    }
}
```

### Why two-pass

buffa's wire format requires the **length prefix** of every length-delimited sub-message to be known *before* writing its body. The standard pattern (also used by prost) is to compute and cache sub-sizes, then write with the cached values. `SizeCache` is an existing buffa primitive — we re-use it verbatim.

### Field iteration order

Critical: emit fields in the **order the descriptor lists them** (`MessageEntry::fields[i].proto_field_index` order). This is the same order buffa's generated code uses and what `protoc --decode_raw` will produce. Deviating breaks the byte-equivalence test in §10.

### Per-Kind encoder dispatch

Each `KindRef` maps to one of buffa's encoder primitives. The dispatch lives in a `encode_value(field_desc, value, cache, buf)` function — table here, code in `dynamic_codec.rs`:

| `KindRef` | Wire type | Encoder |
| --- | --- | --- |
| `Double` | `Bit64` | `encoding::encode_double` |
| `Float` | `Bit32` | `encoding::encode_float` |
| `Int32`/`Int64` | `Varint` | `encoding::encode_int32` / `int64` |
| `Uint32`/`Uint64` | `Varint` | `encoding::encode_uint32` / `uint64` |
| `Sint32`/`Sint64` | `Varint` (zigzag) | `encoding::encode_sint32` / `sint64` |
| `Fixed32`/`Fixed64` | `Bit32`/`Bit64` | `encoding::encode_fixed32` / `fixed64` |
| `Sfixed32`/`Sfixed64` | `Bit32`/`Bit64` | `encoding::encode_sfixed32` / `sfixed64` |
| `Bool` | `Varint` | `encoding::encode_bool` |
| `String` | `LengthDelimited` | `types::encode_string` |
| `Bytes` | `LengthDelimited` | `types::encode_bytes` |
| `Enum` | `Varint` | `types::encode_int32` (the i32 number) |
| `Message` | `LengthDelimited` | `compute_size` recursively + length-prefixed body |

### Repeated fields & packed encoding

For repeated scalars where `field.is_packed()` is true, emit a single tag with `LengthDelimited` wire type and a single body containing concatenated values (no per-value tags). This matches buffa's generated code.

For non-packed repeated, emit one tag-and-value per element.

For repeated messages: always one tag-and-length-delimited-body per element.

### Map fields

A `map<K, V>` field encodes as a repeated message of synthetic `XxxEntry { K key = 1; V value = 2; }`. We synthesize the encoding ourselves rather than instantiating an entry `DynamicMessage` per entry — cheaper. The encoding loop:

```text
for (k, v) in map:
  cache_slot = cache.reserve()
  inner_size = size_of_key(k, key_kind) + size_of_value(v, value_kind)
  cache.set(cache_slot, inner_size)
  write tag(field_number, LengthDelimited)
  write varint(inner_size)
  write tag(1, key_wire_type); write key bytes
  write tag(2, value_wire_type); write value bytes
```

### Oneof emission

A oneof contributes **at most one tag** to the wire, corresponding to the active member. We read `oneof_active[oneof_index]`, look up the field, and emit it normally (its `proto_field_index` placement in the iteration order doesn't matter — protoc accepts any ordering, but for byte-equivalence we keep the descriptor's order).

### Unknown fields

After all known fields are written, append `unknown_fields.write_to(buf)` (buffa primitive). This preserves any tags the descriptor doesn't recognize, in the order they were observed on decode.

---

## 4. Wire decode

```rust
pub fn merge_from_slice(&mut self, bytes: &[u8]) -> Result<(), DynamicError> {
    let mut buf = bytes;
    decode_into(self, &mut buf, ::buffa::RECURSION_LIMIT)
}
```

The decoder loops over wire tags and dispatches by field number:

1. `let tag = encoding::Tag::decode(&mut buf)?;`
2. Look up the field by number via `descriptor.get_field_by_number(tag.field_number())`:
   - **Hit**: dispatch by the field's `KindRef` and `cardinality`.
   - **Miss**: append to `unknown_fields` via `encoding::decode_unknown_field`.
3. Validate the tag's wire type matches what the field expects; if not → `DynamicError::Decode { source: WireTypeMismatch }`. Exception: a packed-encoded repeated scalar may legally appear as `LengthDelimited` even when its declared wire type is `Varint` — handle that case explicitly (reads a length-prefixed packed body and decodes individual elements).
4. For oneof members: when we set the slot, also set `oneof_active[oi] = Some(field_pos)` and clear any previously-active member of the same oneof.
5. Recursion: pass `depth - 1` into recursive sub-message decoding; bail with `RecursionLimitExceeded` at zero. Mirrors buffa's existing recursion-limit pattern.

### Forward-compat enum decoding

An enum field whose wire value is unknown to the descriptor is stored as `Value::EnumNumber(raw)` — *not* dropped. This matches proto3's "open enum" semantics and what buffa generates (`EnumValue::Unknown(raw)`). Round-trip through `encode_to_vec` re-emits the unknown number byte-identically.

---

## 5. Mutation API

### `set_field` validation

`set_field(field, value)` does **type validation** before mutating. The validation table:

| Field's `KindRef` | Acceptable `Value` shapes |
| --- | --- |
| `Bool` | `Value::Bool` |
| `I32` / `Sint32` / `Sfixed32` | `Value::I32` |
| `I64` / `Sint64` / `Sfixed64` | `Value::I64` |
| `U32` / `Fixed32` | `Value::U32` |
| `U64` / `Fixed64` | `Value::U64` |
| `F32` | `Value::F32` |
| `F64` | `Value::F64` |
| `String` | `Value::String` |
| `Bytes` | `Value::Bytes` |
| `Enum` | `Value::EnumNumber` (or `Value::I32` accepted as an alias) |
| `Message` | `Value::Message` (whose descriptor must match the field's expected message type — or be a descendant in the same pool) |

For repeated fields: `Value::List(items)`; each item validated against the field's element kind.

For map fields: `Value::Map(entries)`; key validated against the map key kind, value validated against the value kind.

Mismatch → `SetFieldError::TypeMismatch`. Sub-message descriptor mismatch → `SetFieldError::TypeMismatch` with `expected = "message<acme.User>", actual = "message<acme.Org>"`.

### Strict vs lax enum acceptance

Phase 2a accepts any `i32` for an enum-typed field — matches proto3's open-enum semantics. A future optional `set_field_strict` could reject unknown enum numbers; not in this phase.

### Oneof mutation invariant

Setting a field that belongs to a oneof:

1. Reads `oneof_active[oi]`. If `Some(prev_field_pos)` and `prev_field_pos != new_field_pos`: `slots[prev_field_pos] = None`.
2. Writes `slots[new_field_pos] = Some(value)`.
3. Writes `oneof_active[oi] = Some(new_field_pos)`.

`clear_field` on a oneof member: `slots[field_pos] = None`; `oneof_active[oi] = None` (only if the cleared field was the active one — which for oneof members it always is, by invariant 1 above).

---

## 6. `ReflectMessage` extension

Two new methods, both default-implemented via a wire round-trip:

```rust
#[cfg(feature = "dynamic")]
fn transcode_to_dynamic(&self) -> DynamicMessage {
    let descriptor = self.descriptor();
    let bytes = ::buffa::Message::encode_to_vec(self);
    DynamicMessage::decode(descriptor, &bytes)
        .expect("self-encoded message must decode")
}

#[cfg(feature = "dynamic")]
fn from_dynamic(dyn_msg: &DynamicMessage) -> Result<Self, DynamicError>
where
    Self: ::buffa::Message + Default,
{
    let dyn_fqn = dyn_msg.descriptor().full_name();
    let static_fqn = Self::default().descriptor().full_name().to_string();
    if dyn_fqn != static_fqn {
        return Err(DynamicError::TypeMismatch {
            expected: static_fqn,
            actual: dyn_fqn.to_string(),
        });
    }
    let bytes = dyn_msg.encode_to_vec();
    Self::decode_from_slice(&bytes).map_err(|source| DynamicError::Decode {
        full_name: static_fqn,
        source,
    })
}
```

Both impls have a single failure mode worth highlighting:
- `transcode_to_dynamic` cannot fail — encoding our own struct yields bytes, and decoding bytes our own descriptor produced cannot mismatch wire types. The `expect` is therefore safe and documented.
- `from_dynamic` can fail if (a) the descriptors mismatch, or (b) the dynamic message contains values that the static type's decoder rejects (e.g., a `required` field is missing in proto2). Both surface through `DynamicError`.

Both are `#[cfg(feature = "dynamic")]` so the trait is unchanged when the feature is off.

---

## 7. `feature = "dynamic"`

```toml
# crates/buffa-reflect/Cargo.toml
[features]
default = ["derive", "dynamic"]
derive = ["dep:buffa-reflect-derive"]
dynamic = []
```

`dynamic` is on by default. Consumers who want a leaner runtime (e.g., Phase-1-only typed reflection) opt out via `default-features = false, features = ["derive"]`.

`dynamic` does not introduce new external deps — `Bytes`, `BTreeMap`, `Cow` are all already available (`buffa::bytes`, `std::collections`, `std::borrow`).

---

## 8. Module layout

```
crates/buffa-reflect/src/
  lib.rs           # re-exports gated on `dynamic`
  pool.rs          # unchanged
  pool_build.rs    # unchanged
  message.rs       # +1 method: default_dynamic() -> DynamicMessage (factory)
  field.rs         # unchanged
  enumeration.rs   # unchanged
  oneof.rs         # unchanged
  reflect.rs       # +2 trait methods (dynamic-feature-gated)
  dynamic/
    mod.rs         # public types: DynamicMessage, Value, MapKey, errors
    storage.rs     # internal: slot vec, oneof_active, unknown_fields
    encode.rs      # compute_size + write_to dispatch
    decode.rs      # merge_from_slice loop + per-Kind decode
    value.rs       # Value/MapKey impls (PartialEq, From, accessors)
    accessors.rs   # get/set/has/clear surface
    defaults.rs    # default-value synthesis (proto2 default_value parsing)
```

`dynamic/` mirrors prost-reflect's split. Each file is small and focused.

---

## 9. Cycles, recursion, depth limits

- **Cyclic message types** (e.g., `message Node { Node child = 1; }`) work — `Value::Message(DynamicMessage)` heap-allocates inside the parent's `Vec`. There's no compile-time issue and no recursive-allocation issue at construction (the user can build arbitrarily deep nesting; we don't pre-populate sub-messages).
- **Decode recursion**: capped at `RECURSION_LIMIT` (100), same default as buffa. Configurable via `decode_with_options(descriptor, bytes, DecodeOptions::new().with_recursion_limit(N))`. This guards against malicious deeply-nested inputs.
- **Encode recursion**: not capped. The user constructed the tree, so we trust them; if you build a 10000-level tree and OOM, that's the cost. (We could add a soft cap; not in Phase 2a.)
- **`Drop` recursion**: a deeply-nested `DynamicMessage` drops its children recursively. For pathological inputs (10⁵+ levels), this could stack-overflow. Mitigation: same as Vec/Box — for ordinary protos this is irrelevant. If a real workload exposes it, switch to an iterative `Drop`.

---

## 10. Testing & acceptance

Unit tests live in `crates/buffa-reflect/src/dynamic/*.rs` (`#[cfg(test)] mod tests`). Integration tests under `crates/buffa-reflect/tests/dynamic.rs`.

Acceptance bar:

- **Byte-equivalence round-trip**: for the equivalence fixtures (already covers all scalars, maps, oneofs, synthetic oneofs, nested + doubly-nested, cross-file imports), `DynamicMessage::decode(d, bytes).encode_to_vec() == bytes` for every populated field combination produced by:
  - empty,
  - one of each scalar type,
  - the oneof in each of its arms,
  - the synthetic-oneof set and unset,
  - a non-empty repeated and non-empty map,
  - a nested message at depth 1, 2, 3.
- **Set/get round-trip**: for each kind, set a value then read it back; `get == set`.
- **Type-mismatch rejection**: `set_field_by_name("count", Value::String("x".into()))` returns `SetFieldError::TypeMismatch` (not panic).
- **Unknown-field preservation**: decode with extra wire data not in the descriptor; encode; observe extra bytes are still present.
- **Recursion-limit enforcement**: a hand-crafted 200-level nested wire input fails with `RecursionLimitExceeded` at default depth, succeeds at `with_recursion_limit(300)`.
- **`transcode_to_dynamic` round-trip**: for every fixture in the equivalence suite, `typed.encode_to_vec() == typed.transcode_to_dynamic().encode_to_vec()`.
- **`from_dynamic` round-trip**: `User::from_dynamic(&user.transcode_to_dynamic())? == user`.
- **Send + Sync**: the existing `const _:` assertion in `pool.rs` extended to cover `DynamicMessage`.
- **`cargo test --workspace --no-default-features --features=derive` clean** — proves `dynamic` is a clean opt-out.

---

## 11. Performance characteristics

These are **expected**, not measured (benchmarks deferred to a separate spec):

| Operation | Complexity | Notes |
| --- | --- | --- |
| `new(descriptor)` | O(N) allocation where N = number of fields | Pre-sized `Vec<Option<Value>>` |
| `get_field_by_name` | O(1) name → field lookup + O(1) slot read | Two HashMap probes via `MessageEntry::by_name` then direct array access |
| `set_field` | O(1) for scalar, O(1) for message | + oneof clear cost (O(1)) when applicable |
| `decode` | O(B) where B = wire bytes | Each field number lookup is O(1); recursion adds nothing beyond per-tag work |
| `encode` | 2× O(B) due to two-pass (size then write) | Standard buffa pattern; SizeCache amortizes |
| `clone` | O(N + total payload) | Vec clone + per-Value clone; `Bytes` clones are O(1) refcount bumps |

prost-reflect benchmarks at roughly **2–3×** the cost of typed encode/decode for representative messages. We expect parity since the algorithms are structurally identical and we share buffa's primitives.

---

## 12. Interaction with Phase 1

- **Backward compatible.** All Phase 1 APIs unchanged.
- **`DescriptorPool::add_file_descriptor_set` and `Arc::make_mut`**: a `DynamicMessage` constructed against a pool clone retains its own pool snapshot through the cheap `Arc` clone. If the parent pool is later mutated via `add_file_descriptor_set`, the dynamic message keeps pointing at the snapshot it was built against — no dangling, but also no auto-discovery of newly-added types. Documented.
- **`MessageDescriptor::default_dynamic()` factory** added (so `descriptor.default_dynamic() == DynamicMessage::new(descriptor)`). Convenience only.

---

## 13. Risks & mitigations (design-time)

| Risk | Mitigation |
| --- | --- |
| Wire encode produces bytes in a different field-tag order than buffa's typed encoder, breaking interop tests. | Iterate fields in declaration (`proto_field_index`) order verbatim. Test against fixtures generated by both buffa typed code and `protoc --encode`. |
| `Value::Message(DynamicMessage)` makes `Value` huge (recursive enum carries another `Vec` etc.). | Acceptable for clarity; the enum itself is the size of its largest variant (`Value::Message ≈ 64 bytes`). For cache pressure, the dominant allocation is the inner message's `slots`, not the enum tag. If profiling reveals a problem, box the `Message` variant. |
| `set_field` sub-message cross-pool validation: a `Value::Message(other)` from a different `DescriptorPool` refers to descriptors not in `self`'s pool. | Compare by `Arc::ptr_eq` on the inner pool. Mismatch → `SetFieldError::TypeMismatch`. (Permissive cross-pool transcoding can come later as an explicit method.) |
| Map encoding ordering is non-deterministic across runs unless the underlying map is sorted. | Use `BTreeMap` for `Value::Map` so iteration is in key-sort order. Wire conformance does not require this, but tests and JSON output benefit. |
| Default values for proto2 (`default_value = "..."`) require parsing the descriptor's string field. | Parse lazily on `get_field` for unset proto2 fields with explicit defaults; cache the parsed `Value` per `FieldDescriptor` via a `OnceLock` in `FieldEntry` (next phase, not 2a). For 2a: parse on every read; unconditionally returns the documented default. |
| `Bytes` field encoding: buffa generates `Vec<u8>` by default and `Bytes` only via `use_bytes_type_in`. We always store `Value::Bytes(Bytes)`. | `Bytes::from(vec)` is O(1). Encoding is O(N) write either way. No issue. |

---

## 14. Out of scope, captured for follow-ups

- **`DynamicMessage::merge`** — merging two dynamics with the same descriptor (proto's "merge" semantics). Phase 2a ships only "merge from wire bytes." A pure-Rust merge variant is the next-step convenience.
- **`Value::try_into_*` / `From` impls in both directions** — accessors get tedious without sugar. Phase 2a provides the manual `match` API; sugar via macros is a small follow-up.
- **`DynamicMessage::clear_all()`** — clearing every populated field (preserving descriptor). Trivial; ship if needed.
- **`Reflect` derive on view types** — see Phase 2e.
- **JSON / textproto** — see Phases 2b and 2c.
