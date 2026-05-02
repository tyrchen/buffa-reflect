# buffa-reflect Phase 2a — `DynamicMessage` Design

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). This document covers **only** Phase 2a — the `DynamicMessage` type, its `Value` model, the wire encode/decode dispatch, and the new methods on `ReflectMessage`. JSON, textproto, gRPC reflection, and view reflection have their own specs and depend on this one.

Pre-reads:
- [buffa-reflect Phase 1 design](./buffa-reflect-design.md) — descriptor pool, handles, validation
- [docs/research/buffa-architecture.md](../docs/research/buffa-architecture.md) — buffa's wire-encoding surface
- prost-reflect source (`vendors/prost-reflect/prost-reflect/src/dynamic/`) — the reference implementation; this design follows its choices closely so consumers migrating between the two ecosystems find the same shape.

> **Note on the audit.** An earlier draft of this spec proposed a `Vec<Option<Value>>` storage model. After auditing the prost-reflect source — where `BTreeMap<u32, ValueOrUnknown>` is used, with a `Taken` sentinel for draining iterators, a `FieldDescriptorLike` trait that unifies fields and extensions, and a dual `set_field`-panics / `try_set_field`-returns-`Result` API — the model below was revised to track those choices. The reasoning is captured under "Decisions revised after the audit" in §15.

---

## 1. Surface area at a glance

```rust
// crates/buffa-reflect/src/lib.rs
#[cfg(feature = "dynamic")]
pub use crate::dynamic::{
    DynamicMessage, MapKey, SetFieldError, UnknownField, UnknownFieldSet, Value,
};
```

```rust
// crates/buffa-reflect/src/reflect.rs (Phase 2 extension)
pub trait ReflectMessage: ::buffa::Message {
    fn descriptor(&self) -> MessageDescriptor;

    /// Transcode `self` to a [`DynamicMessage`].
    ///
    /// Default impl encodes `self` and decodes the bytes against
    /// `self.descriptor()`. The cost is one wire round-trip — accept-
    /// able for the common case; specialisations may override (e.g.
    /// `DynamicMessage::transcode_to_dynamic` returns `self.clone()`).
    #[cfg(feature = "dynamic")]
    fn transcode_to_dynamic(&self) -> DynamicMessage
    where
        Self: Sized,
    {
        let descriptor = self.descriptor();
        let bytes = ::buffa::Message::encode_to_vec(self);
        DynamicMessage::decode(descriptor, bytes.as_slice())
            .expect("buffa-reflect: a self-encoded message must decode")
    }
}
```

