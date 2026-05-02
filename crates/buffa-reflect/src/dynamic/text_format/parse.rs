//! Textproto parser (logos-tokenized stream + recursive-descent).
//!
//! Handles scalars, repeated entries (one-per-line and `[a, b, c]`
//! short form), nested messages (`field { ... }`), maps (as repeated
//! entries with `key:` / `value:`), enum names and numbers, all C-style
//! escapes inside string/bytes literals, and `# to-EOL` comments.

use std::{collections::HashMap, fmt};

use buffa::bytes::Bytes;
use logos::{Lexer, Logos};

use crate::{
    dynamic::{
        message::DynamicMessage,
        value::{MapKey, Value},
    },
    field::{FieldDescriptor, Kind},
    message::MessageDescriptor,
};

/// Parse error raised by [`crate::DynamicMessage::parse_text_format`].
#[derive(Debug)]
pub struct ParseError {
    /// 1-based source line.
    pub line: u32,
    /// 1-based source column.
    pub col: u32,
    /// Underlying parser error kind.
    pub kind: ParseErrorKind,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {:?}", self.line, self.col, self.kind)
    }
}

impl std::error::Error for ParseError {}

/// Tagged reasons a textproto parse can fail.
#[derive(Debug)]
pub enum ParseErrorKind {
    /// Lexer-level error (invalid token).
    InvalidToken,
    /// Unexpected token where another was required.
    UnexpectedToken(String),
    /// Reached EOF before the parse was complete.
    UnexpectedEof,
    /// Field name didn't match anything on the descriptor.
    UnknownField(String),
    /// Schema-level rejection (type mismatch, invalid enum name, etc.).
    Schema(String),
}

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\r\n]+")]
#[logos(skip(r"#[^\n]*", allow_greedy = true))]
enum Token<'a> {
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice())]
    Ident(&'a str),

    #[regex(r"-?(?:0[xX][0-9a-fA-F]+|0[0-7]+|[0-9]+)", |lex| lex.slice())]
    Int(&'a str),

    // Floats: optional sign, integer, fractional, exponent.
    #[regex(r"-?[0-9]+\.[0-9]*([eE][-+]?[0-9]+)?", |lex| lex.slice())]
    #[regex(r"-?\.[0-9]+([eE][-+]?[0-9]+)?", |lex| lex.slice())]
    #[regex(r"-?[0-9]+[eE][-+]?[0-9]+", |lex| lex.slice())]
    Float(&'a str),

    // String literal — captured raw (including quotes); we decode escapes
    // separately in `decode_string`.
    #[regex(r#""([^"\\]|\\.)*""#, |lex| lex.slice())]
    #[regex(r#"'([^'\\]|\\.)*'"#, |lex| lex.slice())]
    StringLit(&'a str),

    #[token(":")]
    Colon,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("<")]
    LAngle,
    #[token(">")]
    RAngle,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[token(".")]
    Dot,
    #[token("/")]
    Slash,
}

struct Parser<'a> {
    src: &'a str,
    lex: Lexer<'a, Token<'a>>,
    peeked: Option<Token<'a>>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            lex: Token::lexer(src),
            peeked: None,
        }
    }

    fn line_col(&self) -> (u32, u32) {
        // Compute current 1-based line/col from byte offset.
        let span = self.lex.span();
        let pos = span.start.min(self.src.len());
        let mut line = 1u32;
        let mut col = 1u32;
        for ch in self.src[..pos].chars() {
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    fn err(&self, kind: ParseErrorKind) -> ParseError {
        let (line, col) = self.line_col();
        ParseError { line, col, kind }
    }

    fn peek(&mut self) -> Result<Option<&Token<'a>>, ParseError> {
        if self.peeked.is_some() {
            return Ok(self.peeked.as_ref());
        }
        match self.lex.next() {
            None => Ok(None),
            Some(Ok(tok)) => {
                self.peeked = Some(tok);
                Ok(self.peeked.as_ref())
            }
            Some(Err(_)) => Err(self.err(ParseErrorKind::InvalidToken)),
        }
    }

    fn next(&mut self) -> Result<Option<Token<'a>>, ParseError> {
        if self.peeked.is_some() {
            return Ok(self.peeked.take());
        }
        match self.lex.next() {
            None => Ok(None),
            Some(Ok(tok)) => Ok(Some(tok)),
            Some(Err(_)) => Err(self.err(ParseErrorKind::InvalidToken)),
        }
    }

    fn require(&mut self) -> Result<Token<'a>, ParseError> {
        self.next()?
            .ok_or_else(|| self.err(ParseErrorKind::UnexpectedEof))
    }

    fn expect(&mut self, expected: Token<'a>) -> Result<(), ParseError> {
        let actual = self.require()?;
        if actual == expected {
            Ok(())
        } else {
            Err(self.err(ParseErrorKind::UnexpectedToken(format!(
                "expected {expected:?}, got {actual:?}"
            ))))
        }
    }
}

