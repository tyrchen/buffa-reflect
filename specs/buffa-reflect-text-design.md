# Phase 2c — Textproto encode/decode

Companion to [phase-2 PRD](./buffa-reflect-phase2-prd.md). **Depends on** [`DynamicMessage`](./buffa-reflect-dynamic-design.md).

Textproto (sometimes "protobuf text format") is the human-readable serialization printed by `protoc --decode_raw` and used widely in Google internal config files. It is **less specified** than proto3 JSON and has more variants in the wild. Goal here is to match `protoc`'s output exactly so the buf / protoc / textproto-based tooling ecosystem treats us as a peer.

---

## 1. Goals

- `DynamicMessage::to_text(&self) -> String` producing canonical textproto.
- `DynamicMessage::from_text(descriptor: MessageDescriptor, src: &str) -> Result<Self, TextError>` accepting that same format.
- Round-trip: for every fixture, `from_text(d, msg.to_text())? == msg`.
- Compatibility: bytes produced match `protoc --decode <fixture>` modulo whitespace.

## 2. Non-goals

- `Any` payload "expanded" form (`[type.googleapis.com/X] { … }`) — accept on parse, but always emit the unexpanded form. Easier to interoperate.
- Round-trip of comments / source positions. Textproto comments are stripped on parse and never emitted.
- Custom field-printer extensibility (à la C++ `TextFormat::Printer`). The format is fixed; sufficient for prost-reflect parity.

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
- Field name first, then `:` then value (scalars, strings) or block `{ … }` (sub-messages).
- Repeated fields: one entry per field name (no list literal).
- Maps: emitted as repeated entries with `key:` / `value:` pairs.
- Strings: C-style escapes (`\n`, `\xFF`, etc.). Bytes: same syntax with raw-bytes interpretation.
- Enums: variant name (no quotes); decimal number also accepted on parse.
- Singular sub-messages: `field { ... }`.
- Nested messages: indented by 2 spaces per level (canonical printer); parser is whitespace-tolerant.

The `[type.googleapis.com/Foo] { … }` "expanded Any" form is accepted on parse but Phase 2c does not emit it.

---

## 4. Public surface

```rust
#[cfg(all(feature = "dynamic", feature = "text"))]
impl DynamicMessage {
    /// Render this message in canonical textproto format.
    pub fn to_text(&self) -> String;

    /// Render with explicit options (indent width, fields-on-one-line, etc.).
    pub fn to_text_with(&self, opts: &TextOptions) -> String;

    /// Parse a textproto string.
    pub fn from_text(
        descriptor: MessageDescriptor,
        src: &str,
    ) -> Result<Self, TextError>;
}

#[derive(Clone, Debug)]
pub struct TextOptions {
    /// Spaces per indent level.  Default: 2.
    pub indent: usize,
    /// Emit fields equal to default values.  Default: false (omitted).
    pub emit_default_fields: bool,
    /// Emit enums as numbers instead of names.  Default: false.
    pub emit_enum_values_as_numbers: bool,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TextError {
    #[error("textproto parse error at line {line} col {col}: {message}")]
    Parse { line: u32, col: u32, message: String },
    #[error("unknown field `{field}` in `{full_name}`")]
    UnknownField { full_name: String, field: String },
    #[error("type mismatch on `{full_name}`: expected {expected}, got {actual}")]
    TypeMismatch {
        full_name: String,
        expected: &'static str,
        actual: &'static str,
    },
    #[error("invalid escape sequence: {0}")]
    BadEscape(String),
}
```

`to_text` is a free convenience wrapper around `to_text_with(&TextOptions::default())`.

There is no `serde` integration — textproto is not a serde-compatible format.

---

## 5. Parser

A small hand-written recursive-descent parser. **Not** `nom` or `winnow` to keep the dep tree thin (json already brings serde_json; we don't add another parser combinator). Approximate grammar:

```
message       := field*
field         := name ':' scalar | name '{' message '}'
                | name ':' '[' scalar (',' scalar)* ']'      // (legal but rare)
                | '[' typeurl ']' '{' message '}'             // expanded Any
name          := identifier ('.' identifier)*
scalar        := number | string | bool | enum_name
string        := '"' (cstyle_char | escape)* '"'
escape        := '\' ('n' | 't' | 'r' | '\\' | '"' | "'" | 'x' hex hex | octal{1,3})
number        := '-'? (decimal | '0x' hex+ | float)
bool          := 'true' | 'false'
enum_name     := identifier
```

Whitespace and `#` comments are lexer-level skips.

---

## 6. Printer

Stateful printer with an `indent: usize` counter. Walks fields in `descriptor.fields()` order (same order as wire encoding); for each populated field:

- scalar: `<name>: <repr>\n`
- repeated: one line per entry
- map: one block per entry: `<name> {\n  key: <k>\n  value: <v>\n}\n`
- message: `<name> {\n` + recurse (indent + 2) + `}\n`

Special-case escaping for strings (C-style: control chars, `"`, non-ASCII).

Special-case bytes: same as strings but every non-printable-ASCII byte is escaped as `\<octal>`.

---

## 7. Module layout

```
crates/buffa-reflect/src/
  text/
    mod.rs            # public surface
    options.rs        # TextOptions
    parser.rs         # tokenizer + recursive-descent parser
    printer.rs        # canonical printer
    escape.rs         # C-style string/bytes escaping
    error.rs          # TextError
```

Feature: `text = ["dynamic"]`. No external deps beyond what `dynamic` already pulls in.

---

## 8. Testing

- Round-trip per field shape, same fixture set as Phase 2b.
- Tabular tests for escaping (every C-style escape, multi-byte UTF-8, embedded NUL).
- Reference fixtures captured from `protoc --decode google.protobuf.Library < zoo.bin` and asserted byte-equal.
- Textproto conformance suite (separate suite from binary/JSON): `known_failures_text.txt`.
- Parser fuzz target (`cargo-fuzz` integration): random strings → either parse or error, never panic. Phase 2c lands the corpus + harness; CI fuzz integration deferred.

---

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| Textproto's "extension" syntax `[fully.qualified.ext] { ... }` is rarely used but legal. | Phase 2c parses but does not emit (no first-class extension API yet). Documented. |
| Number formats: `inf`, `-inf`, `nan`, `0x1f`, scientific notation — combinations multiply. | Use Rust's `str::parse::<f64>` for floats (handles `inf`/`nan`/scientific) and `i64::from_str_radix(s, 16)` after a `0x` prefix-strip for hex. Test each branch. |
| String escapes interact with UTF-8 in interesting ways (e.g., `\xC3\xA9` is valid bytes that decode as `é`). | Decode escapes byte-by-byte; for `string` fields, validate UTF-8 after; for `bytes`, accept arbitrary. |
| `[type.googleapis.com/Foo] { ... }` Any expansion requires looking up Foo in the type registry. | Phase 2c parses the syntax, looks up via `parent_pool().get_message_by_name(...)`; emits canonical Any (`type_url` + `value`) form. |

---

## 10. Acceptance for Phase 2c

- Round-trip per fixture passes.
- Reference parity vs `protoc --decode` for the equivalence-suite fixtures.
- Conformance text format ≥ 95 % pass rate; the rest in `known_failures_text.txt` with justifications.
- `cargo test --workspace --features text` clean.
