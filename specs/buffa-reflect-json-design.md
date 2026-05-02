# Phase 2b — Proto3 JSON via serde

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). **Depends on** [`DynamicMessage`](./buffa-reflect-dynamic-design.md). Cannot ship before Phase 2a.

This document specifies the proto3 canonical JSON mapping for `DynamicMessage`, the public configuration surface (`SerializeOptions` / `DeserializeOptions`), and the well-known-type special-cases.

The design closely follows prost-reflect (`vendors/prost-reflect/prost-reflect/src/dynamic/serde/`) — including the **`MessageDescriptor` is itself a `DeserializeSeed`** idiom, the exact field names on the options structs, and the WKT dispatch table of 16 hard-coded type names.

---

## 1. Goals

- `serde::Serialize` impl on `DynamicMessage` producing **canonical proto3 JSON** ([reference](https://protobuf.dev/programming-guides/proto3/#json)).
- `serde::de::DeserializeSeed` impl on `MessageDescriptor` so the descriptor *is* the seed; users write `descriptor.deserialize(&mut json_deserializer)`.
- Round-trip identity for every field shape covered by `examples/equivalence/proto/`.
- Configurable: `stringify_64_bit_integers`, `use_enum_numbers`, `use_proto_field_name`, `skip_default_fields` on the serialize side; `deny_unknown_fields` on the deserialize side. (Field names match prost-reflect exactly so migration docs are short.)
- Support all 16 well-known types currently in `google.protobuf` (Any, Timestamp, Duration, Struct, Value, ListValue, FieldMask, Empty, NullValue, plus the eight `*Value` wrapper types).

## 2. Non-goals

- A `serde` impl for typed `T: ReflectMessage` (serialize a typed struct directly). Out of scope; consumers `.transcode_to_dynamic()` first.
- Streaming JSON parser. `serde_json::from_reader` works; we don't add anything beyond.
- Non-canonical JSON variants (e.g., proto2-style with int64 as numbers without quoting).
- A separate type registry knob for `Any` payload resolution. The dynamic message's `parent_pool()` is the source of truth — same as prost-reflect (`vendors/prost-reflect/prost-reflect/src/dynamic/serde/ser/wkt.rs:64-68`).

---

## 3. Public surface

```rust
// crates/buffa-reflect/src/dynamic/serde/mod.rs
#[cfg(all(feature = "dynamic", feature = "serde"))]
pub use crate::dynamic::serde::{DeserializeOptions, SerializeOptions};

#[derive(Debug, Clone)]
pub struct SerializeOptions {
    /// Encode 64-bit integers as JSON strings rather than numbers, per
    /// proto3 JSON spec. Default: `true`.
    stringify_64_bit_integers: bool,
    /// Encode enum values as their numeric value rather than the
    /// declared name. Default: `false`.
    use_enum_numbers: bool,
    /// Use the proto field name (snake_case) instead of the JSON name
    /// (lowerCamelCase). Default: `false`.
    use_proto_field_name: bool,
    /// Omit fields whose value equals the proto default. Default: `true`.
    skip_default_fields: bool,
}

impl SerializeOptions {
    pub fn new() -> Self;
    #[must_use] pub fn stringify_64_bit_integers(self, b: bool) -> Self;
    #[must_use] pub fn use_enum_numbers(self, b: bool) -> Self;
    #[must_use] pub fn use_proto_field_name(self, b: bool) -> Self;
    #[must_use] pub fn skip_default_fields(self, b: bool) -> Self;
}

#[derive(Debug, Clone)]
pub struct DeserializeOptions {
    /// Reject unknown fields rather than silently dropping them.
    /// Default: `false` (matches the proto3 spec's
    /// `ignore_unknown_fields = true` default).
    deny_unknown_fields: bool,
}

impl DeserializeOptions {
    pub fn new() -> Self;
    #[must_use] pub fn deny_unknown_fields(self, b: bool) -> Self;
}
```

```rust
// The trait impls — the ergonomic core.
impl Serialize for DynamicMessage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize_with_options(serializer, &SerializeOptions::default())
    }
}

impl<'de> DeserializeSeed<'de> for MessageDescriptor {
    type Value = DynamicMessage;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        DynamicMessage::deserialize(self, deserializer)
    }
}

impl DynamicMessage {
    pub fn serialize_with_options<S: Serializer>(
        &self,
        serializer: S,
        options: &SerializeOptions,
    ) -> Result<S::Ok, S::Error>;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        descriptor: MessageDescriptor,
        deserializer: D,
    ) -> Result<Self, D::Error>;

    pub fn deserialize_with_options<'de, D: Deserializer<'de>>(
        descriptor: MessageDescriptor,
        deserializer: D,
        options: &DeserializeOptions,
    ) -> Result<Self, D::Error>;
}
```

### Why `MessageDescriptor: DeserializeSeed`

`serde::Deserialize::deserialize` doesn't take state — it only takes a `Deserializer`. Plain `Deserialize` cannot know which message type to construct. `DeserializeSeed` is serde's stateful counterpart: the seed *is* the state. Making `MessageDescriptor` the seed means the API reads naturally:

```rust
let json = r#"{ "foo": 150 }"#;
let mut deserializer = serde_json::de::Deserializer::from_str(json);
let dynamic_message = message_descriptor.deserialize(&mut deserializer)?;
```

This is the API prost-reflect ships (`vendors/prost-reflect/prost-reflect/src/dynamic/serde/mod.rs:60-87`). It avoids the awkward `from_json_with_pool(pool, json)` shape my earlier draft proposed.

### `JsonError` is **not** introduced as a new type

prost-reflect surfaces JSON errors as `serde_json::Error` directly — schema-level mismatches end up as `serde::de::Error::custom(...)` strings inside the serde error chain. Adding a parallel `JsonError` enum costs more than it gives:
- serde's error type already carries line/column info.
- The error message is the only useful thing for end users; the structured variants would mostly carry the same string back.

Phase 2b: pass through `serde_json::Error` (or whatever serde driver the user picks). Schema problems use `Error::custom("at field `acme.User.id`: expected string, got bool")`-style messages. Identical to prost-reflect.

---

## 4. Field mappings

The proto3 JSON spec is precise. Summary table:

| Field shape | JSON shape | Notes |
| --- | --- | --- |
| `bool` | JSON boolean | direct |
| `int32` / `sint32` / `sfixed32` / `uint32` / `fixed32` | JSON number | within JS-safe int range |
| `int64` / `sint64` / `sfixed64` / `uint64` / `fixed64` | JSON **string** by default (configurable via `stringify_64_bit_integers = false`) | required by spec for portability |
| `float` / `double` | JSON number; `"NaN"` / `"Infinity"` / `"-Infinity"` accepted as strings; non-finite values must serialize to those strings | |
| `string` | JSON string | UTF-8 |
| `bytes` | JSON string, **standard base64** with padding | spec also accepts URL-safe; we emit standard, accept both |
| `enum` | JSON **string** with the variant name (e.g., `"ROLE_ADMIN"`); unknown numbers serialize as numbers; configurable to numbers via `use_enum_numbers` | |
| `message` | JSON object | recursive |
| `repeated T` | JSON array of T's mapping; empty arrays omitted unless `!skip_default_fields` | |
| `map<K, V>` | JSON object; non-string keys stringified | |

### Field naming

By default, JSON-name (lowerCamelCase) — already provided by Phase 1's `FieldDescriptor::json_name`. With `use_proto_field_name = true`, the proto name (snake_case) is used. On deserialize, **both** are accepted regardless of the option (matches the spec's "either form" rule).

### Default-omission semantics

By default (`skip_default_fields = true`), fields equal to their proto3 zero value are **omitted**. With `skip_default_fields = false`, every field is emitted (proto3 JSON's `including_default_value_fields` flag). Repeated/map empty values: omitted by default; emitted as `[]` / `{}` when the flag is off.

For oneofs: only the active member is emitted. Critically, **an active oneof member is always emitted, even if its value is the proto zero** — presence is meaningful. Same special-case as prost-reflect.

For proto3 `optional` synthetic-oneof members: same rule; if presence is set, emit even at zero.

---

## 5. Well-known type special-cases

Hard-coded dispatch table by full_name (`vendors/prost-reflect/prost-reflect/src/dynamic/serde/ser/wkt.rs:23-49`):

| Type | JSON mapping |
| --- | --- |
| `google.protobuf.Empty` | `{}` |
| `google.protobuf.Timestamp` | RFC 3339 string with `Z` suffix or numeric offset, e.g. `"2024-01-01T00:00:00.123456789Z"` |
| `google.protobuf.Duration` | string with `s` suffix, e.g. `"3s"`, `"3.000000001s"`, `"-1.5s"` |
| `google.protobuf.FieldMask` | comma-separated lowerCamelCase paths, e.g. `"f.b,f.c"` |
| `google.protobuf.Struct` | JSON object; `fields` map directly |
| `google.protobuf.Value` | JSON value of any type — null / bool / number / string / array / object — by unwrapping the `kind` oneof |
| `google.protobuf.ListValue` | JSON array; `values` field directly |
| `google.protobuf.NullValue` | JSON `null` |
| `google.protobuf.{Bool,String,Bytes,Int32,Int64,UInt32,UInt64,Float,Double}Value` | the underlying scalar (wrappers unwrapped) |
| `google.protobuf.Any` | JSON object with `"@type": "type.googleapis.com/<full.name>"` plus the unpacked fields inline (or `"value"` for non-message payloads) |

That's exactly the 16 entries prost-reflect supports. Detection is a `match` on `full_name()` returning a function pointer; encoding/decoding then dispatches to the WKT-specific handler.

### `Any` resolution

When serializing, parse `type_url` into the message FQN, look up via `dyn_msg.parent_pool().get_message_by_name(...)`. If absent, error via `serde::ser::Error::custom("Any payload type ... not found in pool")`. **No** separate type-registry knob — same as prost-reflect.

If the `Any` payload type itself has a special JSON mapping (e.g., `Any` containing a `Timestamp`), we apply the WKT mapping to the inner message and emit `{"@type": "...", "value": "<rfc3339>"}` — note `value` not inline. Same special-case as prost-reflect (and the proto3 JSON spec).

### Non-finite floats inside `Value`

`google.protobuf.Value` claims to represent any JSON value, but JSON itself can't represent NaN or Infinity. Per the spec, a `Value::number_value` set to non-finite is a serialization error. We surface that via `serde::ser::Error::custom("cannot serialize non-finite double in google.protobuf.Value")` — same wording as prost-reflect (`vendors/prost-reflect/prost-reflect/src/dynamic/serde/ser/wkt.rs:354`).

---

## 6. Implementation strategy

### Module layout

```
crates/buffa-reflect/src/dynamic/serde/
  mod.rs            # public re-exports, SerializeOptions, DeserializeOptions
  case.rs           # snake_case ↔ lowerCamelCase conversions
  ser/
    mod.rs          # serialize_message dispatch
    field.rs        # per-Kind serializers (scalars, list, map)
    wkt.rs          # WKT dispatch table + per-WKT serializers
  de/
    mod.rs          # MessageVisitor + per-Kind deserialization
    wkt.rs          # WKT-specific deserializers
```

### Reuse of buffa's helpers

`buffa::json_helpers` already implements the canonical scalar mappings (int64-as-string, bytes-as-base64, RFC 3339 for Timestamp, etc.) used by the typed code path when `generate_json` is enabled. **Lift those functions wholesale** rather than re-implementing. The integration is one wrapper that adapts the buffa helpers to operate over a `Value` instead of a typed scalar — cuts the spec-conformance risk meaningfully.

The handful of WKT helpers that buffa doesn't expose (Struct, Value, ListValue, Any) need to be written for Phase 2b; they're each ~20 lines and the proto3 JSON spec specifies them precisely.

### `serde-value` dependency

prost-reflect uses `serde-value` (`vendors/prost-reflect/prost-reflect/Cargo.toml:25`) to buffer JSON values when the deserializer needs to peek (e.g., `Any` deserialization needs to look at `@type` before knowing how to interpret the rest of the object). We adopt the same dep.

---

## 7. Cargo features

```toml
# crates/buffa-reflect/Cargo.toml
[features]
default = ["derive", "dynamic"]
dynamic = []
serde = ["dynamic", "dep:serde", "dep:base64", "dep:serde-value"]
```

Feature name is `serde` (not `json`) — match prost-reflect (`vendors/prost-reflect/prost-reflect/Cargo.toml:25`). Rationale: the impl is, literally, serde::Serialize / serde::Deserialize. Calling it `json` would be misleading — a future YAML or CBOR consumer would benefit from the same impl.

---

## 8. Testing

- **Round-trip per fixture** — every shape from `examples/equivalence/proto/` round-trips through serialize → deserialize → equal `DynamicMessage`.
- **WKT vectors** — hand-crafted JSON strings for each WKT, asserting our serializer emits exactly those bytes.
- **Conformance suite** — extend `crates/buffa-reflect-conformance-tests` with the JSON harness. Target **100 % pass** (prost-reflect's `failure_list.txt` is empty for JSON conformance — they pass everything).
- **Default-options serialization** — `serde_json::to_string(&dyn_msg)` works with the trait impl, no explicit `serialize_with_options` call.
- **Non-finite double in Value** — error case per §5.

---

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| WKT JSON mappings have edge cases (Duration sign rules, Timestamp sub-second precision) that diverge between protoc versions. | Pin the conformance image; lift implementations from `buffa::json_helpers` where they exist. |
| `Any.@type` URL parsing varies (`type.googleapis.com/X` vs bare `X`). | Accept both on parse; emit the canonical `type.googleapis.com/...` form. |
| `serde_json` accepts `NaN`/`Infinity` (extension over RFC 8259) but other JSON parsers reject them. | Emit as JSON strings (`"NaN"`, `"Infinity"`) for non-`Value` fields. For `google.protobuf.Value`, error on non-finite per spec. |
| Map keys with non-string types serialize as JSON strings, but deserialization needs to round-trip parse them. | Per spec: emit string-of-the-key; on parse, attempt to parse the string back to the key's native type. |
| Active-oneof-emit-at-default-rule subtle to get right for the synthetic proto3-optional case. | Treat synthetic oneof identically to a real oneof for emission; the synthetic-ness is invisible at the JSON layer. Test fixture covers both. |

---

## 10. Acceptance for Phase 2b

- Every WKT in §5 has a passing round-trip test.
- Conformance suite passes 100 % of JSON tests; any documented failures listed in `crates/buffa-reflect-conformance-tests/failure_list_json.txt` with one-line justifications.
- `cargo test --workspace --features=serde` clean.
- `cargo build --workspace --no-default-features --features="derive,dynamic"` clean (proves `serde` is a clean opt-in).