/// Parser entry point.
pub(super) fn parse(
    descriptor: MessageDescriptor,
    input: &str,
) -> Result<DynamicMessage, ParseError> {
    let mut parser = Parser::new(input);
    let mut msg = DynamicMessage::new(descriptor.clone());
    parse_message_body(
        &mut parser,
        &descriptor,
        &mut msg,
        /* until_brace */ false,
    )?;
    Ok(msg)
}

fn parse_message_body(
    parser: &mut Parser<'_>,
    descriptor: &MessageDescriptor,
    msg: &mut DynamicMessage,
    until_brace: bool,
) -> Result<(), ParseError> {
    loop {
        let tok = match parser.peek()? {
            Some(t) => t.clone(),
            None => {
                if until_brace {
                    return Err(parser.err(ParseErrorKind::UnexpectedEof));
                }
                return Ok(());
            }
        };
        if until_brace && matches!(tok, Token::RBrace) {
            parser.next()?;
            return Ok(());
        }
        match tok {
            Token::Ident(name) => {
                parser.next()?;
                let field = descriptor
                    .get_field_by_name(name)
                    .or_else(|| descriptor.get_field_by_json_name(name))
                    .ok_or_else(|| parser.err(ParseErrorKind::UnknownField(name.to_string())))?;
                parse_field_value(parser, &field, msg)?;
            }
            Token::Semicolon => {
                parser.next()?;
            }
            other => {
                return Err(parser.err(ParseErrorKind::UnexpectedToken(format!(
                    "expected field name, got {other:?}"
                ))));
            }
        }
    }
}

fn parse_field_value(
    parser: &mut Parser<'_>,
    field: &FieldDescriptor,
    msg: &mut DynamicMessage,
) -> Result<(), ParseError> {
    // Optional `:` (required for scalars; optional before `{`).
    let next = parser.peek()?.cloned();
    let is_brace = matches!(next, Some(Token::LBrace) | Some(Token::LAngle));
    if !is_brace {
        parser.expect(Token::Colon)?;
    }

    if field.is_map() {
        return parse_map_entry(parser, field, msg);
    }
    if field.is_list() {
        // Repeated: either `[a, b, c]` short form or single value.
        let next = parser.peek()?;
        if let Some(Token::LBracket) = next {
            parser.next()?;
            let kind = field.kind();
            loop {
                let v = parse_singular_value(parser, &kind, &field.kind())?;
                push_list(msg, field, v);
                let sep = parser.next()?;
                match sep {
                    Some(Token::Comma) => continue,
                    Some(Token::RBracket) => break,
                    other => {
                        return Err(parser.err(ParseErrorKind::UnexpectedToken(format!(
                            "expected `,` or `]`, got {other:?}"
                        ))));
                    }
                }
            }
            return Ok(());
        }
        let kind = field.kind();
        let v = parse_singular_value(parser, &kind, &field.kind())?;
        push_list(msg, field, v);
        return Ok(());
    }

    let kind = field.kind();
    let v = parse_singular_value(parser, &kind, &field.kind())?;
    msg.try_set_field(field, v)
        .map_err(|e| parser.err(ParseErrorKind::Schema(e.to_string())))?;
    Ok(())
}

