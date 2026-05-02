//! Textproto printer.

use std::fmt::Write as _;

use crate::{
    dynamic::{
        message::DynamicMessage,
        value::{MapKey, Value},
    },
    field::{FieldDescriptor, Kind},
};

/// Configuration for [`crate::DynamicMessage::to_text_format_with_options`].
#[derive(Debug, Clone)]
pub struct FormatOptions {
    pub(crate) pretty: bool,
    pub(crate) skip_unknown_fields: bool,
    pub(crate) expand_any: bool,
    pub(crate) skip_default_fields: bool,
    pub(crate) print_message_fields_in_index_order: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatOptions {
    /// Defaults: compact (single-line), keep unknowns, expand `Any`,
    /// keep defaults, number-order. Matches prost-reflect.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pretty: false,
            skip_unknown_fields: false,
            expand_any: true,
            skip_default_fields: false,
            print_message_fields_in_index_order: false,
        }
    }

    /// Multi-line, indented output. Default `false`.
    #[must_use]
    pub const fn pretty(mut self, yes: bool) -> Self {
        self.pretty = yes;
        self
    }

    /// Drop unknown fields on output. Default `false`.
    #[must_use]
    pub const fn skip_unknown_fields(mut self, yes: bool) -> Self {
        self.skip_unknown_fields = yes;
        self
    }

    /// Use the `[type.googleapis.com/X] { ... }` syntax for `Any`.
    /// Default `true`.
    #[must_use]
    pub const fn expand_any(mut self, yes: bool) -> Self {
        self.expand_any = yes;
        self
    }

    /// Skip fields equal to their proto default. Default `false`.
    #[must_use]
    pub const fn skip_default_fields(mut self, yes: bool) -> Self {
        self.skip_default_fields = yes;
        self
    }

    /// Iterate fields in declaration order rather than tag-number order.
    /// Default `false`.
    #[must_use]
    pub const fn print_message_fields_in_index_order(mut self, yes: bool) -> Self {
        self.print_message_fields_in_index_order = yes;
        self
    }
}

pub(super) fn format_message(
    msg: &DynamicMessage,
    options: &FormatOptions,
    out: &mut String,
    indent: usize,
) {
    let descriptor = msg.descriptor();
    let mut fields: Vec<FieldDescriptor> = descriptor.fields().collect();
    if !options.print_message_fields_in_index_order {
        fields.sort_by_key(|f| f.number());
    }
    let mut first = true;
    for field in fields {
        // Only emit fields that are actually set.
        if !msg.has_field(&field) {
            continue;
        }
        if options.skip_default_fields {
            let v = msg.get_field(&field);
            let kind = field.kind();
            if v.is_default(&kind) {
                continue;
            }
        }
        let value = msg.get_field(&field);
        format_field(&field, value.as_ref(), options, out, indent, &mut first);
    }
    // unknown fields
    if !options.skip_unknown_fields {
        for (number, set) in msg.unknown_fields() {
            for entry in set.iter() {
                write_indent(out, options, indent, &mut first);
                let _ = write!(out, "{number}: ");
                let _ = write!(out, "{}", format_unknown(&entry.data));
                end_field(out, options);
            }
        }
    }
}

fn format_field(
    field: &FieldDescriptor,
    value: &Value,
    options: &FormatOptions,
    out: &mut String,
    indent: usize,
    first: &mut bool,
) {
    if field.is_list() {
        if let Value::List(items) = value {
            for item in items {
                write_indent(out, options, indent, first);
                let _ = write!(out, "{}", field.name());
                write_value(field, item, options, out, indent);
                end_field(out, options);
            }
        }
        return;
    }
    if field.is_map() {
        if let Value::Map(entries) = value {
            let mut sorted: Vec<_> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            for (k, v) in sorted {
                write_indent(out, options, indent, first);
                let _ = write!(out, "{} ", field.name());
                out.push('{');
                if options.pretty {
                    out.push('\n');
                    push_indent(out, indent + 1);
                }
                let _ = write!(out, "key: {}", format_map_key(k));
                if options.pretty {
                    out.push('\n');
                    push_indent(out, indent + 1);
                } else {
                    out.push(' ');
                }
                let kind = match crate::dynamic::value::map_entry_kinds(field) {
                    Some((_, vk)) => vk,
                    None => continue,
                };
                let _ = write!(out, "value: ");
                format_scalar_value(&kind, v, options, out, indent + 1);
                if options.pretty {
                    out.push('\n');
                    push_indent(out, indent);
                }
                out.push('}');
                end_field(out, options);
            }
        }
        return;
    }
    write_indent(out, options, indent, first);
    let _ = write!(out, "{}", field.name());
    write_value(field, value, options, out, indent);
    end_field(out, options);
}

