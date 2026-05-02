# Phase 2a — `DynamicMessage` Implementation Plan

Pre-reads: [phase-2 PRD](./buffa-reflect-phase2-prd.md), [dynamic design](./buffa-reflect-dynamic-design.md).

This plan covers Phase 2a only. JSON / textproto / gRPC / view reflection have their own plans, all of which transitively depend on this one.

---

## 1. Milestones

### D1 — Module skeleton + `Value` / `MapKey` (1 day)

- New `crates/buffa-reflect/src/dynamic/{mod,value,storage}.rs` — empty types, `Cargo.toml` `[features] dynamic = []` flag default-on.
- `Value` enum + `MapKey` enum + `Default` / `From` / `PartialEq` / `Debug` impls.
- `DynamicMessage::new(descriptor)` — empty constructor.
- Smoke test: `DynamicMessage::new(d).descriptor() == d`.

### D2 — Accessors: get/set/has/clear (2–3 days)

- `accessors.rs`: implement the eight `*_by_name` / `*_by_number` / `*_by_field_descriptor` variants.
- Type validation for `set_field` per the table in design §5.
- Oneof set/clear invariants (mutate `slots` + `oneof_active` together).
- Default-value synthesis (`defaults.rs`) for the proto3 zero values; proto2 explicit `default_value` parsing for scalars.
- Tests:
  - `set` then `get` round-trips for every Kind.
  - `set` of wrong type returns `SetFieldError::TypeMismatch` (no panic).
  - `set` on field A of oneof, then field B of same oneof — `has(A) == false`, `has(B) == true`.
  - `clear` of an absent field is a no-op.
  - Nested message: build a multi-level tree with `set_field`.

### D3 — Wire decode (2 days)

- `decode.rs`: `merge_from_slice` loop, dispatching by `KindRef` and `cardinality`.
- Recursion-limit threading (default `RECURSION_LIMIT`).
- Packed-vs-unpacked dual reading for repeated scalars (handle either wire encoding regardless of the field's packed flag — protoc emits both depending on the producer).
- Unknown-field preservation via `encoding::decode_unknown_field`.
- Forward-compat enum decoding: store unknown enum numbers in `Value::EnumNumber(raw)`.
- Tests:
  - For every fixture in the equivalence suite (`examples/equivalence/proto/`), construct the typed message via `buffa`, encode to bytes, then `DynamicMessage::decode` against the same descriptor — assert non-error and that `populated_fields()` count matches expectations.
  - Recursion-limit guard: a hand-crafted 200-deep wire input fails with `RecursionLimitExceeded`.
  - Unknown-field round-trip: append a fake tag with a varint payload; decode; observe the bytes are preserved.

### D4 — Wire encode (2–3 days)

- `encode.rs`: `compute_size` + `write_to` mirroring buffa's `Message::compute_size` / `write_to` pattern.
- Per-Kind dispatch (table in design §3).
- Packed encoding emission for `is_packed()` repeated scalars.
- Map field encoding via synthetic `Entry { K key = 1; V value = 2; }`.
- Field iteration order = descriptor's `field` declaration order.
- `unknown_fields.write_to(buf)` after known fields.
- Tests:
  - Set-then-encode produces non-empty bytes that decode back to the same `DynamicMessage` (PartialEq).
  - **Byte-equivalence**: for every equivalence-suite fixture, `buffa_typed.encode_to_vec() == DynamicMessage::decode(d, buffa_typed.encode_to_vec()).encode_to_vec()`.
  - Map encoding order is deterministic (BTreeMap → ascending key order).
  - Packed scalars round-trip through `is_packed = true` and `is_packed = false` fixtures.

### D5 — `ReflectMessage::transcode_to_dynamic` / `from_dynamic` (1 day)

- Two trait methods with default impls (design §6).
- The methods compile cleanly when `dynamic` is off (cfg-gated).
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

Total: **~10–13 working days** for one contributor.

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
- [ ] All eight scenarios in design §10 are covered by tests with descriptive `test_should_…` names.
- [ ] The conformance harness runs against the full fixture set and `known_failures.txt` documents every failure.
- [ ] `examples/equivalence/` extended to cover `DynamicMessage` parity.
- [ ] No `unsafe` (the crate-level `unsafe_code = "forbid"` attribute remains).
- [ ] No `unwrap`/`expect` outside test code, except in `transcode_to_dynamic`'s self-encode round-trip (documented invariant).

---

## 4. Risks & mitigations during implementation

| Risk | Mitigation |
| --- | --- |
| Two-pass encode + map ordering subtly diverges from buffa's typed output. | Run the byte-equivalence test (§D4) against the full equivalence fixture before merging. Discrepancies surface as a single `assert_eq!` diff with the offending bytes. |
| Default-value parsing for proto2 (`default_value = "0x1f"` for bytes, `default_value = "nan"` for doubles, etc.) hits edge cases the spec only documents informally. | For Phase 2a: only support a documented subset (numeric literals, bool, string with C-style escapes, enum by name). Anything else falls back to the kind's zero value with a debug-log warning. JSON / textproto phases tighten this. |
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
