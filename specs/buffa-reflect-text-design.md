# Phase 2c — Textproto encode/decode

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). **Depends on** [`DynamicMessage`](./buffa-reflect-dynamic-design.md).

Textproto (sometimes "protobuf text format") is the human-readable serialization printed by `protoc --decode_raw` and used widely in Google internal config files. It is **less specified** than proto3 JSON and has more variants in the wild. Goal here is to match `protoc`'s output exactly so the buf / protoc / textproto-based tooling ecosystem treats us as a peer.

The design follows prost-reflect's textproto module (`vendors/prost-reflect/prost-reflect/src/dynamic/text_format/`) — including using **`logos` for tokenization** (battle-tested escape handling) and a hand-written recursive-descent parser on top.

---

## 1. Goals

- `DynamicMessage::to_text_format(&self) -> String` and `to_text_format_with_options(&self, &FormatOptions)` producing canonical textproto.
- `DynamicMessage::parse_text_format(descriptor: MessageDescriptor, src: &str) -> Result<Self, ParseError>` accepting the same format.
- Round-trip identity: for every fixture, `parse_text_format(d, msg.to_text_format())? == msg`.
- Compatibility: bytes produced match `protoc --decode <FQN>` modulo whitespace differences when `pretty = true`.
- Support `[type.googleapis.com/X] { … }` Any expansion on **both** parse and emit (controlled by `expand_any`).

## 2. Non-goals

- Round-trip of comments / source positions. Textproto comments are stripped on parse and never emitted.
- Custom field-printer extensibility (à la C++ `TextFormat::Printer`). The format is fixed; sufficient for prost-reflect parity.
- Round-trip of insignificant whitespace.

---

## 3. Format primer

Textproto by example:

```text
name: "Bath Public Library"
books {
  id: "b-001"
  title: "Pride and Prejudice"
  authors: "Jane Austen"
  authors: "Charlotte Brontë"
  genre: GENRE_FICTION
  excerpts {
    page: 1
    text: "It is a truth..."
  }
}
tags {
  key: "city"
  value: "Bath"
}
```

Rules:
- Field name first, then `:` then value (scalars, strings) or block `{ … }` (sub-messages). The `:` is omitted when the value is a `{...}` block.
- Repeated fields: one entry per field name (no list literal); the `[v1, v2, v3]` short-form is also legal and should be accepted on parse.
- Maps: emitted as repeated entries with `key:` / `value:` pairs, one synthetic message per pair.
- Strings: C-style escapes (`\n`, `\xFF`, `\NNN` octal, `\uXXXX` unicode). Bytes: same syntax with raw-bytes interpretation.
- Enums: variant name (no quotes); decimal number also accepted on parse.
- Singular sub-messages: `field { ... }`.
- Comments: `# to end of line`.
- Extensions: `[fully.qualified.ext_name]: value` syntax.
- Any expansion: `[type.googleapis.com/Foo] { ... }` is the expanded form; the unexpanded form has `type_url` and `value` fields.

---

## 4. Public surface

