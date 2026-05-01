# buffa-reflect — Implementation Plan

Pre-reads: [buffa-reflect-prd.md](./buffa-reflect-prd.md), [buffa-reflect-design.md](./buffa-reflect-design.md).

This plan covers Phase 1 only (build-script crate + minimum runtime). Phase 2 (`DynamicMessage`, JSON, text-format) ships under a separate spec.

---

## 1. Milestones

### M1 — Workspace scaffolding (≤1 day)

- Rename `crates/core/` → `crates/buffa-reflect/`. Update `crates/buffa-reflect/Cargo.toml`'s `name` and the workspace `Cargo.toml` `buffa-reflect-core` workspace dep alias.
- Create `crates/buffa-reflect-build/` and `crates/buffa-reflect-derive/` package skeletons with empty `lib.rs` and CI-clean `Cargo.toml` files.
- Add buffa workspace deps as path-overrides in the workspace `Cargo.toml` (`buffa`, `buffa-descriptor`, `buffa-build`, `buffa-codegen` all `path = "../../vendors/buffa/<crate>"`). Alias each to its crates.io name so that future swaps are mechanical.
- Smoke check: `cargo build -p buffa-reflect -p buffa-reflect-build -p buffa-reflect-derive`.

### M2 — `buffa-reflect` runtime, validation only (1–2 days)

- `DescriptorPool::decode` + `from_file_descriptor_set` + the index-building pipeline.
- All public descriptor handles with index-based lookup (`get_message_by_name`, `get_field_by_number`, etc.).
- `Kind` resolution from `FieldDescriptorProto.{type, type_name}` against the pool's own message/enum tables.
- Validation pass: name resolution, field-number range, proto3 enum-zero, duplicate FQNs.
- Unit tests in-module:
  - small hand-crafted `FileDescriptorSet` (programmatically built from `buffa_descriptor` types) decoded into a pool.
  - assert `pool.get_message_by_name("...").unwrap().fields().len()`, `get_field_by_number(2)`, etc.
  - error cases: dangling `type_name`, duplicate type, out-of-range field number.

### M3 — `buffa-reflect-derive` (1 day)

- Parse `#[buffa_reflect(descriptor_pool = "...", message_name = "...")]` (and the `file_descriptor_set_bytes` alternate) using `syn::meta::ParseNestedMeta`.
- Two expansion shapes per design §4.2.
- Compile-time error diagnostics with `syn::Error::new_spanned`.
- `trybuild` tests under `crates/buffa-reflect-derive/tests/ui/` for the four error cases (missing binding, both bindings, missing message name, bad attribute shape).

### M4 — `buffa-reflect-build` (2–3 days, the longest pole)

- Implement Builder per design §3.1.
- Implement `compile()` per design §3.2:
  - protoc invocation (`Command::new(env!("PROTOC")).args([...]).output()`); same probe logic as buffa-build (`PROTOC` env or PATH).
  - buf invocation (`buf build --as-file-descriptor-set -o <path>`).
  - precompiled-source copy.
  - decode FDS via `buffa_descriptor::generated::descriptor::FileDescriptorSet::decode_from_slice`.
  - walk top-level + nested messages, accumulate FQNs.
  - construct `buffa_build::Config`, push three `type_attribute` entries per message, push user passthrough attrs/options.
  - call `Config::descriptor_set(fds_path).compile()`.
  - emit `cargo:rerun-if-changed` for each `.proto` and the FDS file; `cargo:rerun-if-env-changed=PROTOC`.