fn parse_map_entry(
    parser: &mut Parser<'_>,
    field: &FieldDescriptor,
    msg: &mut DynamicMessage,
) -> Result<(), ParseError> {
    let opener = parser.require()?;
    let closer = match opener {
        Token::LBrace => Token::RBrace,
        Token::LAngle => Token::RAngle,
        other => {
            return Err(parser.err(ParseErrorKind::UnexpectedToken(format!(
                "expected `{{` or `<`, got {other:?}"
            ))));
        }
    };
    let (key_kind, value_kind) = crate::dynamic::value::map_entry_kinds(field)
        .ok_or_else(|| parser.err(ParseErrorKind::Schema("not a map field".into())))?;
    let mut key: Option<Value> = None;
    let mut value: Option<Value> = None;
    loop {
        let next = parser.require()?;
        if next == closer {
            break;
        }
        let name = match next {
            Token::Ident(n) => n,
            other => {
                return Err(parser.err(ParseErrorKind::UnexpectedToken(format!(
                    "expected `key`/`value`, got {other:?}"
                ))));
            }
        };
        parser.expect(Token::Colon)?;
        match name {
            "key" => key = Some(parse_singular_value(parser, &key_kind, &key_kind)?),
            "value" => value = Some(parse_singular_value(parser, &value_kind, &value_kind)?),
            other => {
                return Err(parser.err(ParseErrorKind::UnknownField(other.to_string())));
            }
        }
    }
    let key = key.unwrap_or_else(|| Value::default_value(&key_kind));
    let value = value.unwrap_or_else(|| Value::default_value(&value_kind));
    let mk = value_to_map_key(&key)
        .ok_or_else(|| parser.err(ParseErrorKind::Schema("invalid map key".into())))?;

    if !msg.has_field(field) {
        msg.set_field(field, Value::Map(HashMap::new()));
    }
    if let Some(Value::Map(m)) = msg.get_field_by_name_mut(field.name()) {
        m.insert(mk, value);
    }
    Ok(())
}

fn value_to_map_key(value: &Value) -> Option<MapKey> {
    Some(match value {
        Value::Bool(b) => MapKey::Bool(*b),
        Value::I32(v) => MapKey::I32(*v),
        Value::I64(v) => MapKey::I64(*v),
        Value::U32(v) => MapKey::U32(*v),
        Value::U64(v) => MapKey::U64(*v),
        Value::String(s) => MapKey::String(s.clone()),
        _ => return None,
    })
}

fn push_list(msg: &mut DynamicMessage, field: &FieldDescriptor, v: Value) {
    if !msg.has_field(field) {
        msg.set_field(field, Value::List(Vec::new()));
    }
    if let Some(Value::List(l)) = msg.get_field_by_name_mut(field.name()) {
        l.push(v);
    }
}

fn parse_singular_value(
    parser: &mut Parser<'_>,
    kind: &Kind,
    _field_kind: &Kind,
) -> Result<Value, ParseError> {
    if let Kind::Message(d) = kind {
        let opener = parser.require()?;
        let closer = match opener {
            Token::LBrace => Token::RBrace,
            Token::LAngle => Token::RAngle,
            other => {
                return Err(parser.err(ParseErrorKind::UnexpectedToken(format!(
                    "expected `{{` or `<` for sub-message, got {other:?}"
                ))));
            }
        };
        let mut inner = DynamicMessage::new(d.clone());
        parse_message_until(parser, d, &mut inner, &closer)?;
        return Ok(Value::Message(inner));
    }
    let tok = parser.require()?;
    match (kind, tok) {
        (Kind::Bool, Token::Ident("true")) => Ok(Value::Bool(true)),
        (Kind::Bool, Token::Ident("false")) => Ok(Value::Bool(false)),
        (Kind::Int32 | Kind::Sint32 | Kind::Sfixed32, Token::Int(s)) => {
            Ok(Value::I32(parse_signed_int(s)? as i32))
        }
        (Kind::Int64 | Kind::Sint64 | Kind::Sfixed64, Token::Int(s)) => {
            Ok(Value::I64(parse_signed_int(s)?))
        }
        (Kind::Uint32 | Kind::Fixed32, Token::Int(s)) => {
            Ok(Value::U32(parse_unsigned_int(s)? as u32))
        }
        (Kind::Uint64 | Kind::Fixed64, Token::Int(s)) => Ok(Value::U64(parse_unsigned_int(s)?)),
        (Kind::Float | Kind::Double, Token::Int(s)) => {
            let v: f64 = s
                .parse()
                .map_err(|_| parser.err(ParseErrorKind::Schema(format!("invalid number `{s}`"))))?;
            if matches!(kind, Kind::Float) {
                Ok(Value::F32(v as f32))
            } else {
                Ok(Value::F64(v))
            }
        }
        (Kind::Float | Kind::Double, Token::Float(s)) => {
            let v: f64 = s
                .parse()
                .map_err(|_| parser.err(ParseErrorKind::Schema(format!("invalid float `{s}`"))))?;
            if matches!(kind, Kind::Float) {
                Ok(Value::F32(v as f32))
            } else {
                Ok(Value::F64(v))
            }
        }
        (Kind::Float | Kind::Double, Token::Ident(name)) => {
            let v: f64 = match name {
                "nan" | "NaN" => f64::NAN,
                "inf" | "infinity" | "Infinity" => f64::INFINITY,
                _ => {
                    return Err(parser.err(ParseErrorKind::Schema(format!(
                        "invalid float ident `{name}`"
                    ))));
                }
            };
            if matches!(kind, Kind::Float) {
                Ok(Value::F32(v as f32))
            } else {
                Ok(Value::F64(v))
            }
        }
        (Kind::String, Token::StringLit(raw)) => {
            let bytes = decode_string(raw).map_err(|e| parser.err(ParseErrorKind::Schema(e)))?;
            String::from_utf8(bytes)
                .map(Value::String)
                .map_err(|_| parser.err(ParseErrorKind::Schema("invalid UTF-8 in string".into())))
        }
        (Kind::Bytes, Token::StringLit(raw)) => {
            let bytes = decode_string(raw).map_err(|e| parser.err(ParseErrorKind::Schema(e)))?;
            Ok(Value::Bytes(Bytes::from(bytes)))
        }
        (Kind::Enum(d), Token::Ident(name)) => match d.values().find(|v| v.name() == name) {
            Some(v) => Ok(Value::EnumNumber(v.number())),
            None => Err(parser.err(ParseErrorKind::Schema(format!(
                "unknown enum variant `{name}`"
            )))),
        },
        (Kind::Enum(_), Token::Int(s)) => Ok(Value::EnumNumber(parse_signed_int(s)? as i32)),
        (kind, tok) => Err(parser.err(ParseErrorKind::UnexpectedToken(format!(
            "kind={kind:?} got {tok:?}"
        )))),
    }
}