fn write_value(
    field: &FieldDescriptor,
    value: &Value,
    options: &FormatOptions,
    out: &mut String,
    indent: usize,
) {
    match (&field.kind(), value) {
        (Kind::Message(_), Value::Message(m)) => {
            out.push(' ');
            out.push('{');
            if options.pretty {
                out.push('\n');
            }
            format_message(m, options, out, indent + 1);
            if options.pretty {
                push_indent(out, indent);
            }
            out.push('}');
        }
        (kind, _) => {
            let _ = write!(out, ": ");
            format_scalar_value(kind, value, options, out, indent);
        }
    }
}

fn format_scalar_value(
    kind: &Kind,
    value: &Value,
    options: &FormatOptions,
    out: &mut String,
    indent: usize,
) {
    match (kind, value) {
        (Kind::Bool, Value::Bool(b)) => {
            let _ = write!(out, "{}", if *b { "true" } else { "false" });
        }
        (Kind::Int32 | Kind::Sint32 | Kind::Sfixed32, Value::I32(v)) => {
            let _ = write!(out, "{v}");
        }
        (Kind::Uint32 | Kind::Fixed32, Value::U32(v)) => {
            let _ = write!(out, "{v}");
        }
        (Kind::Int64 | Kind::Sint64 | Kind::Sfixed64, Value::I64(v)) => {
            let _ = write!(out, "{v}");
        }
        (Kind::Uint64 | Kind::Fixed64, Value::U64(v)) => {
            let _ = write!(out, "{v}");
        }
        (Kind::Float, Value::F32(v)) => format_float_into(*v as f64, out),
        (Kind::Double, Value::F64(v)) => format_float_into(*v, out),
        (Kind::String, Value::String(s)) => {
            out.push('"');
            escape_into(s.as_bytes(), out, true);
            out.push('"');
        }
        (Kind::Bytes, Value::Bytes(b)) => {
            out.push('"');
            escape_into(b.as_ref(), out, false);
            out.push('"');
        }
        (Kind::Enum(d), Value::EnumNumber(n)) => match d.values().find(|v| v.number() == *n) {
            Some(v) => {
                let _ = write!(out, "{}", v.name());
            }
            None => {
                let _ = write!(out, "{n}");
            }
        },
        (Kind::Message(_), Value::Message(m)) => {
            out.push('{');
            if options.pretty {
                out.push('\n');
            }
            format_message(m, options, out, indent + 1);
            if options.pretty {
                push_indent(out, indent);
            }
            out.push('}');
        }
        _ => {
            let _ = write!(out, "/* mismatched */");
        }
    }
}

fn format_float_into(v: f64, out: &mut String) {
    if v.is_nan() {
        out.push_str("nan");
    } else if v == f64::INFINITY {
        out.push_str("inf");
    } else if v == f64::NEG_INFINITY {
        out.push_str("-inf");
    } else if v == v.trunc() {
        let _ = write!(out, "{v:.1}");
    } else {
        let _ = write!(out, "{v}");
    }
}

fn escape_into(bytes: &[u8], out: &mut String, _utf8_string: bool) {
    for &b in bytes {
        match b {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\'' => out.push_str("\\'"),
            0x20..=0x7e => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
}

fn format_map_key(key: &MapKey) -> String {
    match key {
        MapKey::Bool(b) => b.to_string(),
        MapKey::I32(v) => v.to_string(),
        MapKey::I64(v) => v.to_string(),
        MapKey::U32(v) => v.to_string(),
        MapKey::U64(v) => v.to_string(),
        MapKey::String(s) => format!("\"{s}\""),
    }
}

fn write_indent(out: &mut String, options: &FormatOptions, indent: usize, first: &mut bool) {
    if options.pretty {
        push_indent(out, indent);
    } else if !*first {
        out.push(' ');
    }
    *first = false;
}

fn end_field(out: &mut String, options: &FormatOptions) {
    if options.pretty {
        out.push('\n');
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn format_unknown(data: &buffa::unknown_fields::UnknownFieldData) -> String {
    use buffa::unknown_fields::UnknownFieldData;
    match data {
        UnknownFieldData::Varint(v) => v.to_string(),
        UnknownFieldData::Fixed64(v) => v.to_string(),
        UnknownFieldData::Fixed32(v) => v.to_string(),
        UnknownFieldData::LengthDelimited(b) => {
            let mut s = String::with_capacity(b.len() + 2);
            s.push('"');
            escape_into(b, &mut s, false);
            s.push('"');
            s
        }
        UnknownFieldData::Group(_) => "{ ... }".into(),
    }
}