- Integration tests in `crates/buffa-reflect-build/tests/`:
  - **fixture/**: a tiny `.proto` (e.g., `acme/api/v1/user.proto` with one message and one enum).
  - **`build_protoc.rs`**: spawns the Builder against the fixture in a tempdir, asserts `OUT_DIR/file_descriptor_set.bin` exists, asserts the generated `.rs` contains `#[derive(::buffa_reflect::ReflectMessage)]`.
  - **`build_precompiled.rs`**: `protoc --descriptor_set_out=...` once, then drives Builder via `descriptor_set(...)`.
  - **`build_buf.rs`** (gated on `buf` being on PATH): same shape, via buf.
- Doc-tests in lib.rs covering the two configuration shapes (pool, bytes).

### M5 — End-to-end smoke example (½ day)

- New crate under `apps/` (or repurpose `apps/server/`) demonstrating:
  - a real `.proto` with nested messages, enum, oneof, map field;
  - `build.rs` wired to `buffa_reflect_build::Builder`;
  - `main.rs` that constructs a message and walks its fields by descriptor.
- This is also the artifact the README screenshot/snippet links to.

### M6 — CI integration & final polish (½ day)

- `Makefile` targets:
  - `make test` → `cargo build --workspace && cargo test --workspace`.
  - `make lint` → `cargo +nightly fmt --check && cargo clippy --workspace -- -D warnings`.
  - `make verify` → both, plus `cargo doc --workspace --no-deps`.
- README in each new crate's directory with the bare-minimum quickstart.
- Update the root `README.md` with a top-level "What is this" paragraph + pointer to specs/docs.

Total estimate, end-to-end: **~7 working days** for a single contributor.

---

## 2. Test fixtures

The minimum viable fixture set lives under `crates/buffa-reflect-build/tests/fixtures/proto/`:

```
acme/api/v1/user.proto       # nested package, message User { string name = 1; Role role = 2; ... }
acme/api/v1/role.proto       # enum Role { ROLE_UNSPECIFIED = 0; ADMIN = 1; ... }
acme/api/v1/contact.proto    # one-of Contact (email|phone), map<string, string> labels
```

These exercise:
- nested package paths and the resulting Rust module tree;
- enums (so `Kind::Enum(EnumDescriptor)` wires correctly);
- oneofs (so `FieldDescriptor::containing_oneof()` returns Some);
- maps (synthetic map-entry messages — verify they're walked but not exposed as user-facing types).

Conformance with the protobuf wire format is **not** in scope for Phase 1; we get that for free from buffa itself.

---

## 3. Acceptance checklist

This is the bar the implementer checks against before posting "ready for review":

- [ ] `cargo build --workspace` clean.
- [ ] `cargo test --workspace` clean (≥80% line coverage on new code per CLAUDE.md).
- [ ] `cargo +nightly fmt --check` clean.
- [ ] `cargo clippy --workspace -- -D warnings` clean.
- [ ] `cargo doc --workspace --no-deps` builds; every public item has a `///` doc comment.
- [ ] `cargo audit` reports no advisories on the new crates' dep trees.
- [ ] All four `trybuild` UI tests in `buffa-reflect-derive` produce stable, reviewable diagnostics.
- [ ] At least one integration test exercises **each** of `Protoc` / `Buf` / `Precompiled` descriptor sources (the `buf` test may be `#[ignore]` and run only in CI where buf is provisioned).
- [ ] The example crate in `apps/` runs and prints field metadata for a real message, end-to-end.
- [ ] `README.md` updated.
- [ ] No `unwrap()` / `expect()` outside test code or with-justification `expect()` calls in macro-generated code (which already pre-quote a static message).
- [ ] No new dependencies pulled into the runtime crate beyond those listed in design §5.7.

---

## 4. Risks & mitigations during impl

| Risk | Mitigation |
| --- | --- |
| Path-dep on the buffa submodule means commit-pinning matters. | Document the pinned commit in `vendors/buffa/COMMIT.md`. CI re-clones the submodule deterministically. |
| protoc invocation differs subtly from buffa-build's. | Read `vendors/buffa/buffa-build/src/lib.rs:invoke_protoc` and replicate flags exactly (`--include_imports --include_source_info --descriptor_set_out`). Add an integration test that diffs our `file_descriptor_set.bin` against `protoc`'s direct output for the same fixtures. |
| Macro emits a `static OnceLock` per message — bloat for crates with hundreds of messages. | Acceptable; each `OnceLock` is one pointer-sized atomic + a tagged init bit. For very large schemas (>1000 messages) the pool form sidesteps this entirely. Document the tradeoff. |
| Users want both reflection and the existing `apps/server` pattern. | Builder is purely additive over `buffa_build::Config`; non-reflective consumers continue using `buffa-build` directly with no behavior change. |
| Generating descriptor pool indices for huge schemas is O(N) per build. | Accepted (same cost as prost-reflect). Cache benchmarking deferred to Phase 2. |

---

## 5. Phase 2 follow-ups (out of scope here)

Captured so the design choices in this plan don't paint us into a corner:

1. `DynamicMessage` + `Value` + `MapKey`. Re-implements buffa's encode/decode driven by `MessageDescriptor`. Most of the work; needs its own design.
2. `transcode_to_dynamic()` default impl on `ReflectMessage` (gated behind a `dynamic` feature on `buffa-reflect`).
3. `serde` feature on `buffa-reflect`: canonical proto3 JSON for `DynamicMessage`, with `SerializeOptions`/`DeserializeOptions` mirroring prost-reflect.
4. `text-format` feature on `buffa-reflect`: textproto round-trip for `DynamicMessage`.
5. View-type reflection: emit `#[derive(ReflectMessage)]` on `__view::*View<'a>` types too. Requires a new `ReflectMessageView<'a>` trait or a generalized `: ReflectMessage` bound; design TBD.
6. Optional global pool helper: `buffa_reflect::global() -> &'static DescriptorPool`. Plus `add_to_global(...)`.
7. Conformance suite via `DynamicMessage`, mirroring `vendors/prost-reflect/prost-reflect-conformance-tests/`.
8. Optional upstream PR to `buffa-build` adding a `pre_codegen_hook` that lets us inject `type_attribute` entries without duplicating descriptor enumeration logic.

Any one of these deserves its own design pass; tracking them here only so the Phase 1 module/feature naming reserves the right space.