```rust
// crates/buffa-reflect/src/dynamic/text_format/mod.rs
#[cfg(all(feature = "dynamic", feature = "text-format"))]
pub use crate::dynamic::text_format::{FormatOptions, ParseError};

#[derive(Debug, Clone)]
pub struct FormatOptions {
    /// Multi-line, indented output. Default: `false` (compact, single-line).
    pretty: bool,
    /// Skip unknown fields (don't print them) on output. Default: `false`.
    skip_unknown_fields: bool,
    /// Use the expanded `[type.googleapis.com/X] { ... }` syntax for
    /// `google.protobuf.Any`. Default: `true`.
    expand_any: bool,
    /// Skip fields equal to their default value. Default: `false`
    /// (textproto convention is to print everything; differ from JSON
    /// where omission is the default).
    skip_default_fields: bool,
    /// Print fields in the proto declaration order rather than field-
    /// number order. Default: `false`.
    print_message_fields_in_index_order: bool,
}

impl FormatOptions {
    pub fn new() -> Self;
    #[must_use] pub fn pretty(self, b: bool) -> Self;
    #[must_use] pub fn skip_unknown_fields(self, b: bool) -> Self;
    #[must_use] pub fn expand_any(self, b: bool) -> Self;
    #[must_use] pub fn skip_default_fields(self, b: bool) -> Self;
    #[must_use] pub fn print_message_fields_in_index_order(self, b: bool) -> Self;
}

impl DynamicMessage {
    pub fn to_text_format(&self) -> String;
    pub fn to_text_format_with_options(&self, options: &FormatOptions) -> String;
    pub fn parse_text_format(
        desc: MessageDescriptor,
        input: &str,
    ) -> Result<Self, ParseError>;
}

#[derive(Debug)]
pub struct ParseError {
    /// Source line (1-based).
    line: u32,
    /// Source column (1-based).
    col: u32,
    /// Underlying parser error kind (string-typed enum, see source).
    kind: ParseErrorKind,
}
```

API names mirror prost-reflect (`vendors/prost-reflect/prost-reflect/src/dynamic/text_format/mod.rs:80, 99, 40`) for cross-ecosystem familiarity.