```rust
// crates/buffa-reflect/src/dynamic/mod.rs (Phase 2 — new)
pub struct DynamicMessage {
    desc: MessageDescriptor,
    fields: DynamicMessageFieldSet,  // pub(crate)
}

impl DynamicMessage {
    pub fn new(desc: MessageDescriptor) -> Self;
    pub fn descriptor(&self) -> &MessageDescriptor;
    pub fn parent_pool(&self) -> DescriptorPool;

    // ── decode ────────────────────────────────────────────────
    pub fn decode<B: ::buffa::bytes::Buf>(desc: MessageDescriptor, buf: B) -> Result<Self, ::buffa::DecodeError>;
    pub fn decode_with_options<B: ::buffa::bytes::Buf>(
        desc: MessageDescriptor,
        buf: B,
        opts: ::buffa::DecodeOptions,
    ) -> Result<Self, ::buffa::DecodeError>;
    pub fn merge<B: ::buffa::bytes::Buf>(&mut self, buf: B) -> Result<(), ::buffa::DecodeError>;

    // ── encode ────────────────────────────────────────────────
    pub fn encoded_len(&self) -> usize;
    pub fn encode<B: ::buffa::bytes::BufMut>(&self, buf: &mut B) -> Result<(), ::buffa::EncodeError>;
    pub fn encode_to_vec(&self) -> Vec<u8>;
    pub fn encode_to_bytes(&self) -> ::buffa::bytes::Bytes;

    // ── fast-path conversions ─────────────────────────────────
    pub fn transcode_from<T: ::buffa::Message>(&mut self, value: &T) -> Result<(), ::buffa::DecodeError>;
    pub fn transcode_to<T: ::buffa::Message + Default>(&self) -> Result<T, ::buffa::DecodeError>;

    // ── inspection / iteration ────────────────────────────────
    pub fn fields(&self) -> impl Iterator<Item = (FieldDescriptor, &Value)> + '_;
    pub fn iter_with_options<'a>(
        &'a self,
        include_default: bool,
        index_order: bool,
    ) -> impl Iterator<Item = (FieldDescriptor, Cow<'a, Value>)> + 'a;

    pub fn has_field(&self, field: &FieldDescriptor) -> bool;
    pub fn get_field(&self, field: &FieldDescriptor) -> Cow<'_, Value>;
    pub fn get_field_mut(&mut self, field: &FieldDescriptor) -> &mut Value;

    pub fn has_field_by_name(&self, name: &str) -> bool;
    pub fn get_field_by_name(&self, name: &str) -> Option<Cow<'_, Value>>;
    pub fn get_field_by_name_mut(&mut self, name: &str) -> Option<&mut Value>;

    pub fn has_field_by_number(&self, number: u32) -> bool;
    pub fn get_field_by_number(&self, number: u32) -> Option<Cow<'_, Value>>;
    pub fn get_field_by_number_mut(&mut self, number: u32) -> Option<&mut Value>;

    // ── mutation: dual API ────────────────────────────────────
    /// Validates with `debug_assert!` (zero cost in release builds).
    /// Panics on type mismatch in debug. Use `try_set_field` if the
    /// value type is data-driven.
    pub fn set_field(&mut self, field: &FieldDescriptor, value: Value);
    pub fn try_set_field(&mut self, field: &FieldDescriptor, value: Value) -> Result<(), SetFieldError>;
    pub fn set_field_by_number(&mut self, number: u32, value: Value);
    pub fn try_set_field_by_number(&mut self, number: u32, value: Value) -> Result<(), SetFieldError>;
    pub fn set_field_by_name(&mut self, name: &str, value: Value);
    pub fn try_set_field_by_name(&mut self, name: &str, value: Value) -> Result<(), SetFieldError>;

    pub fn clear_field(&mut self, field: &FieldDescriptor);
    pub fn clear_field_by_name(&mut self, name: &str);
    pub fn clear_field_by_number(&mut self, number: u32);

    // ── unknown fields ────────────────────────────────────────
    pub fn unknown_fields(&self) -> impl Iterator<Item = (u32, &UnknownFieldSet)> + '_;
    pub fn drain_unknown_fields(&mut self) -> impl Iterator<Item = (u32, UnknownFieldSet)> + '_;
}

/// `DynamicMessage` is itself a `ReflectMessage`; `transcode_to_dynamic`
/// short-circuits to `self.clone()`, avoiding the wire round-trip.
impl ReflectMessage for DynamicMessage { /* … */ }
```

