# Phase 2b — Proto3 JSON via serde

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). **Depends on** [`DynamicMessage`](./buffa-reflect-dynamic-design.md). Cannot ship before Phase 2a.

This document specifies the proto3 canonical JSON mapping for `DynamicMessage`, the public configuration surface (`SerializeOptions` / `DeserializeOptions`), and the well-known-type special-cases.

---

## 1. Goals

- `serde::Serialize` impl on `DynamicMessage` producing **canonical proto3 JSON** ([reference](https://protobuf.dev/programming-guides/proto3/#json)).
- `serde::Deserialize` impl on `DynamicMessage` accepting that same JSON.
- Round-trip identity for every field shape covered by the equivalence-suite fixture.
- Configurable: emit-default behavior, integer-as-string mode, unknown-field strictness, fully-qualified Any.
- Mirror prost-reflect's `SerializeOptions` / `DeserializeOptions` API names where reasonable so migration docs are short.
- Support all 12 well-known types (Timestamp, Duration, Any, Empty, Struct, Value, ListValue, FieldMask, NullValue, BoolValue / StringValue / NumberValue / etc. — see §4).

## 2. Non-goals

- A typed `serde` impl for individual `User: ReflectMessage` (serialize a typed struct via `Serialize`). Out of scope; consumers can `.transcode_to_dynamic()` first.
- Streaming JSON parser. `serde_json::from_reader` works; we don't add anything beyond.
- Non-canonical JSON variants (e.g., proto2-style with int64 as numbers without quoting). The canonical form is what interoperates.

---

## 3. Public surface

```rust
// crates/buffa-reflect/src/json.rs
#[cfg(all(feature = "dynamic", feature = "json"))]
pub use crate::json::{DeserializeOptions, JsonError, SerializeOptions, deserialize, serialize};

#[derive(Clone, Debug)]
pub struct SerializeOptions {
    /// Emit fields equal to their proto-default ("zero") values.  Default: false.
    pub emit_default_fields: bool,
    /// Use enum number instead of name when serializing.  Default: false (use name).
    pub emit_enum_values_as_numbers: bool,
    /// Pretty-print output.  Default: false.
    pub pretty: bool,
    /// Type registry used to resolve `google.protobuf.Any` payload types.
    /// Default: derived from the dynamic message's parent pool.
    pub type_registry: Option<DescriptorPool>,
    // ... other knobs added as needed
}

#[derive(Clone, Debug)]
pub struct DeserializeOptions {
    /// Reject unknown fields rather than silently dropping them.
    /// Default: true (matches the proto3 spec's `ignore_unknown_fields = false`).
    pub deny_unknown_fields: bool,
    /// Type registry for `Any` payloads.
    pub type_registry: Option<DescriptorPool>,
}

pub fn serialize<S: serde::Serializer>(
    msg: &DynamicMessage,
    serializer: S,
    opts: &SerializeOptions,
) -> Result<S::Ok, S::Error>;

pub fn deserialize<'de, D: serde::Deserializer<'de>>(
    descriptor: MessageDescriptor,
    deserializer: D,
    opts: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error>;

// Convenience trait impls (use default options):
impl serde::Serialize for DynamicMessage { /* ... */ }
// Note: serde::Deserialize is not directly implementable for DynamicMessage
// because it needs a descriptor. We expose `deserialize_with_pool` helpers:
impl DynamicMessage {
    pub fn from_json_with_descriptor(
        descriptor: MessageDescriptor,
        json: &str,
    ) -> Result<Self, JsonError>;
    pub fn from_json_value_with_descriptor(
        descriptor: MessageDescriptor,
        json: &serde_json::Value,
    ) -> Result<Self, JsonError>;
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JsonError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("`{full_name}` JSON: {message}")]
    Schema { full_name: String, message: String },
    #[error("Any payload type `{type_url}` not found in registry")]
    UnknownAnyType { type_url: String },
}
```

## 4. Field mappings

The proto3 JSON spec is precise. Summary table — each row is a field's `KindRef`/Cardinality combination → its JSON shape:

| Field shape | JSON shape | Notes |
| --- | --- | --- |
| `bool` | JSON boolean | direct |
| `int32` / `sint32` / `sfixed32` / `uint32` / `fixed32` | JSON number | within JS-safe int range |
| `int64` / `sint64` / `sfixed64` / `uint64` / `fixed64` | JSON **string** | required to avoid JS `Number` precision loss |
| `float` / `double` | JSON number, with `"NaN"` / `"Infinity"` / `"-Infinity"` accepted as strings | |
| `string` | JSON string | UTF-8 |
| `bytes` | JSON string, **base64**-encoded | use `base64` crate |
| `enum` | JSON **string** with the variant name (e.g., `"ROLE_ADMIN"`); accept number on parse | configurable via `emit_enum_values_as_numbers` |
| `message` | JSON object | recursive |
| `repeated T` | JSON array of T's mapping | empty arrays omitted unless `emit_default_fields` |
| `map<K, V>` | JSON object; keys stringified | int/bool keys serialised as JSON strings |

### Field naming

Field names use **JSON name** (lowerCamelCase) — already provided by Phase 1's `FieldDescriptor::json_name`. `deny_unknown_fields = false` accepts both JSON name and proto name on deserialization (matches the spec).

### Default-omission semantics

By default, fields equal to their proto3 zero value are **omitted** from output. With `emit_default_fields: true`, every field is emitted (matches proto3 JSON's `including_default_value_fields` flag). Repeated/map empty values: omitted by default; emitted as `[]` / `{}` when the flag is on.

For oneofs: only the active member is emitted. The synthetic-oneof for proto3 `optional` is treated as a regular oneof for emission (the "wrapper" oneof itself is not visible in JSON; the inner field name appears directly).

---

## 5. Well-known type special-cases

These types have JSON mappings that diverge from "render the message as an object":

| Type | JSON mapping | Notes |
| --- | --- | --- |
| `google.protobuf.Empty` | `{}` | always |
| `google.protobuf.Timestamp` | RFC 3339 string, e.g. `"2024-01-01T00:00:00Z"` | nanoseconds preserved, `Z` suffix or `+HH:MM` |
| `google.protobuf.Duration` | string with `"s"` suffix, e.g. `"3.000000001s"`, `"-1.5s"` | seconds field can be negative |
| `google.protobuf.FieldMask` | comma-separated lowerCamelCase paths, e.g. `"f.b,f.c"` | |
| `google.protobuf.Struct` | JSON object; fields directly the struct's `fields` map | |
| `google.protobuf.Value` | JSON value of any type — null / bool / number / string / array / object | unwrap `kind` oneof |
| `google.protobuf.ListValue` | JSON array | unwrap `values` field |
| `google.protobuf.NullValue` | `null` | |
| `google.protobuf.{Bool,String,Bytes,Int32,Int64,UInt32,UInt64,Float,Double}Value` | underlying scalar | wrappers are unwrapped |
| `google.protobuf.Any` | JSON object with `"@type": "type.googleapis.com/<full.name>"` and the unpacked fields inline | requires the `type_registry` knob to find the inner descriptor |

The detection lookup is by `descriptor.full_name() == "google.protobuf.Timestamp"` etc. — string compare on the FQN. We hard-code the table.

If `type_registry` is `None`, fall back to the dynamic message's `parent_pool()`. If the inner Any type isn't there, error with `JsonError::UnknownAnyType`.

---

## 6. Implementation strategy

### Serialization

```rust
fn serialize_message<S: Serializer>(
    msg: &DynamicMessage,
    s: S,
    opts: &SerializeOptions,
) -> Result<S::Ok, S::Error> {
    if let Some(special) = wkt_serializer(msg.descriptor().full_name()) {
        return special(msg, s, opts);
    }
    let mut state = s.serialize_map(None)?;
    for (field, value) in msg.populated_fields() {
        if !opts.emit_default_fields && is_default(&value, &field) {
            continue;
        }
        state.serialize_entry(field.json_name(), &SerializeValue { value, field, opts })?;
    }
    state.end()
}
```

`SerializeValue` is a wrapper struct with a hand-written `Serialize` impl that dispatches by Kind to the right scalar/list/map serialization.

### Deserialization

```rust
fn deserialize_message<'de, D: Deserializer<'de>>(
    descriptor: &MessageDescriptor,
    d: D,
    opts: &DeserializeOptions,
) -> Result<DynamicMessage, D::Error> {
    if let Some(special) = wkt_deserializer(descriptor.full_name()) {
        return special(descriptor, d, opts);
    }
    d.deserialize_map(MessageVisitor { descriptor, opts })
}
```

`MessageVisitor` implements `serde::de::Visitor`. For each `(key, value)` pair: look up the field by JSON name, then by proto name; if unknown and `deny_unknown_fields`: error. Otherwise dispatch by Kind through a `ValueVisitor`.

### Reuse of buffa's helpers

`buffa::json_helpers` already has parsers/encoders for the canonical scalar mappings (int64-as-string, bytes-as-base64, etc.) used by the typed code path. **Lift directly** rather than reimplementing. The integration is one wrapper that adapts the buffa helper functions to operate over a `Value` instead of a typed scalar.

### Special cases internal to serialization

- Maps with non-string keys (`map<int32, string>`): keys serialised as `serializer.serialize_str(&key.to_string())` per the spec.
- `NaN`/`Infinity`/`-Infinity`: emitted as JSON strings (`"NaN"`, `"Infinity"`, `"-Infinity"`); **not** valid JSON numbers.
- Negative-zero floats: emitted as `-0`.

---

## 7. Error model

JsonError vs serde_json::Error: serde_json::Error covers the syntactic JSON layer; JsonError wraps schema-level mismatches. The split:
- Bad UTF-8, mismatched braces → `serde_json::Error` → wrapped in `JsonError::Json`.
- Field not in descriptor + `deny_unknown_fields` → `JsonError::Schema { ... }`.
- Enum string doesn't match a variant → `JsonError::Schema`.
- `Any` payload type not in registry → `JsonError::UnknownAnyType`.

Parse errors carry the offending JSON path via serde_json's machinery; we do not reimplement that.

---

## 8. Module layout

```
crates/buffa-reflect/src/
  json/
    mod.rs           # public surface, re-exports
    options.rs       # SerializeOptions, DeserializeOptions
    serialize.rs     # Serializer dispatch, ValueWrapper, populated-field walk
    deserialize.rs   # Visitor types, key resolution, value dispatch
    wkt.rs           # well-known-type detection + dispatch tables
    helpers.rs       # base64, RFC3339, varint-as-string adapters (lift from buffa)
    error.rs         # JsonError
```

Feature gating: `#[cfg(feature = "json")]`. The runtime crate adds `[features] json = ["dynamic", "dep:base64", "dep:serde", "dep:serde_json"]`. `dynamic` is required (Value is the bridge).

---

## 9. Testing

- Round-trip: every field shape from the equivalence fixture serialises to JSON and deserialises back to a `PartialEq`-equal `DynamicMessage`.
- Reference vectors: hand-crafted JSON strings (small) for each WKT, verifying our serializer emits exactly those bytes.
- Conformance suite: integrate the JSON conformance tests from the protobuf conformance runner. `known_failures.txt` for documented gaps.
- `serde_json::to_string` of a `&DynamicMessage` works without explicit `serialize` call (default-options `Serialize` impl).

---

## 10. Risks

| Risk | Mitigation |
| --- | --- |
| WKT JSON mappings have edge cases (e.g., `Duration`'s seconds≥0/<0 sign rules) that diverge between protoc versions. | Reference protoc 33 as the spec; pin the conformance runner image. |
| `Any.@type` URL parsing differs across implementations. | Accept all forms (`type.googleapis.com/X`, bare `X`); emit canonical form. |
| `NaN` / `Infinity` accepted by JS `JSON.parse` but not by RFC 8259. | Emit as JSON strings (per spec). Document. |
| Default-omission interacting badly with `oneof` semantics (a oneof field set to its zero value should still emit, because *presence* is meaningful). | Special-case oneof members: always emit when active, regardless of value. |

---

## 11. Acceptance for Phase 2b

- Every WKT in §4 has a passing round-trip test.
- Conformance suite passes ≥ 95 % of the JSON tests; the remainder live in `known_failures_json.txt` with one-line justifications.
- `cargo test --workspace --features json` clean.