There is no `serde` integration — textproto is not a serde-friendly format (no defined value model, custom escape rules, extension syntax don't map cleanly to serde).

---

## 5. Lexer: `logos`

The earlier draft proposed a hand-written tokenizer "to keep deps thin." Audit changed my mind: **prost-reflect uses `logos`** (`vendors/prost-reflect/prost-reflect/src/dynamic/text_format/parse/lex.rs:3-10`) and gets battle-tested escape handling for free. The dep cost is acceptable (logos is small, ~100 KB compiled, no runtime cost). Re-implementing the tokenizer would risk getting one of the C-style escape edge cases wrong (octal parses up to 3 digits but stops on the first non-octal-digit; hex requires exactly 2 digits; multi-byte unicode escapes have specific rules).

The lexer skips whitespace and `# to end of line` comments via `#[logos(skip ...)]` directives. Token enum covers: identifiers, string literals (with all escape variants pre-decoded), numeric literals (decimal/hex/octal/float with `inf`/`nan`), punctuation (`:` `{` `}` `[` `]` `,` `<` `>`).

```rust
#[derive(Logos)]
#[logos(skip r"[\t\v\f\r\n ]+")]
#[logos(skip r"#[^\n]*\n?")]
enum Token<'a> {
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident(&'a str),
    #[regex("...complex string regex...", parse_string)]
    StringLit(Vec<u8>),
    // ... etc.
}
```

Detailed lexer regexes lift directly from `prost-reflect/src/dynamic/text_format/parse/lex.rs` — that file is the spec.

## 6. Parser: hand-written recursive descent

On top of the logos token stream sits a hand-written parser with one-token peek lookahead (matches `parse/mod.rs:21-30`). Approximate grammar:

```
message       := (field)*
field         := field_name ':' scalar_or_list
                | field_name '{' message '}'
                | field_name '<' message '>'                    // legacy `<...>` block syntax
field_name    := identifier
                | '[' qualified_name ']'                        // extension or Any expansion
qualified_name:= identifier ('.' identifier)*
                | identifier ('.' identifier)* '/' identifier   // type.googleapis.com/X
scalar_or_list:= scalar
                | '[' scalar (',' scalar)* ']'                  // short repeated form
scalar        := number | string | bool | enum_name
```

`field_name` parses into a `FieldName` enum: `Ident(string)`, `Extension(string)`, `Any(type_url)`. Same shape as prost-reflect (`parse/mod.rs:26-30`).

### Round-trip identity

The printer emits in `print_message_fields_in_index_order` order if requested; otherwise in field-number order (the `BTreeMap` natural order). Test fixtures exercise both modes.

For `Any` fields with `expand_any = true`, the printer:
1. Looks up the inner type via `dyn_msg.parent_pool().get_message_by_name(type_url_inner_name)`.
2. Decodes `value` bytes against that descriptor.
3. Emits `[type.googleapis.com/X] { ...inner fields... }`.

If lookup fails, falls back to the unexpanded form (`type_url: "..." value: "..."`). Per prost-reflect — silent-fallback rather than error so corrupted Any fields don't crash log output.

---

## 7. Module layout

```
crates/buffa-reflect/src/dynamic/text_format/
  mod.rs                    # public surface, FormatOptions
  format.rs                 # printer (Writer struct)
  parse/
    mod.rs                  # parser entry, recursive descent, FieldName enum
    lex.rs                  # logos lexer
    error.rs                # ParseError, ParseErrorKind
```

Mirrors `vendors/prost-reflect/prost-reflect/src/dynamic/text_format/`.

---

## 8. Cargo features

```toml
[features]
text-format = ["dynamic", "dep:logos"]
```

Feature name `text-format` (not `text`) — match prost-reflect (`vendors/prost-reflect/prost-reflect/Cargo.toml:26`).

---

## 9. Testing

- Round-trip per field shape, same fixture set as Phase 2b.
- Tabular tests for escaping (every C-style escape, multi-byte UTF-8, embedded NUL, octal-stops-on-non-digit edge cases).
- Reference fixtures captured from `protoc --decode <FQN> < zoo.bin` and asserted byte-equal modulo whitespace differences with `pretty`.
- Both modes for `print_message_fields_in_index_order`.
- Both modes for `expand_any`.
- Conformance suite: extend `crates/buffa-reflect-conformance-tests` with the textproto harness. **Target 100 % pass** (`vendors/prost-reflect/prost-reflect-conformance-tests/text_format_failure_list.txt` is empty).
- Parser fuzz target (`cargo-fuzz` integration) — random strings → either parse or error, never panic. Phase 2c lands the corpus and harness; CI fuzz integration deferred.

---

## 10. Risks

| Risk | Mitigation |
| --- | --- |
| Textproto's `[ext.name]` extension syntax requires extension lookup. | Phase 2c parses via `pool.get_extension_by_name(...)`. The pool side already exposes raw extension descriptors via `descriptor_proto()`; promoting them to first-class is a separate Phase 2 add (Phase 2-extensions, not specified yet). For Phase 2c, extensions parse but are stored as unknown fields if no descriptor is found — the same lossy behaviour `protoc --decode` exhibits on missing extensions. |
| Number formats (`inf`, `-inf`, `nan`, `0x1f`, scientific notation, leading-zero octals like `0123`) interact in surprising ways. | Lift the lexer regex tables from prost-reflect verbatim; the test fixtures from prost-reflect-conformance-tests cover the corner cases. |
| String escapes interact with UTF-8 (e.g., `\xC3\xA9` is valid bytes that decode as `é`). | Decode escapes byte-by-byte; for `string` fields, validate UTF-8 after; for `bytes`, accept arbitrary. |
| `[type.googleapis.com/X] { ... }` Any expansion requires looking up X in the type registry. | Use `dyn_msg.parent_pool()` (same as JSON Any). On lookup miss, fall back to unexpanded form on emit; on parse, store as bytes. |
| Map field round-trip ordering depends on `HashMap` iteration order (non-deterministic). | Encode maps in sorted key order (cheap, see dynamic-design.md §3). |
| Multiline string literals (`"foo" "bar"` concatenation, per textproto spec) — easy to miss. | Lexer handles by emitting two StringLit tokens; parser concatenates adjacent ones. Test fixture covers. |

---

## 11. Acceptance for Phase 2c

- Round-trip per fixture passes.
- Reference parity vs `protoc --decode` for the equivalence-suite fixtures (whitespace-tolerant comparison).
- Conformance text format ≥ 100 % pass rate; any documented gaps in `text_format_failure_list.txt` with justifications.
- `cargo test --workspace --features=text-format` clean.
- `cargo build --workspace --no-default-features --features="derive,dynamic"` clean (proves `text-format` is a clean opt-in).