fn parse_message_until(
    parser: &mut Parser<'_>,
    descriptor: &MessageDescriptor,
    msg: &mut DynamicMessage,
    closer: &Token<'_>,
) -> Result<(), ParseError> {
    loop {
        let tok = parser.peek()?.cloned();
        match tok {
            Some(t) if &t == closer => {
                parser.next()?;
                return Ok(());
            }
            Some(Token::Ident(name)) => {
                parser.next()?;
                let field = descriptor
                    .get_field_by_name(name)
                    .or_else(|| descriptor.get_field_by_json_name(name))
                    .ok_or_else(|| parser.err(ParseErrorKind::UnknownField(name.to_string())))?;
                parse_field_value(parser, &field, msg)?;
            }
            Some(Token::Semicolon) => {
                parser.next()?;
            }
            Some(other) => {
                return Err(parser.err(ParseErrorKind::UnexpectedToken(format!(
                    "expected field name, got {other:?}"
                ))));
            }
            None => return Err(parser.err(ParseErrorKind::UnexpectedEof)),
        }
    }
}

fn parse_signed_int(raw: &str) -> Result<i64, ParseError> {
    let (negative, body) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let v = parse_unsigned_int(body)? as i128;
    let v = if negative { -v } else { v };
    if v < i64::MIN as i128 || v > i64::MAX as i128 {
        return Err(ParseError {
            line: 0,
            col: 0,
            kind: ParseErrorKind::Schema(format!("integer out of range: {raw}")),
        });
    }
    Ok(v as i64)
}

fn parse_unsigned_int(raw: &str) -> Result<u64, ParseError> {
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_err(|e| ParseError {
            line: 0,
            col: 0,
            kind: ParseErrorKind::Schema(format!("invalid hex `{raw}`: {e}")),
        });
    }
    if raw.starts_with('0') && raw.len() > 1 && raw.chars().all(|c| c.is_ascii_digit()) {
        return u64::from_str_radix(&raw[1..], 8).map_err(|e| ParseError {
            line: 0,
            col: 0,
            kind: ParseErrorKind::Schema(format!("invalid octal `{raw}`: {e}")),
        });
    }
    raw.parse::<u64>().map_err(|e| ParseError {
        line: 0,
        col: 0,
        kind: ParseErrorKind::Schema(format!("invalid integer `{raw}`: {e}")),
    })
}

fn decode_string(raw: &str) -> Result<Vec<u8>, String> {
    // Strip leading/trailing quote (single or double).
    let bytes = raw.as_bytes();
    if bytes.len() < 2 {
        return Err("string literal too short".into());
    }
    let inner = &raw[1..raw.len() - 1];
    crate::dynamic::defaults::parse_default_value(inner, &crate::pool::KindRef::Bytes, None)
        .and_then(|v| match v {
            Value::Bytes(b) => Ok(b.to_vec()),
            _ => Err("internal: expected Bytes".into()),
        })
}
