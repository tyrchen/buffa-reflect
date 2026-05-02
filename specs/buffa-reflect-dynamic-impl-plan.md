# Phase 2a — `DynamicMessage` Implementation Plan

Pre-reads: [phase-2 PRD](./buffa-reflect-phase2-prd.md), [dynamic design](./buffa-reflect-dynamic-design.md).

This plan covers Phase 2a only. JSON / textproto / gRPC / view reflection have their own plans, all of which transitively depend on this one.

---

## 1. Milestones

### D1 — Module skeleton, `Value`, `MapKey`, `FieldDescriptorLike` (1.5 days)

- New `crates/buffa-reflect/src/dynamic/{mod,value,fields,unknown}.rs` — empty types, `Cargo.toml` `[features] dynamic = []` flag default-on.
- `Value` enum (matches prost-reflect's variant set: `Bool`, `I32`, `I64`, `U32`, `U64`, `F32`, `F64`, `String`, `Bytes`, `EnumNumber`, `Message`, `List(Vec<Value>)`, `Map(HashMap<MapKey, Value>)`).
- `MapKey` enum (subset of Value variants legal for map keys: `Bool`, `I32`, `I64`, `U32`, `U64`, `String`).
- `Value::is_valid_for_field` — recursive validation per design §7.
- `FieldDescriptorLike` trait declared, `FieldDescriptor` impl provided. Extension impl deferred.
- `DynamicMessageFieldSet` struct with `BTreeMap<u32, ValueOrUnknown>` storage; `ValueOrUnknown::Taken` sentinel. Per-method shape mirrors `vendors/prost-reflect/prost-reflect/src/dynamic/fields.rs`.
- `DynamicMessage::new(descriptor)` constructor.
- Smoke tests: empty construction; `Value::is_valid_for_field` for each Kind.

### D2 — Accessors: get/set/has/clear (2 days)

- `set_field` (debug-assert validation, panics in debug) + `try_set_field` (Result). Same pattern for `_by_name` / `_by_number` variants. Per design §5.
- `get_field` returns `Cow<'_, Value>`; defaults synthesised on miss.
- `get_field_mut` calls `clear_oneof_fields` before returning a mutable handle.
- Oneof set / clear / get_mut invariants (mirror `prost-reflect/src/dynamic/fields.rs:107-114`).
- Tests:
  - `set` then `get` round-trips for every Kind.
  - `try_set_field` rejection: a string-into-int32 attempt returns `SetFieldError::InvalidType` (no panic in release; `debug_assert!` panic in debug).
  - Cross-pool `Value::Message` set rejected with `SetFieldError::InvalidType`.
  - Oneof: set field A, then field B of the same oneof → `has(A) == false`, `has(B) == true`. `clear(B)` → both unset.
  - `clear` of an absent field is a no-op.
  - Nested message: build a multi-level tree with `set_field` only.

### D2.5 — Eager default-value parser at pool-build time (1 day, Phase 1 amend)

- `crates/buffa-reflect/src/pool_build.rs` — parse `FieldDescriptorProto::default_value` once at pool build, store on `FieldEntry`.
- Parser handles: signed/unsigned int (decimal/octal/hex), floats (incl. `inf`/`-inf`/`nan`), `true`/`false`, C-escaped strings, byte literals, enum-by-name. Mirrors `vendors/prost-reflect/prost-reflect/src/descriptor/build/resolve.rs:627-698`.
- New variant `DescriptorError::InvalidDefaultValue { field, value, message }`.
- `DescriptorPool::decode` accumulates all such errors and returns them aggregated (not first-fail) — better diagnostics.
- Tests: each scalar Kind with a representative `default_value` string; one explicit-failure case for each malformed input class.

### D3 — Wire decode (2 days)

- `dynamic/message.rs`: `merge<B: Buf>` loop, dispatching by `Kind` and `cardinality` over the decoded `Tag`.
- Recursion-limit threading via `DecodeOptions` (default `RECURSION_LIMIT`).
- Packed-vs-unpacked dual reading for repeated scalars: dispatch by *observed* wire type, not descriptor's `is_packed()`.
- Unknown-field preservation via `decode_unknown_field` into `UnknownFieldSet` keyed by the original number; multiple tags with the same number accumulate in insertion order.
- Forward-compat enum decoding: unknown enum numbers stored as `Value::EnumNumber(raw)`.
- Tests:
  - For every fixture in `examples/equivalence/proto/`, construct the typed message via `buffa`, encode, then `DynamicMessage::decode` against the same descriptor — assert non-error and that `fields().count()` matches.
  - Recursion-limit guard: hand-crafted 200-deep wire input fails with `RecursionLimitExceeded`.
  - Unknown-field round-trip: append a fake tag with a varint payload; decode; observe the bytes are preserved at the right number-position on re-encode.
  - Decode unknown enum number; encode; observe round-trip.

### D4 — Wire encode (2 days)

- `dynamic/message.rs`: `encoded_len` + `encode<B: BufMut>`, two-pass.
- Per-Kind dispatch (single big match per Kind × cardinality, like `prost-reflect/src/dynamic/message.rs:93-244`).
- Packed encoding emission for `is_packed()` repeated scalars; checked per encode (no caching).
- Map field encoding: synthesise `Entry { K key = 1; V value = 2; }` directly without instantiating an entry `DynamicMessage`. Sort entries by `MapKey` order before emit.
- Iteration order: BTreeMap natural order (field number ascending). Known and unknown fields interleave at their numbers.
- Tests:
  - Set-then-encode produces bytes that decode back to a `PartialEq`-equal `DynamicMessage`.
  - **Byte-equivalence**: for every fixture, `buffa_typed.encode_to_vec() == DynamicMessage::decode(d, buffa_typed.encode_to_vec()).encode_to_vec()`.
  - Map encoding order is deterministic across runs.
  - Packed scalars round-trip in both packed and unpacked encodings.

### D5 — `ReflectMessage::transcode_to_dynamic` + `DynamicMessage::transcode_to/from` (1 day)

- One trait method on `ReflectMessage` (`transcode_to_dynamic`), plus `transcode_from`/`transcode_to::<T>` on `DynamicMessage`. All cfg-gated on `dynamic`.
- `impl ReflectMessage for DynamicMessage` with `transcode_to_dynamic` short-circuiting to `self.clone()` (matches `prost-reflect/src/dynamic/mod.rs:585-596`).
- Tests (in the existing `tests/derive.rs`):
  - For every UserBytesForm / UserPoolForm fixture: `typed.transcode_to_dynamic().get_field_by_name("...")` matches `typed.field`.
  - `dyn.transcode_to::<User>() == user_typed`.
  - `(typed_user).transcode_to_dynamic().transcode_to::<User>() == typed_user`.
  - `dyn.transcode_to_dynamic() ≡ dyn.clone()` (specialisation correctness).
- Tests (in the existing `tests/derive.rs` file; the derive emits the trait impl):
  - For every UserBytesForm / UserPoolForm fixture: `typed.transcode_to_dynamic().get_field_by_name("...")` matches `typed.field`.
  - `User::from_dynamic(&typed.transcode_to_dynamic()) == typed`.
  - Cross-type `from_dynamic` with mismatched descriptor returns `DynamicError::TypeMismatch`.

### D6 — Equivalence test extension (1 day)

- `examples/equivalence/tests/equivalence.rs`: extend with `dynamic_round_trips`. For each message in the fixture, decode via prost-reflect's `DynamicMessage`, decode via buffa-reflect's `DynamicMessage`, assert structural equivalence (same `populated_fields` count and same `Value` per field after canonical projection).
- The leaf workspace continues to gate on protoc presence.

### D7 — Conformance harness scaffold (1–2 days)

- New crate (in workspace) `buffa-reflect-conformance-tests` modeled on `vendors/prost-reflect/prost-reflect-conformance-tests/`.
- Reads test cases from the protobuf conformance runner (Docker-driven), encodes/decodes via `DynamicMessage`, returns results.
- `known_failures.txt` capturing the binary-format failures we accept (proto2 `required` semantics is the most likely set).
- Wire only the binary-format conformance for Phase 2a; JSON / text formats land with their respective phases.

### D8 — Polish & docs (1 day)

- `crates/buffa-reflect/src/dynamic/mod.rs` carries a `# Examples` doc block with the full transcode loop.
- `cargo doc --workspace --no-deps` clean.
- Add the "default-features include `dynamic`" note to the root `README.md`.
- `make verify` clean.

Total: **~11–14 working days** for one contributor (the +1 over the original estimate is the D2.5 default-value parser amend).

---

## 2. Test fixtures

We re-use the fixture proto from `examples/equivalence/proto/acme/equiv/v1/`:
- `zoo.proto` — every scalar, real oneof, synthetic oneof, two map types, doubly-nested messages, top-level + nested enums, cross-file import.
- `neighbors.proto` — cross-file dependency that exercises the relative-name resolver under decode.

Add a Phase 2a-only proto:
- `cycles.proto` — `message Node { Node next = 1; repeated Node children = 2; }` for recursion-limit and depth tests.
- `proto2.proto` — `syntax = "proto2";` with `optional int32 count = 1 [default = 42];` and a `required string name = 2;` for default-value synthesis and proto2 required semantics.

---

## 3. Acceptance checklist

Before posting "ready for review" the implementer ticks:

- [ ] `cargo build --workspace --all-features` clean.
- [ ] `cargo test --workspace --all-features` clean.
- [ ] `cargo build --workspace --no-default-features --features=derive` clean (proves `dynamic` is opt-out clean).
- [ ] `cargo test --workspace --no-default-features --features=derive` clean (no test depends on `dynamic` outside the dynamic feature).
- [ ] `cargo +nightly fmt --check` clean.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo doc --workspace --no-deps` clean.
- [ ] Every scenario in design §13 is covered by tests with descriptive `test_should_…` names (currently 9 scenarios: byte-equivalence, set/get round-trip, try_set rejection, unknown-field preservation, recursion-limit, transcode_to_dynamic round-trip, transcode_to round-trip, DynamicMessage-as-ReflectMessage, Send+Sync, feature opt-out).
- [ ] The conformance harness runs against the full fixture set and `known_failures.txt` documents every failure.
- [ ] `examples/equivalence/` extended to cover `DynamicMessage` parity.
- [ ] No `unsafe` (the crate-level `unsafe_code = "forbid"` attribute remains).
- [ ] No `unwrap`/`expect` outside test code, except in `transcode_to_dynamic`'s self-encode round-trip (documented invariant).

---

## 4. Risks & mitigations during implementation

| Risk | Mitigation |
| --- | --- |
| Two-pass encode + map ordering subtly diverges from buffa's typed output. | Run the byte-equivalence test (§D4) against the full equivalence fixture before merging. Discrepancies surface as a single `assert_eq!` diff with the offending bytes. |
| Default-value parsing for proto2 (`default_value = "0x1f"` for bytes, `default_value = "nan"` for doubles, etc.) hits edge cases the spec only documents informally. | Lift the parser shape from `vendors/prost-reflect/prost-reflect/src/descriptor/build/resolve.rs:627-698` — it covers signed/unsigned int (decimal/octal/hex), floats with `inf`/`nan`, bools, C-escaped strings, byte literals, enum-by-name. Errors accumulate at pool-build time so they surface as `DescriptorPool::decode` failures, not surprises at first read. |
| `Bytes` storage in `Value::Bytes` interacts with `Bytes::clone` cost in surprising ways for very large blobs. | `Bytes` clones are refcount bumps; no copy. The downstream user only pays for the deep clone when they explicitly call `.to_vec()`. Documented. |
| Recursion guard interacts with `Drop` for very deep trees. | Document the limitation; iterative drop is a future improvement if real workloads exhibit it. |
| Performance regression vs. typed code is meaningful (~5–10× rather than the expected 2–3×). | Ship Phase 2a without micro-optimization; benchmark in a follow-up using `criterion`. The PRD already disclaims hot-path optimization for this phase. |

---

## 5. Phase 2a → Phase 2b/2c handoff

Phase 2b (JSON) and 2c (textproto) consume `DynamicMessage` via the public surface only. They do **not** reach into `dynamic::storage` internals. The contract is:

- `populated_fields()` for "what got set" iteration.
- `get_field`/`get_field_mut` for value access.
- `Value` and `MapKey` are the canonical exchange types.

If 2a needs internal-API additions to support 2b/2c (e.g., a way to serialize a `Value` directly without going through a `DynamicMessage`), those are added back in 2a's surface as `pub` APIs in `dynamic/mod.rs`. We do not let 2b/2c reach into `pub(crate)` storage.

---

## 6. Out of scope for Phase 2a (parked)

- A `Builder` flag on `buffa-reflect-build` to expose a per-package `static DESCRIPTOR_POOL: LazyLock<DescriptorPool>` with a dynamic-friendly factory. (Convenience; downstream can write 5 lines themselves.)
- A `transcode_lazy_to_dynamic` that avoids the encode round-trip via specialization. Requires unsafe / nightly features; not worth it.
- `DynamicMessage::merge_from_dynamic(&other)` — merging two dynamics in pure Rust without re-encoding. Useful but additive; ship Phase 2a without and add when a real consumer asks.
