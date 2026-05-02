//! Eager parser for `FieldDescriptorProto::default_value` literals.
//!
//! Called from [`crate::pool_build`] at pool-construction time so any
//! malformed default surfaces as a [`crate::DescriptorError`] rather
//! than a surprise crash on first read. Mirrors prost-reflect's
//! `descriptor::build::resolve` parser.

use buffa::bytes::Bytes;

use crate::{
    dynamic::value::Value,
    pool::{EnumEntry, KindRef},
};

/// Parse a proto2 `[default = …]` literal against the field's resolved
/// kind.
///
/// Errors (a single human-readable message) are bubbled up by the
/// caller and aggregated into [`crate::DescriptorError`].
pub(crate) fn parse_default_value(
    raw: &str,
    kind: &KindRef,
    enum_entry: Option<&EnumEntry>,
) -> Result<Value, String> {
    match kind {
        KindRef::Bool => match raw {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            other => Err(format!("invalid bool default `{other}`")),
        },
        KindRef::Int32 | KindRef::Sint32 | KindRef::Sfixed32 => {
            parse_signed_int(raw).map(|v| Value::I32(v as i32))
        }
        KindRef::Int64 | KindRef::Sint64 | KindRef::Sfixed64 => {
            parse_signed_int(raw).map(Value::I64)
        }
        KindRef::Uint32 | KindRef::Fixed32 => parse_unsigned_int(raw).map(|v| Value::U32(v as u32)),
        KindRef::Uint64 | KindRef::Fixed64 => parse_unsigned_int(raw).map(Value::U64),
        KindRef::Float => parse_float(raw).map(|v| Value::F32(v as f32)),
        KindRef::Double => parse_float(raw).map(Value::F64),
        KindRef::String => Ok(Value::String(decode_c_escapes_str(raw))),
        KindRef::Bytes => decode_c_escapes_bytes(raw).map(|v| Value::Bytes(Bytes::from(v))),
        KindRef::Enum(_) => {
            let entry = enum_entry.ok_or_else(|| "enum default with no descriptor".to_string())?;
            // Try variant name first, then fall back to numeric.
            if let Some(idx) = entry.by_name.get(raw) {
                let v = &entry.values[*idx as usize];
                return Ok(Value::EnumNumber(v.number));
            }
            let n: i32 = raw
                .parse()
                .map_err(|_| format!("invalid enum default `{raw}` (no variant of that name)"))?;
            Ok(Value::EnumNumber(n))
        }
        KindRef::Message(_) => Err("messages cannot have default values".into()),
    }
}

fn parse_signed_int(raw: &str) -> Result<i64, String> {
    let raw = raw.trim();
    let (negative, body) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw.strip_prefix('+').unwrap_or(raw)),
    };
    let v = parse_unsigned_int(body)? as i128;
    let v = if negative { -v } else { v };
    if v < i64::MIN as i128 || v > i64::MAX as i128 {
        return Err(format!("default `{raw}` out of int64 range"));
    }
    Ok(v as i64)
}

fn parse_unsigned_int(raw: &str) -> Result<u64, String> {
    let raw = raw.trim();
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex `{raw}`: {e}"));
    }
    if raw.starts_with('0') && raw.len() > 1 && raw.chars().all(|c| c.is_ascii_digit()) {
        // Octal (per protobuf's default-value grammar).
        return u64::from_str_radix(&raw[1..], 8)
            .map_err(|e| format!("invalid octal `{raw}`: {e}"));
    }
    raw.parse::<u64>()
        .map_err(|e| format!("invalid integer `{raw}`: {e}"))
}

fn parse_float(raw: &str) -> Result<f64, String> {
    match raw {
        "nan" | "NaN" => Ok(f64::NAN),
        "inf" | "Infinity" | "+inf" | "+Infinity" => Ok(f64::INFINITY),
        "-inf" | "-Infinity" => Ok(f64::NEG_INFINITY),
        other => other
            .parse::<f64>()
            .map_err(|e| format!("invalid float `{raw}`: {e}")),
    }
}

fn decode_c_escapes_str(raw: &str) -> String {
    // For string defaults, the input is already a UTF-8 source literal;
    // protoc stores the decoded text verbatim.
    let bytes = decode_c_escapes_bytes(raw).unwrap_or_default();
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn decode_c_escapes_bytes(raw: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        // Escape sequence.
        i += 1;
        if i >= bytes.len() {
            return Err("default string ends in `\\`".into());
        }
        match bytes[i] {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'\\' => out.push(b'\\'),
            b'\'' => out.push(b'\''),
            b'"' => out.push(b'"'),
            b'?' => out.push(b'?'),
            b'0'..=b'7' => {
                // Up to 3 octal digits.
                let start = i;
                let mut end = i;
                while end < bytes.len() && end - start < 3 && (b'0'..=b'7').contains(&bytes[end]) {
                    end += 1;
                }
                let s = std::str::from_utf8(&bytes[start..end]).unwrap();
                let v = u32::from_str_radix(s, 8).unwrap();
                out.push(v as u8);
                i = end;
                continue;
            }
            b'x' | b'X' => {
                // Two hex digits (per C; protobuf documents 1-2 hex digits).
                i += 1;
                let start = i;
                let mut end = i;
                while end < bytes.len() && end - start < 2 && bytes[end].is_ascii_hexdigit() {
                    end += 1;
                }
                if end == start {
                    return Err("`\\x` with no hex digits".into());
                }
                let s = std::str::from_utf8(&bytes[start..end]).unwrap();
                let v = u32::from_str_radix(s, 16).unwrap();
                out.push(v as u8);
                i = end;
                continue;
            }
            b'u' => {
                // \uXXXX — 4 hex digits.
                i += 1;
                if i + 4 > bytes.len() {
                    return Err("`\\u` with fewer than 4 hex digits".into());
                }
                let s = std::str::from_utf8(&bytes[i..i + 4])
                    .map_err(|_| "`\\u` non-ASCII".to_string())?;
                let v = u32::from_str_radix(s, 16).map_err(|_| "`\\u` invalid hex".to_string())?;
                if let Some(c) = char::from_u32(v) {
                    let mut tmp = [0u8; 4];
                    let s = c.encode_utf8(&mut tmp);
                    out.extend_from_slice(s.as_bytes());
                }
                i += 4;
                continue;
            }
            b'U' => {
                // \UXXXXXXXX — 8 hex digits.
                i += 1;
                if i + 8 > bytes.len() {
                    return Err("`\\U` with fewer than 8 hex digits".into());
                }
                let s = std::str::from_utf8(&bytes[i..i + 8])
                    .map_err(|_| "`\\U` non-ASCII".to_string())?;
                let v = u32::from_str_radix(s, 16).map_err(|_| "`\\U` invalid hex".to_string())?;
                if let Some(c) = char::from_u32(v) {
                    let mut tmp = [0u8; 4];
                    let s = c.encode_utf8(&mut tmp);
                    out.extend_from_slice(s.as_bytes());
                }
                i += 8;
                continue;
            }
            other => return Err(format!("unknown escape `\\{}`", other as char)),
        }
        i += 1;
    }
    Ok(out)
}
