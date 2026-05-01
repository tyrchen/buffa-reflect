//! Canonical view types + collectors for the two reflection
//! implementations.
//!
//! Both crates expose conceptually the same data, but with different
//! types. We project each into a tiny canonical form and diff that, so
//! the assertion is one-line per finding rather than ad-hoc field
//! comparisons across two type families.

#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use prost_reflect::{Cardinality as PCardinality, Kind as PKind};

/// Canonical type-token used for cross-impl Kind comparison.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum CanonicalKind {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes,
    Message(String),
    Enum(String),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CanonicalCardinality {
    Optional,
    Required,
    Repeated,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanonicalField {
    pub name: String,
    pub number: u32,
    pub json_name: String,
    pub kind: CanonicalKind,
    pub cardinality: CanonicalCardinality,
    pub supports_presence: bool,
    pub is_packed: bool,
    pub is_map: bool,
    /// Containing oneof name (synthetic oneofs included).
    pub containing_oneof: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanonicalOneof {
    pub name: String,
    pub is_synthetic: bool,
    /// Field names belonging to this oneof, in proto declaration order.
    pub field_names: Vec<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanonicalMessage {
    pub full_name: String,
    pub is_map_entry: bool,
    pub fields: Vec<CanonicalField>,
    pub oneofs: Vec<CanonicalOneof>,
    /// Parent message FQN (None for top-level).
    pub parent: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanonicalEnumValue {
    pub name: String,
    pub number: i32,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanonicalEnum {
    pub full_name: String,
    pub values: Vec<CanonicalEnumValue>,
    pub parent: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CanonicalFile {
    pub name: String,
    pub package: String,
    pub syntax: String,
}

#[derive(Debug, Clone)]
pub struct CanonicalView {
    pub files: BTreeMap<String, CanonicalFile>,
    pub messages: BTreeMap<String, CanonicalMessage>,
    pub enums: BTreeMap<String, CanonicalEnum>,
}

impl CanonicalView {
    pub fn diff(&self, other: &Self) -> Vec<String> {
        let mut out = Vec::new();

        // Files: same set of names; same per-file metadata.
        let lhs_files: Vec<_> = self.files.keys().collect();
        let rhs_files: Vec<_> = other.files.keys().collect();
        if lhs_files != rhs_files {
            out.push(format!(
                "file set mismatch:\n  prost: {lhs_files:?}\n  buffa: {rhs_files:?}"
            ));
        }
        for (name, lhs) in &self.files {
            if let Some(rhs) = other.files.get(name)
                && lhs != rhs
            {
                out.push(format!("file `{name}` differs: {lhs:?} vs {rhs:?}"));
            }
        }

        // Messages: same FQNs, same content per message.
        let lhs_msgs: Vec<_> = self.messages.keys().collect();
        let rhs_msgs: Vec<_> = other.messages.keys().collect();
        if lhs_msgs != rhs_msgs {
            let only_lhs: Vec<_> = lhs_msgs.iter().filter(|n| !other.messages.contains_key(**n)).collect();
            let only_rhs: Vec<_> = rhs_msgs.iter().filter(|n| !self.messages.contains_key(**n)).collect();
            out.push(format!(
                "message set mismatch:\n  only in prost: {only_lhs:?}\n  only in buffa: {only_rhs:?}"
            ));
        }
        for (name, lhs) in &self.messages {
            if let Some(rhs) = other.messages.get(name) {
                diff_message(name, lhs, rhs, &mut out);
            }
        }

        // Enums.
        let lhs_enums: Vec<_> = self.enums.keys().collect();
        let rhs_enums: Vec<_> = other.enums.keys().collect();
        if lhs_enums != rhs_enums {
            out.push(format!(
                "enum set mismatch:\n  prost: {lhs_enums:?}\n  buffa: {rhs_enums:?}"
            ));
        }
        for (name, lhs) in &self.enums {
            if let Some(rhs) = other.enums.get(name)
                && lhs != rhs
            {
                out.push(format!("enum `{name}` differs:\n  prost: {lhs:?}\n  buffa: {rhs:?}"));
            }
        }

        out
    }
}

fn diff_message(name: &str, lhs: &CanonicalMessage, rhs: &CanonicalMessage, out: &mut Vec<String>) {
    if lhs.is_map_entry != rhs.is_map_entry {
        out.push(format!(
            "message `{name}` is_map_entry differs: prost={} buffa={}",
            lhs.is_map_entry, rhs.is_map_entry,
        ));
    }
    if lhs.parent != rhs.parent {
        out.push(format!(
            "message `{name}` parent differs: prost={:?} buffa={:?}",
            lhs.parent, rhs.parent,
        ));
    }
    let lhs_fields: Vec<_> = lhs.fields.iter().map(|f| &f.name).collect();
    let rhs_fields: Vec<_> = rhs.fields.iter().map(|f| &f.name).collect();
    if lhs_fields != rhs_fields {
        out.push(format!(
            "message `{name}` field order/set differs:\n  prost: {lhs_fields:?}\n  buffa: {rhs_fields:?}"
        ));
        return;
    }
    for (l, r) in lhs.fields.iter().zip(rhs.fields.iter()) {
        if l != r {
            out.push(format!(
                "message `{name}` field `{}` differs:\n  prost: {l:?}\n  buffa: {r:?}",
                l.name
            ));
        }
    }
    let lhs_oneofs: Vec<_> = lhs.oneofs.iter().map(|o| &o.name).collect();
    let rhs_oneofs: Vec<_> = rhs.oneofs.iter().map(|o| &o.name).collect();
    if lhs_oneofs != rhs_oneofs {
        out.push(format!(
            "message `{name}` oneof set differs:\n  prost: {lhs_oneofs:?}\n  buffa: {rhs_oneofs:?}"
        ));
    } else {
        for (l, r) in lhs.oneofs.iter().zip(rhs.oneofs.iter()) {
            if l != r {
                out.push(format!(
                    "message `{name}` oneof `{}` differs:\n  prost: {l:?}\n  buffa: {r:?}",
                    l.name
                ));
            }
        }
    }
}

// ── prost-reflect collector ────────────────────────────────────────────

pub fn collect_prost(fds_bytes: &[u8]) -> Result<CanonicalView, Box<dyn std::error::Error>> {
    let pool = prost_reflect::DescriptorPool::decode(fds_bytes)?;

    let mut files = BTreeMap::new();
    for f in pool.files() {
        files.insert(
            f.name().to_string(),
            CanonicalFile {
                name: f.name().to_string(),
                package: f.package_name().to_string(),
                syntax: prost_syntax_str(f.syntax()).to_string(),
            },
        );
    }

    let mut messages = BTreeMap::new();
    for m in pool.all_messages() {
        let parent = m
            .parent_message()
            .map(|p| p.full_name().to_string());
        let oneofs: Vec<CanonicalOneof> = m
            .oneofs()
            .map(|o| CanonicalOneof {
                name: o.name().to_string(),
                // prost-reflect added is_synthetic in 0.16; some FQN-only
                // oneofs reflect proto3 optionals.
                is_synthetic: o.fields().count() == 1
                    && o.fields().next().is_some_and(|f| {
                        let fp = f.field_descriptor_proto();
                        fp.proto3_optional.unwrap_or(false)
                    }),
                field_names: o.fields().map(|f| f.name().to_string()).collect(),
            })
            .collect();

        let fields: Vec<CanonicalField> = m
            .fields()
            .map(|f| CanonicalField {
                name: f.name().to_string(),
                number: f.number(),
                json_name: f.json_name().to_string(),
                kind: prost_kind(&f.kind()),
                cardinality: prost_cardinality(f.cardinality()),
                supports_presence: f.supports_presence(),
                is_packed: f.is_packed(),
                is_map: f.is_map(),
                containing_oneof: f.containing_oneof().map(|o| o.name().to_string()),
            })
            .collect();

        messages.insert(
            m.full_name().to_string(),
            CanonicalMessage {
                full_name: m.full_name().to_string(),
                is_map_entry: m.is_map_entry(),
                fields,
                oneofs,
                parent,
            },
        );
    }

    let mut enums = BTreeMap::new();
    for e in pool.all_enums() {
        let parent = e
            .parent_message()
            .map(|p| p.full_name().to_string());
        enums.insert(
            e.full_name().to_string(),
            CanonicalEnum {
                full_name: e.full_name().to_string(),
                values: e
                    .values()
                    .map(|v| CanonicalEnumValue {
                        name: v.name().to_string(),
                        number: v.number(),
                    })
                    .collect(),
                parent,
            },
        );
    }

    Ok(CanonicalView {
        files,
        messages,
        enums,
    })
}

fn prost_kind(k: &PKind) -> CanonicalKind {
    match k {
        PKind::Double => CanonicalKind::Double,
        PKind::Float => CanonicalKind::Float,
        PKind::Int32 => CanonicalKind::Int32,
        PKind::Int64 => CanonicalKind::Int64,
        PKind::Uint32 => CanonicalKind::Uint32,
        PKind::Uint64 => CanonicalKind::Uint64,
        PKind::Sint32 => CanonicalKind::Sint32,
        PKind::Sint64 => CanonicalKind::Sint64,
        PKind::Fixed32 => CanonicalKind::Fixed32,
        PKind::Fixed64 => CanonicalKind::Fixed64,
        PKind::Sfixed32 => CanonicalKind::Sfixed32,
        PKind::Sfixed64 => CanonicalKind::Sfixed64,
        PKind::Bool => CanonicalKind::Bool,
        PKind::String => CanonicalKind::String,
        PKind::Bytes => CanonicalKind::Bytes,
        PKind::Message(m) => CanonicalKind::Message(m.full_name().to_string()),
        PKind::Enum(e) => CanonicalKind::Enum(e.full_name().to_string()),
    }
}

fn prost_cardinality(c: PCardinality) -> CanonicalCardinality {
    match c {
        PCardinality::Optional => CanonicalCardinality::Optional,
        PCardinality::Required => CanonicalCardinality::Required,
        PCardinality::Repeated => CanonicalCardinality::Repeated,
    }
}

fn prost_syntax_str(s: prost_reflect::Syntax) -> &'static str {
    match s {
        prost_reflect::Syntax::Proto2 => "proto2",
        prost_reflect::Syntax::Proto3 => "proto3",
    }
}

// ── buffa-reflect collector ────────────────────────────────────────────

pub fn collect_buffa(fds_bytes: &[u8]) -> Result<CanonicalView, Box<dyn std::error::Error>> {
    use buffa_reflect::Kind as BKind;

    let pool = buffa_reflect::DescriptorPool::decode(fds_bytes)?;

    let mut files = BTreeMap::new();
    for f in pool.files() {
        files.insert(
            f.name().to_string(),
            CanonicalFile {
                name: f.name().to_string(),
                package: f.package().to_string(),
                syntax: f.syntax().to_string(),
            },
        );
    }

    let mut messages = BTreeMap::new();
    for m in pool.all_messages() {
        let parent = m.parent_message().map(|p| p.full_name().to_string());
        let oneofs: Vec<CanonicalOneof> = m
            .oneofs()
            .map(|o| CanonicalOneof {
                name: o.name().to_string(),
                is_synthetic: o.is_synthetic(),
                field_names: o.fields().map(|f| f.name().to_string()).collect(),
            })
            .collect();

        let fields: Vec<CanonicalField> = m
            .fields()
            .map(|f| CanonicalField {
                name: f.name().to_string(),
                number: f.number(),
                json_name: f.json_name().to_string(),
                kind: match f.kind() {
                    BKind::Double => CanonicalKind::Double,
                    BKind::Float => CanonicalKind::Float,
                    BKind::Int32 => CanonicalKind::Int32,
                    BKind::Int64 => CanonicalKind::Int64,
                    BKind::Uint32 => CanonicalKind::Uint32,
                    BKind::Uint64 => CanonicalKind::Uint64,
                    BKind::Sint32 => CanonicalKind::Sint32,
                    BKind::Sint64 => CanonicalKind::Sint64,
                    BKind::Fixed32 => CanonicalKind::Fixed32,
                    BKind::Fixed64 => CanonicalKind::Fixed64,
                    BKind::Sfixed32 => CanonicalKind::Sfixed32,
                    BKind::Sfixed64 => CanonicalKind::Sfixed64,
                    BKind::Bool => CanonicalKind::Bool,
                    BKind::String => CanonicalKind::String,
                    BKind::Bytes => CanonicalKind::Bytes,
                    BKind::Message(md) => CanonicalKind::Message(md.full_name().to_string()),
                    BKind::Enum(ed) => CanonicalKind::Enum(ed.full_name().to_string()),
                    other => panic!("buffa-reflect added a Kind variant we don't know: {other:?}"),
                },
                cardinality: match f.cardinality() {
                    buffa_reflect::Cardinality::Optional => CanonicalCardinality::Optional,
                    buffa_reflect::Cardinality::Required => CanonicalCardinality::Required,
                    buffa_reflect::Cardinality::Repeated => CanonicalCardinality::Repeated,
                    other => panic!("buffa-reflect added a Cardinality variant: {other:?}"),
                },
                supports_presence: f.supports_presence(),
                is_packed: f.is_packed(),
                is_map: f.is_map(),
                containing_oneof: f.containing_oneof().map(|o| o.name().to_string()),
            })
            .collect();

        messages.insert(
            m.full_name().to_string(),
            CanonicalMessage {
                full_name: m.full_name().to_string(),
                is_map_entry: m.is_map_entry(),
                fields,
                oneofs,
                parent,
            },
        );
    }

    let mut enums = BTreeMap::new();
    for e in pool.all_enums() {
        enums.insert(
            e.full_name().to_string(),
            CanonicalEnum {
                full_name: e.full_name().to_string(),
                values: e
                    .values()
                    .map(|v| CanonicalEnumValue {
                        name: v.name().to_string(),
                        number: v.number(),
                    })
                    .collect(),
                parent: e.parent_message().map(|p| p.full_name().to_string()),
            },
        );
    }

    Ok(CanonicalView {
        files,
        messages,
        enums,
    })
}

// ── helpers ─────────────────────────────────────────────────────────────

pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto")
}

pub fn run_protoc(include_root: &Path, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let protoc = std::env::var("PROTOC").unwrap_or_else(|_| "protoc".to_string());
    let status = Command::new(&protoc)
        .arg("--include_imports")
        .arg("--include_source_info")
        .arg(format!("--descriptor_set_out={}", out.display()))
        .arg(format!("--proto_path={}", include_root.display()))
        .arg(include_root.join("acme/equiv/v1/zoo.proto"))
        .arg(include_root.join("acme/equiv/v1/neighbors.proto"))
        .status()?;
    if !status.success() {
        return Err(format!("protoc failed (status={status})").into());
    }
    Ok(())
}
