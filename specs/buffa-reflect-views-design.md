# Phase 2e — View-type reflection

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). **Depends only on Phase 1**, so this can ship independently of `DynamicMessage`.

This is the smallest deliverable in Phase 2 by code size, but conceptually load-bearing for any consumer that uses buffa's zero-copy decode path. Without it, any observability code that wants to introspect a message must first allocate to convert the borrowed `*View<'a>` into the owned form — exactly the cost the view types exist to avoid.

---

## 1. Goals

- A new trait `ReflectMessageView<'a>` whose `descriptor()` returns the same `MessageDescriptor` as the owned form.
- Generated `*View<'a>` types implement the trait via the same descriptor binding the owned type uses.
- `buffa-reflect-build` adds an opt-in toggle to emit the view derive (default: on, since it's free in code size when views are already generated).

## 2. Non-goals

- A `DynamicMessageView<'a>` zero-copy dynamic. Phase 2a deliberately ships only the owned form; views are a Phase 3 add-on.
- `transcode_to_dynamic` from a view (view → owned would defeat the zero-copy). Skip; consumers can `view.to_owned_message()` first if they need that.

---

## 3. Public surface

```rust
// crates/buffa-reflect/src/reflect.rs
pub trait ReflectMessage: ::buffa::Message {
    fn descriptor(&self) -> MessageDescriptor;
    // ... Phase 2 methods (transcode_to_dynamic, from_dynamic) gated on `dynamic`
}

/// Reflection over a buffa view type. Bound to `MessageView` so the
/// resolver can be sure it operates on a borrowed view.
pub trait ReflectMessageView<'a>: ::buffa::view::MessageView<'a> {
    fn descriptor(&self) -> MessageDescriptor;
}
```

That's the entire trait. Both traits return the **same** `MessageDescriptor` for the same proto message, regardless of which Rust type produced it.

---

## 4. Implementation in `buffa-reflect-build`

Today, `Builder::apply_reflection_attributes` emits these on every message struct:

```rust
#[derive(::buffa_reflect::ReflectMessage)]
#[buffa_reflect(message_name = "<FQN>")]
#[buffa_reflect(file_descriptor_set_bytes = "<expr>")]
```

Phase 2e adds the same set, but keyed against view types. Buffa codegen emits views into `<pkg>.<file>.__view.rs` files, with the views named `XxxView<'a>` and matched by the FQN of their owned counterpart.

Two implementation paths considered:

### Path A — A separate `view_attribute` mechanism in buffa-build

Requires upstream change. Not happening for Phase 2e.

### Path B — Walk the descriptor in `buffa-reflect-build` and emit a manual `impl ReflectMessageView<'a>` block via `prepended source`

Cleaner than touching upstream. Approach:

1. In `Builder::compile()`, after `cfg.compile()`, generate a Rust source snippet (`OUT_DIR/_reflect_views.rs`) that contains, for every message in the descriptor:
   ```rust
   impl<'a> ::buffa_reflect::ReflectMessageView<'a> for #fqn_view_path<'a> {
       fn descriptor(&self) -> ::buffa_reflect::MessageDescriptor {
           static __INIT: ::std::sync::OnceLock<::buffa_reflect::DescriptorPool> = ::std::sync::OnceLock::new();
           let pool = __INIT.get_or_init(|| {
               ::buffa_reflect::DescriptorPool::decode(#bytes_expr)
                   .expect("buffa-reflect: invalid FileDescriptorSet")
           });
           pool.get_message_by_name(#fqn).expect(concat!(
               "descriptor for `", #fqn, "` not found",
           ))
       }
   }
   ```
2. Downstream `lib.rs` does `include!(concat!(env!("OUT_DIR"), "/_reflect_views.rs"));` once.
3. New `Builder` method: `Builder::view_reflection_include_file(name)` (default `"_reflect_views.rs"`); `Builder::generate_view_reflection(bool)` toggle (default true).

This avoids any change to the proc-macro derive and keeps view reflection a pure-build-script concern.

The path computation for `#fqn_view_path` is the trickiest part: it must mirror buffa-codegen's view-module naming (`__buffa::view::<file_stem>::XxxView`). We replicate the naming convention from `vendors/buffa/buffa-codegen/src/view.rs`. If buffa upstream changes the convention, this generator is the seam to update.

---

## 5. Module / file layout

No new modules in `buffa-reflect`. One new method (`Builder::generate_view_reflection`) and one new file emitted in `buffa-reflect-build` (`_reflect_views.rs`). The `ReflectMessageView<'a>` trait lives in `crates/buffa-reflect/src/reflect.rs` next to `ReflectMessage`.

---

## 6. Testing

Add to `apps/example/`:
- A consumer-side use of the view reflection: build a `LibraryView<'a>` from wire bytes, call `descriptor()`, walk fields. Print the same field listing as the owned version.
- Assert the descriptor returned by `view.descriptor()` is `==` (`Arc::ptr_eq`-equal pool & same index) to the descriptor returned by `owned.descriptor()`.

Cross-impl: extend `examples/equivalence/` with a `view_descriptor_matches_owned` test.

---

## 7. Risks

| Risk | Mitigation |
| --- | --- |
| Buffa changes the view-module naming convention. | Generator code is one function; update with the upstream change. Generator emits a header comment `// @generated by buffa-reflect-build for buffa $VERSION` so version mismatches are easy to diagnose. |
| Map-entry messages have `MapEntryView<'a>` types (or don't — buffa skips Rust generation for them). | Walk the descriptor's `is_map_entry()` flag and skip. Same logic as `apply_reflection_attributes` for owned types. |
| Adding `ReflectMessageView<'a>` impls in a separate include-file means downstream consumers must remember to `include!` it. | Document; provide a one-line snippet in the README. The auto-emit lives in `OUT_DIR` like the owned reflection's bytes constant — same ergonomic story. |
| Generic over `'a` — trait impls with explicit lifetime. Subtle compile errors if the codegen path has the wrong lifetime parameter. | Test fixture: every fixture in `apps/example/` and `examples/equivalence/` exercises a view's `descriptor()`. |

---

## 8. Acceptance for Phase 2e

- `apps/example/main.rs` calls `descriptor()` on a `LibraryView` and prints the same field metadata as the owned version.
- `Builder::generate_view_reflection(false)` opt-out works (no `_reflect_views.rs` emitted, no compile error from missing `include!`).
- `cargo test --workspace` clean.