```rust
#[derive(Debug, Clone, PartialEq)]
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
    /// Enum variant by number. `i32` rather than a typed variant so
    /// forward-compat decoding (unknown enum number) round-trips
    /// losslessly — proto3's "open enum" semantics.
    EnumNumber(i32),
    Message(DynamicMessage),
    List(Vec<Value>),
    Map(HashMap<MapKey, Value>),
}

impl Value {
    pub fn default_value(kind: &Kind) -> Self;
    pub fn default_value_for_field(field: &FieldDescriptor) -> Self;
    pub fn is_default(&self, kind: &Kind) -> bool;
    /// Recursive validation: for `List`, every element validated against
    /// the list's Kind; for `Map`, key against the key kind, value against
    /// the value kind.
    pub fn is_valid_for_field(&self, field: &FieldDescriptor) -> bool;

    // Typed accessors — Some(_) iff the variant matches.
    pub fn as_bool(&self) -> Option<bool>;
    pub fn as_i32(&self) -> Option<i32>;
    pub fn as_i64(&self) -> Option<i64>;
    // … (one per scalar / enum / message / list / map variant, plus _mut variants)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKey {
    Bool(bool),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    String(String),
    // No Bytes: map<bytes, _> is forbidden by the proto spec.
    // No floats: map keys cannot be floats.
}
```

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum SetFieldError {
    /// The field name / number resolution returned no descriptor.
    NotFound,
    /// `Value::is_valid_for_field(&value, field)` returned false.
    InvalidType {
        field: FieldDescriptor,
        value: Value,
    },
}
```

That's the public surface. The rest of this document is the *why* behind these signatures and the *how* of the implementation.

---

## 2. Internal storage

Mirroring prost-reflect (`vendors/prost-reflect/prost-reflect/src/dynamic/fields.rs`):

```rust
#[derive(Default, Debug, Clone, PartialEq)]
pub(super) struct DynamicMessageFieldSet {
    fields: BTreeMap<u32, ValueOrUnknown>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ValueOrUnknown {
    /// A protobuf value with a known field type.
    Value(Value),
    /// One or more unknown fields, keyed by the original wire number.
    Unknown(UnknownFieldSet),
    /// Sentinel used during draining iteration to prevent re-visit.
    Taken,
}
```

### Why `BTreeMap<u32, ValueOrUnknown>` (revised from the earlier `Vec<Option<Value>>` proposal)

The earlier draft argued for `Vec<Option<Value>>` aligned to descriptor field positions: O(1) lookup, no sparse-storage overhead, predictable memory shape. The audit talked me out of it. Specifically:

1. **Unknown fields belong on the same axis as known fields.** Wire data is "field number → value". Unknown fields don't have a descriptor index, so a position-aligned vector cannot hold them. prost-reflect handles this by interleaving `Value` and `Unknown(UnknownFieldSet)` entries in the same map keyed by `u32` (`fields.rs:39-51`). On encode, iteration is in field-number order — known and unknown emit interleaved, naturally producing canonical wire output.
2. **Field-number-sorted iteration is the natural canonical order.** Most consumers want stable serialization; a `BTreeMap` is sorted by construction. (For textproto and JSON we also offer index-order via a separate option — see §6.)
3. **Sparse messages are common.** Wide messages (`google.protobuf.FileOptions` has 50+ fields, of which a typical proto sets 0–2) pay 32 × N bytes per `Vec<Option<Value>>` regardless of population. With a `BTreeMap`, the cost scales with the populated count. The constant-factor difference per populated field is small (~80 bytes for a BTreeMap node vs 32 bytes for an Option<Value> slot), but the absolute waste of the dense Vec on wide-and-sparse messages is meaningful.
4. **Migration from prost-reflect.** Sharing the storage model means consumers can mechanically translate code between the two crates. Less surprise.

The cost we accept: O(log N) lookup vs O(1). For typical proto messages (N < 50 fields), this is single-digit comparisons — neither approach is a hot-path bottleneck once the descriptor lookup itself (which is also O(1) via `MessageEntry::by_number`) is amortised.

### `ValueOrUnknown::Taken` sentinel

`Taken` is used by **draining** iterators (e.g., `drain_unknown_fields`) to keep the entry in the BTreeMap (preserving the key order) while the value has been moved out. Without this sentinel, draining would either remove entries on the fly (which mutates the iterated structure) or require collecting keys first (a double-walk). Same idiom as `prost-reflect/src/dynamic/fields.rs:46`.

### `FieldDescriptorLike` trait

prost-reflect abstracts the `set` / `get` / `has` / `clear` operations over a `FieldDescriptorLike` trait that both `FieldDescriptor` and `ExtensionDescriptor` implement (`fields.rs:17-35`). This avoids duplicating the entire mutation surface for extensions. We adopt the same trait shape; for Phase 2a, only `FieldDescriptor` implements it. Extensions (a future phase) drop in without changing the storage layer.

```rust
pub(crate) trait FieldDescriptorLike: fmt::Debug {
    fn number(&self) -> u32;
    fn default_value(&self) -> Value;
    fn is_default_value(&self, value: &Value) -> bool;
    fn is_valid(&self, value: &Value) -> bool;
    fn containing_oneof(&self) -> Option<OneofDescriptor>;
    fn supports_presence(&self) -> bool;
    fn kind(&self) -> Kind;
    fn is_list(&self) -> bool;
    fn is_map(&self) -> bool;
    fn is_packed(&self) -> bool;
    fn is_packable(&self) -> bool;
    /// "has" semantics: presence-tracking fields → set bit; otherwise
    /// → non-default value.
    fn has(&self, value: &Value) -> bool {
        self.supports_presence() || !self.is_default_value(value)
    }
}
```

### Defaulting: `get_field` returns `Cow<'_, Value>`

When the slot is populated, `get_field` returns `Cow::Borrowed(&Value)`. When unset, it synthesises the field's default via `desc.default_value()` and returns `Cow::Owned`. Identical pattern to `prost-reflect/src/dynamic/fields.rs:73-78`.

### Default-value caching is **eager at pool-build time**

Phase 1 deferred proto2 explicit `default_value` parsing. Phase 2a moves it to pool-build time, mirroring `prost-reflect/src/descriptor/build/resolve.rs:627-698`:

- For each `FieldDescriptorProto::default_value` string, parse once and store the resulting `Value` on the `FieldEntry`.
- Parser handles: signed/unsigned int (decimal, octal, hex), float (incl. `inf`, `-inf`, `nan`), bool literals, C-escaped strings, byte string with octal/hex escapes, and enum-by-name (looked up in the field's enum descriptor).
- Bad defaults accumulate on a pool-level error list (matches prost-reflect's "all errors at once" model). `DescriptorPool::decode` returns an `Err` aggregating all such failures rather than the first one — better diagnostics.

This is a Phase 1 amend on `FieldEntry`: one `default: OnceLock<Value>` field plus the parser. ~150 LOC in `pool_build.rs`. No public-API change.

---

## 3. Wire encode

Two-pass `encoded_len` → `encode`, mirroring buffa's `Message` contract.

```rust
impl DynamicMessage {
    pub fn encoded_len(&self) -> usize {
        let mut size = 0;
        for (field, value) in self.fields.iter_known() {
            size += encode_value_len(&field, value);
        }
        for (number, unknown) in self.fields.iter_unknown() {
            size += unknown.encoded_len(number);
        }
        size
    }

    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), EncodeError> {
        for entry in self.fields.iter_in_number_order() {
            match entry {
                Entry::Field(field, value) => encode_value(&field, value, buf)?,
                Entry::Unknown(number, unknown) => unknown.encode(number, buf)?,
            }
        }
        Ok(())
    }
}
```

### Why interleaved iteration

On the wire, fields are tag-prefixed integers. There's no requirement to sort, but **canonical** output (and the byte-equivalence test) requires stable order. `BTreeMap<u32, _>` already iterates in number order; interleaving `Value` and `Unknown` in that single iteration means encode preserves the original wire order observed on decode. prost-reflect does the same (`vendors/prost-reflect/prost-reflect/src/dynamic/message.rs:62-244`).

### Per-`Kind` encoder dispatch

Single big match per `Kind × Value × cardinality` combination; no dispatch table. Reasoning: the match arms are short (call into a buffa primitive) and `match` lets the compiler emit the optimal jump table. Same pattern as prost-reflect (`message.rs:93-244`).

### Packed encoding decision is **per-encode**, not pre-computed

`field.is_packed()` is checked at encode time; no caching. For repeated scalars, packed emits a single tag with `LengthDelimited` wire type and concatenated values; non-packed emits one tag-and-value per element. Negligible cost (`is_packed` is a flag on the FieldEntry); matches prost-reflect's choice (`message.rs:141-244`).

### Map field encoding

A `map<K, V>` field encodes as a repeated message of synthetic `Entry { K key = 1; V value = 2; }`. We synthesise the encoding directly (not by instantiating an entry `DynamicMessage` per pair) — a few percent saving on map-heavy messages.

For deterministic output we need stable map iteration. **However**, our `Value::Map` uses `HashMap<MapKey, Value>` to match prost-reflect's choice (`vendors/prost-reflect/prost-reflect/src/dynamic/mod.rs:76`). Wire conformance does not require map ordering, but textproto / JSON tests do. Encoding sorts entries by key on the fly when `MapKey` admits ordering (every `MapKey` variant does — both `MapKey` and the underlying types are `Ord`). Cost: one `Vec<&MapKey>` allocation + sort per encoded map. Acceptable.

### Oneof emission

A oneof contributes at most one tag to the wire — the active member. Because oneof members have distinct field numbers and live in the same `BTreeMap`, no special handling: the active member appears at its natural position; cleared siblings are simply absent. Same as prost-reflect.

---

## 4. Wire decode

```rust
pub fn merge<B: Buf>(&mut self, mut buf: B) -> Result<(), DecodeError> {
    let depth_limit = ::buffa::RECURSION_LIMIT;
    while buf.has_remaining() {
        let tag = ::buffa::encoding::Tag::decode(&mut buf)?;
        match self.desc.get_field(tag.field_number()) {
            Some(field) => self.merge_field(&field, tag.wire_type(), &mut buf, depth_limit)?,
            None => self.fields.add_unknown(tag.field_number(),
                ::buffa::encoding::decode_unknown_field(tag, &mut buf, depth_limit)?),
        }
    }
    Ok(())
}
```

### Wire-type tolerance

For repeated scalars, both `LengthDelimited` (packed body) and the field's natural wire type (one-tag-per-value) are accepted regardless of the descriptor's `is_packed()` flag — protoc emits either depending on the source proto's syntax/edition. We dispatch by observed wire type, not by descriptor declaration. Matches prost-reflect's behaviour (`message.rs:35-59`).

### Forward-compat enum decoding

Unknown enum numbers are stored as `Value::EnumNumber(raw)` rather than dropped. Re-encoding emits the same number byte-identically. Matches proto3's open-enum semantics and what buffa's typed code does with `EnumValue::Unknown(raw)`.

### Recursion limit

Default `::buffa::RECURSION_LIMIT` (100). Configurable via `decode_with_options(desc, buf, DecodeOptions::new().with_recursion_limit(N))`. Bails with `DecodeError::RecursionLimitExceeded`. Mirrors buffa's typed-decode behaviour exactly so the dynamic and typed paths can't disagree on what's safe to decode.

### Unknown-field merging

Multiple unknown tags with the same number accumulate in a single `UnknownFieldSet` (a Vec). Insertion order preserved per number. The interleaved BTreeMap layout ensures known and unknown tags for the *same number* coexist gracefully — practical case: a message decoded against an old descriptor (some fields unknown), then merged with a new descriptor (those fields now known) results in known values overwriting unknowns at the same number, while truly-unknown numbers stay separate.

---

## 5. Mutation API: dual `set_field` / `try_set_field`

```rust
pub fn set_field(&mut self, field: &FieldDescriptor, value: Value) {
    self.try_set_field(field, value).unwrap()
}

pub fn try_set_field(
    &mut self,
    field: &FieldDescriptor,
    value: Value,
) -> Result<(), SetFieldError> {
    if value.is_valid_for_field(field) {
        self.fields.set(field, value);
        Ok(())
    } else {
        Err(SetFieldError::InvalidType { field: field.clone(), value })
    }
}
```

The `_by_number` / `_by_name` variants additionally resolve the descriptor and return `SetFieldError::NotFound` when the lookup fails.

### Why dual API (not just Result)

Earlier draft argued for Result-only. The audit changed my mind:

- **`set_field` is the most common operation** in code that's already type-checked the value against the descriptor (e.g., a serializer that just deserialized into `Value`). Forcing `.unwrap()` at every call site is ergonomic friction.
- **`try_set_field` exists for data-driven writes** (e.g., a UI editor where the value type may not match the field). Callers who need that branch already pay the lookup cost; checking `Result` is fine.
- **`debug_assert!` in `fields.rs:97-100`** means the underlying validation is **free in release builds**. The `set_field` panic path only fires in debug, and never in production. This is the right ergonomic for "I've already validated" code.

We adopt the same pattern: `set_field` is a thin wrapper around `try_set_field().unwrap()`, with `debug_assert!` validation at the storage layer to catch programming errors during development without paying for them in release.

### Oneof mutation invariant

`get_field_mut` / `set` both call `clear_oneof_fields` first (`fields.rs:81, 102, 107-114`), which iterates the oneof's siblings and clears any other active member. We mirror this — eager clearing at mutation time, not lazy at iteration time.

---

## 6. Iteration

```rust
pub fn iter_with_options<'a>(
    &'a self,
    include_default: bool,
    index_order: bool,
) -> impl Iterator<Item = (FieldDescriptor, Cow<'a, Value>)> + 'a;
```

Two orthogonal knobs, both inherited from prost-reflect (`fields.rs:147-205`):

| `include_default` | Effect |
| --- | --- |
| `false` (the default) | Only fields with `has_field == true`. Matches "what was actually set." Used for proto3 JSON's "skip default fields" behaviour and for textproto's compact form. |
| `true` | All fields the descriptor declares; defaults synthesised. Useful for "show me everything" debug printers. |

| `index_order` | Effect |
| --- | --- |
| `false` (the default) | Iterate in `BTreeMap` order — i.e., field number ascending. The canonical wire order. |
| `true` | Iterate in proto declaration order (`MessageDescriptor::fields_in_index_order()`). Useful for textproto round-trip and source-faithful debug output. |

Convenience `fields()` shortcut: `iter_with_options(false, false)` — populated fields, number-ordered, the most common case.

---

## 7. `Value::is_valid_for_field` validation

Recursive (`vendors/prost-reflect/prost-reflect/src/dynamic/mod.rs:639-670`):

| Field shape | Acceptable `Value` |
| --- | --- |
| singular `Bool` | `Value::Bool` |
| singular `I32`/`Sint32`/`Sfixed32` | `Value::I32` |
| singular `I64`/`Sint64`/`Sfixed64` | `Value::I64` |
| singular `U32`/`Fixed32` | `Value::U32` |
| singular `U64`/`Fixed64` | `Value::U64` |
| singular `F32` | `Value::F32` |
| singular `F64` | `Value::F64` |
| singular `String` | `Value::String` |
| singular `Bytes` | `Value::Bytes` |
| singular `Enum` | `Value::EnumNumber` (any `i32` accepted — open enum semantics) |
| singular `Message` | `Value::Message(m)` where `m.descriptor()` shares the field's expected message descriptor (compared via `Arc::ptr_eq` on `inner`) |
| repeated T | `Value::List(items)`; each item validated against `T` recursively |
| `map<K, V>` | `Value::Map(entries)`; each key validated against `K` (subset of MapKey variants), each value against `V` recursively |

Cross-pool message values (`Value::Message` from a different pool) are **rejected** as a type mismatch. Permissive cross-pool transcoding requires an explicit method (deferred).

---

## 8. `ReflectMessage` extension

```rust
pub trait ReflectMessage: ::buffa::Message {
    fn descriptor(&self) -> MessageDescriptor;

    #[cfg(feature = "dynamic")]
    fn transcode_to_dynamic(&self) -> DynamicMessage
    where
        Self: Sized,
    {
        let descriptor = self.descriptor();
        DynamicMessage::decode(descriptor, self.encode_to_vec().as_slice())
            .expect("self-encoded message must decode")
    }
}
```

Plus `DynamicMessage`'s own `transcode_from` / `transcode_to`:

```rust
impl DynamicMessage {
    /// Merge a typed message into this dynamic message via wire round-trip.
    pub fn transcode_from<T: Message>(&mut self, value: &T) -> Result<(), DecodeError> {
        self.merge(value.encode_to_vec().as_slice())
    }

    /// Convert this dynamic message into a typed value via wire round-trip.
    pub fn transcode_to<T: Message + Default>(&self) -> Result<T, DecodeError> {
        T::decode_from_slice(self.encode_to_vec().as_slice())
    }
}
```

And the special-case `impl ReflectMessage for DynamicMessage` whose `transcode_to_dynamic` returns `self.clone()` rather than the wire round-trip — saves a meaningful amount of work when generic code calls `transcode_to_dynamic()` on something that's already dynamic. Same as prost-reflect (`mod.rs:585-596`).

### Why `transcode_to_dynamic` lives on the trait but `transcode_to/from` live on `DynamicMessage`

- `transcode_to_dynamic(&self) -> DynamicMessage` is generic over the typed source — convenient as a trait method on every typed message.
- `transcode_to::<T>(&self) -> Result<T, _>` is generic over the typed *target* — naturally a method on `DynamicMessage` rather than a trait.

Splitting them this way matches prost-reflect's API and makes generic code uniform: any `T: ReflectMessage` (typed or dynamic) supports `t.transcode_to_dynamic()`.

---

## 9. Module layout

```
crates/buffa-reflect/src/
  lib.rs            # re-exports gated on `dynamic`
  pool.rs           # +default-value parser hook (Phase 1 amend)
  pool_build.rs     # +eager default_value parsing (Phase 1 amend)
  message.rs        # +default_dynamic() factory
  field.rs          # +default_value() / is_default_value() / is_valid_for_field() helpers
  reflect.rs        # +transcode_to_dynamic (cfg "dynamic")
  dynamic/
    mod.rs          # public types: DynamicMessage, Value, MapKey, SetFieldError
    fields.rs       # DynamicMessageFieldSet (BTreeMap<u32, ValueOrUnknown>),
                    # FieldDescriptorLike trait
    message.rs      # encode / decode / merge dispatch
    value.rs        # Value impls (PartialEq, From, accessors, validation)
    unknown.rs      # UnknownField, UnknownFieldSet (mirrors buffa::UnknownFields shape)
    defaults.rs     # default_value parser (called from pool_build at build time)
    iter.rs         # iter_with_options + helpers
```

Module shape mirrors `vendors/prost-reflect/prost-reflect/src/dynamic/` so cross-referencing during implementation is mechanical.

---

## 10. Cycles, recursion, depth limits

- **Cyclic message types** — `Value::Message(DynamicMessage)` heap-allocates inside the parent's `BTreeMap`; arbitrary tree depth at construction has no cost beyond the natural per-node allocation.
- **Decode recursion** — capped at `RECURSION_LIMIT` (100), configurable via `decode_with_options`. Bails with `DecodeError::RecursionLimitExceeded`. Same default as buffa's typed code.
- **Encode recursion** — uncapped. The user constructed the tree.
- **Drop recursion** — for pathological depth (10⁵+) could stack-overflow. Same caveat as `Vec<Box<Vec<...>>>`. If a real workload exposes it, switch to iterative drop. Not in Phase 2a.

---

## 11. `feature = "dynamic"`

```toml
# crates/buffa-reflect/Cargo.toml
[features]
default = ["derive", "dynamic"]
derive = ["dep:buffa-reflect-derive"]
dynamic = []
```

Default-on. Consumers wanting Phase-1-only typed reflection opt out via `default-features = false, features = ["derive"]`. The `dynamic` feature pulls in nothing new beyond what's already available (`bytes`, `BTreeMap`, `HashMap`, `Cow` are already in scope).

---

## 12. Performance characteristics

Expected (benchmarks in a follow-up):

| Operation | Complexity | Notes |
| --- | --- | --- |
| `new(descriptor)` | O(1) | empty `BTreeMap` |
| `get_field_by_name` / `_by_number` | O(log N) for the lookup + O(1) on the descriptor side | descriptor's `by_name` / `by_number` are O(1) HashMaps; the BTreeMap probe is O(log N) where N = populated field count |
| `set_field` (release build) | O(log N) BTreeMap insert | validation is `debug_assert` — no cost in release |
| `decode` | O(B) where B = wire bytes | descriptor field lookups O(1); BTreeMap inserts O(log N); recursion adds nothing beyond per-tag work |
| `encode` | O(N + B) for two passes | `encoded_len` walks once; `encode` walks once |
| `clone` | O(N + total payload) | BTreeMap clone + per-Value clone; `Bytes` clones are O(1) refcount bumps |

prost-reflect benchmarks at ~2–3× typed encode/decode cost. We expect parity since the algorithms are structurally identical and we share buffa's primitives.

---

## 13. Testing & acceptance

Unit tests in `crates/buffa-reflect/src/dynamic/*.rs` (`#[cfg(test)] mod tests`). Integration tests under `crates/buffa-reflect/tests/dynamic.rs`.

Acceptance bar:

- **Byte-equivalence round-trip** on every fixture in `examples/equivalence/proto/`: `buffa_typed.encode_to_vec() == DynamicMessage::decode(d, buffa_typed.encode_to_vec()).encode_to_vec()`.
- **`set` / `get` round-trip** for every `Kind`.
- **`try_set_field` rejection** for a string-into-int32 attempt (returns `SetFieldError::InvalidType` without panicking).
- **Unknown-field preservation**: decode with extra wire data; encode; bytes are still present, interleaved at the right field-number position.
- **Recursion-limit enforcement** — hand-crafted 200-deep wire input fails at default depth, succeeds at `with_recursion_limit(300)`.
- **`transcode_to_dynamic` round-trip** — for every typed fixture, `typed.encode_to_vec() == typed.transcode_to_dynamic().encode_to_vec()`.
- **`DynamicMessage::transcode_to::<User>()` round-trip** — `dyn.transcode_to::<User>()? == user_typed`.
- **`DynamicMessage as ReflectMessage`** — `dyn.transcode_to_dynamic() ≡ dyn.clone()` (verifies the specialisation); `dyn.descriptor() == d`.
- **`Send + Sync`** — extend the existing `const _:` assertion in `pool.rs` to cover `DynamicMessage`.
- **Feature opt-out** — `cargo test --workspace --no-default-features --features=derive` clean.
- **Conformance** — target **100 % pass rate** on the protobuf binary-format conformance suite. (`vendors/prost-reflect/prost-reflect-conformance-tests/failure_list.txt` is empty in prost-reflect — they pass everything. We aim for the same.)

---

## 14. Interaction with Phase 1

- **Backward compatible.** All Phase 1 APIs unchanged.
- **Phase 1 amend** for default-value parsing: `FieldEntry` gains a `default: Value` field; `pool_build.rs` parses `FieldDescriptorProto::default_value` once at pool-build time (eager, not lazy). New variant `DescriptorError::InvalidDefaultValue { field, value, message }`. ~150 LOC, isolated; documented as an additive change in the Phase 1 design.
- **`MessageDescriptor::default_dynamic()` factory** — convenience method; `descriptor.default_dynamic() == DynamicMessage::new(descriptor)`.
- **Pool snapshot semantics** unchanged — a `DynamicMessage` constructed against a pool clone retains its `Arc` snapshot; mutations via `add_file_descriptor_set` on a clone never affect the dynamic message's view.

---

## 15. Decisions revised after the audit

For the spec-archaeology trail:

1. **Storage**: `Vec<Option<Value>>` → `BTreeMap<u32, ValueOrUnknown>`. Driving reason: needed a single iteration order over interleaved known and unknown fields. (§2)
2. **Mutation API**: Result-only → dual `set_field` (debug-assert) / `try_set_field` (Result). Driving reason: ergonomic + zero-cost in release. (§5)
3. **Iteration**: single `populated_fields()` → `iter_with_options(include_default, index_order)`. Driving reason: textproto needs declaration order; JSON needs default-omitted; canonical wire wants number order. (§6)
4. **Default values**: lazy parse on get → eager parse at pool-build time. Driving reason: report invalid defaults as pool errors, not surprise crashes at first read. (§2 last paragraph)
5. **`ReflectMessage` symmetric extension**: `from_dynamic` on the trait → `transcode_to`/`transcode_from` on `DynamicMessage`. Driving reason: typed-target methods belong on the dynamic side; only typed-source belongs on the trait. (§8)
6. **Conformance target**: ~95 % → 100 %. Driving reason: prost-reflect already achieves this on the binary-format suite; lower bar is unjustified.
7. **`FieldDescriptorLike` trait** added so the storage layer doesn't need to be re-implemented for extensions in a future phase. (§2)
8. **`Value::is_valid_for_field`** added as a public method. The recursive validation logic was missing from the earlier draft. (§7)

---

## 16. Out of scope, captured for follow-ups

- **`DynamicMessage::merge` from another `DynamicMessage`** (pure-Rust merge without re-encoding) — convenience.
- **Extension reading via `FieldDescriptorLike`** — the trait is plumbed but only `FieldDescriptor` implements it for Phase 2a. Adding `ExtensionDescriptor` is purely additive.
- **`Value::try_into_*` / `From` macros** — accessor sugar; deferred.
- **`DynamicMessage::clear_all()` / `Default` impl on DynamicMessage** — trivial follow-ups; not required for Phase 2a.
- **JSON / textproto** — separate specs, depend on this one.
- **gRPC reflection** — Phase 2d, depends only on Phase 1.
- **View reflection** — Phase 2e, depends only on Phase 1.
