//! Two-pass pool builder: walk a `FileDescriptorSet` to register every
//! message and enum, then resolve every field's `type_name` against the
//! finished name table.

use buffa_descriptor::generated::descriptor::{
    DescriptorProto, EnumDescriptorProto, FieldDescriptorProto, FileDescriptorProto,
    FileDescriptorSet, OneofDescriptorProto,
    field_descriptor_proto::{Label, Type},
};

use crate::{
    error::DescriptorError,
    field::Cardinality,
    pool::{
        Definition, EnumEntry, EnumIndex, EnumValueEntry, FieldEntry, FileEntry, FileIndex,
        KindRef, MessageEntry, MessageIndex, OneofEntry, PoolInner,
    },
};

/// Largest legal protobuf field number.
const MAX_FIELD_NUMBER: u32 = 536_870_911;
/// Reserved range internal to the protobuf implementation.
const RESERVED_RANGE: std::ops::RangeInclusive<u32> = 19_000..=19_999;

/// Top-level entry point used by `DescriptorPool::add_file_descriptor_set`.
pub(crate) fn ingest_file_descriptor_set(
    pool: &mut PoolInner,
    fds: FileDescriptorSet,
) -> Result<(), DescriptorError> {
    // Pass 1 — register every name (message / enum / nested message / nested
    // enum) so that field type resolution in pass 2 can look them up.
    for file_proto in fds.file {
        let file_index = u32::try_from(pool.files.len())
            .map_err(|_| DescriptorError::Validation("too many files in pool (>= 2^32)".into()))?;
        let file_name = file_proto
            .name
            .clone()
            .ok_or(DescriptorError::MissingFileName)?;
        if pool.file_names.contains_key(file_name.as_str()) {
            return Err(DescriptorError::DuplicateFile(file_name));
        }

        let package = file_proto.package.as_deref().unwrap_or("").to_string();

        let mut top_messages = Vec::with_capacity(file_proto.message_type.len());
        for (i, msg_proto) in file_proto.message_type.iter().enumerate() {
            let i = u32::try_from(i).map_err(|_| {
                DescriptorError::Validation("too many top-level messages in file".into())
            })?;
            let path = vec![i];
            let idx = register_message(pool, msg_proto, &package, None, file_index, path)?;
            top_messages.push(idx);
        }

        let mut top_enums = Vec::with_capacity(file_proto.enum_type.len());
        for (i, enum_proto) in file_proto.enum_type.iter().enumerate() {
            let i = u32::try_from(i).map_err(|_| {
                DescriptorError::Validation("too many top-level enums in file".into())
            })?;
            let path = vec![i];
            let idx = register_enum(pool, enum_proto, &package, None, file_index, path)?;
            top_enums.push(idx);
        }

        pool.file_names
            .insert(file_name.clone().into_boxed_str(), file_index);
        pool.files.push(FileEntry {
            proto: file_proto,
            messages: top_messages,
            enums: top_enums,
        });
    }

    // Pass 2 — resolve every field's `type_name` and validate field numbers,
    // oneofs, etc.
    let total_messages = pool.messages.len();
    for msg_index in 0..total_messages {
        resolve_message(pool, msg_index as MessageIndex)?;
    }
    let total_enums = pool.enums.len();
    for enum_index in 0..total_enums {
        validate_enum(pool, enum_index as EnumIndex)?;
    }

    Ok(())
}

/// Recursively register a message + its nested types (without resolving
/// field types yet).
fn register_message(
    pool: &mut PoolInner,
    proto: &DescriptorProto,
    parent_scope: &str,
    parent: Option<MessageIndex>,
    file: FileIndex,
    proto_path: Vec<u32>,
) -> Result<MessageIndex, DescriptorError> {
    let name = proto
        .name
        .clone()
        .ok_or_else(|| DescriptorError::MissingName {
            location: parent_scope.to_string(),
        })?;
    let full_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}.{name}")
    };

    let index = u32::try_from(pool.messages.len())
        .map_err(|_| DescriptorError::Validation("too many messages in pool (>= 2^32)".into()))?;
    if pool
        .names
        .insert(
            full_name.clone().into_boxed_str(),
            Definition::Message(index),
        )
        .is_some()
    {
        return Err(DescriptorError::DuplicateType(full_name));
    }

    let is_map_entry = proto
        .options
        .as_option()
        .and_then(|o| o.map_entry)
        .unwrap_or(false);

    pool.messages.push(MessageEntry {
        full_name: full_name.clone().into_boxed_str(),
        name: name.into_boxed_str(),
        file,
        parent,
        proto_path: proto_path.clone(),
        fields: Vec::new(),
        oneofs: Vec::new(),
        nested_messages: Vec::new(),
        nested_enums: Vec::new(),
        by_number: hashbrown::HashMap::new(),
        by_name: hashbrown::HashMap::new(),
        by_json_name: hashbrown::HashMap::new(),
        is_map_entry,
    });

    let mut nested_messages = Vec::with_capacity(proto.nested_type.len());
    for (i, nested_proto) in proto.nested_type.iter().enumerate() {
        let i = u32::try_from(i)
            .map_err(|_| DescriptorError::Validation("too many nested messages".into()))?;
        let mut child_path = proto_path.clone();
        child_path.push(i);
        let nested_index = register_message(
            pool,
            nested_proto,
            &full_name,
            Some(index),
            file,
            child_path,
        )?;
        nested_messages.push(nested_index);
    }

    let mut nested_enums = Vec::with_capacity(proto.enum_type.len());
    for (i, nested_proto) in proto.enum_type.iter().enumerate() {
        let i = u32::try_from(i)
            .map_err(|_| DescriptorError::Validation("too many nested enums".into()))?;
        let mut child_path = proto_path.clone();
        child_path.push(i);
        let nested_index = register_enum(
            pool,
            nested_proto,
            &full_name,
            Some(index),
            file,
            child_path,
        )?;
        nested_enums.push(nested_index);
    }

    pool.messages[index as usize].nested_messages = nested_messages;
    pool.messages[index as usize].nested_enums = nested_enums;
    Ok(index)
}

fn register_enum(
    pool: &mut PoolInner,
    proto: &EnumDescriptorProto,
    parent_scope: &str,
    parent: Option<MessageIndex>,
    file: FileIndex,
    proto_path: Vec<u32>,
) -> Result<EnumIndex, DescriptorError> {
    let name = proto
        .name
        .clone()
        .ok_or_else(|| DescriptorError::MissingName {
            location: parent_scope.to_string(),
        })?;
    let full_name = if parent_scope.is_empty() {
        name.clone()
    } else {
        format!("{parent_scope}.{name}")
    };

    let index = u32::try_from(pool.enums.len())
        .map_err(|_| DescriptorError::Validation("too many enums in pool (>= 2^32)".into()))?;
    if pool
        .names
        .insert(full_name.clone().into_boxed_str(), Definition::Enum(index))
        .is_some()
    {
        return Err(DescriptorError::DuplicateType(full_name));
    }

    let mut values = Vec::with_capacity(proto.value.len());
    let mut by_name = hashbrown::HashMap::with_capacity(proto.value.len());
    let mut by_number: hashbrown::HashMap<i32, u32> = hashbrown::HashMap::new();
    for (i, v) in proto.value.iter().enumerate() {
        let value_name = v.name.clone().ok_or_else(|| DescriptorError::MissingName {
            location: full_name.clone(),
        })?;
        let value_number = v.number.ok_or_else(|| {
            DescriptorError::Validation(format!(
                "enum `{full_name}` value `{value_name}` is missing a number"
            ))
        })?;
        let value_full_name = format!("{full_name}.{value_name}");
        let pos = u32::try_from(i)
            .map_err(|_| DescriptorError::Validation("too many enum values".into()))?;
        if by_name
            .insert(value_name.clone().into_boxed_str(), pos)
            .is_some()
        {
            return Err(DescriptorError::DuplicateType(value_full_name));
        }
        by_number.entry(value_number).or_insert(pos);
        values.push(EnumValueEntry {
            name: value_name.into_boxed_str(),
            full_name: value_full_name.into_boxed_str(),
            number: value_number,
        });
    }

    pool.enums.push(EnumEntry {
        full_name: full_name.into_boxed_str(),
        name: name.into_boxed_str(),
        file,
        parent,
        values,
        proto_path,
        by_name,
        by_number,
    });
    Ok(index)
}

fn resolve_message(pool: &mut PoolInner, index: MessageIndex) -> Result<(), DescriptorError> {
    // Snapshot what we need from the proto without holding a borrow on
    // `pool` (resolution updates `pool.messages[index]`).
    let (file_index, proto_path) = {
        let entry = &pool.messages[index as usize];
        (entry.file, entry.proto_path.clone())
    };
    let file_proto = &pool.files[file_index as usize].proto;
    let proto = resolve_message_proto(file_proto, &proto_path).clone();
    let parent_full_name = pool.messages[index as usize].full_name.to_string();
    let syntax = file_proto.syntax.as_deref().unwrap_or("proto2").to_string();

    // Pre-build oneof entries (synthetic flag is filled later when fields
    // are walked).
    let mut oneofs: Vec<OneofEntry> = proto
        .oneof_decl
        .iter()
        .enumerate()
        .map(|(i, o)| build_oneof_entry(o, &parent_full_name, i as u32))
        .collect::<Result<Vec<_>, _>>()?;

    let mut fields = Vec::with_capacity(proto.field.len());
    let mut by_number: hashbrown::HashMap<u32, u32> =
        hashbrown::HashMap::with_capacity(proto.field.len());
    let mut by_name: hashbrown::HashMap<Box<str>, u32> =
        hashbrown::HashMap::with_capacity(proto.field.len());
    let mut by_json_name: hashbrown::HashMap<Box<str>, u32> =
        hashbrown::HashMap::with_capacity(proto.field.len());

    for (i, field_proto) in proto.field.iter().enumerate() {
        let field_pos = u32::try_from(i)
            .map_err(|_| DescriptorError::Validation("too many fields in message".into()))?;
        let entry = build_field_entry(
            pool,
            file_proto,
            &parent_full_name,
            &syntax,
            field_proto,
            field_pos,
        )?;

        if by_number.insert(entry.number, field_pos).is_some() {
            return Err(DescriptorError::Validation(format!(
                "duplicate field number {} in `{parent_full_name}`",
                entry.number
            )));
        }
        if by_name.insert(entry.name.clone(), field_pos).is_some() {
            return Err(DescriptorError::DuplicateType(format!(
                "{parent_full_name}.{}",
                entry.name
            )));
        }
        // JSON name collisions are not strictly an error in protobuf
        // (different fields can share a json_name in theory) — we keep the
        // first registration so lookup is deterministic.
        by_json_name
            .entry(entry.json_name.clone())
            .or_insert(field_pos);

        if let Some(oi) = entry.oneof_index {
            let count = oneofs.len();
            let oneof =
                oneofs
                    .get_mut(oi as usize)
                    .ok_or_else(|| DescriptorError::InvalidOneofIndex {
                        field: format!("{parent_full_name}.{}", entry.name),
                        index: oi as i32,
                        count,
                    })?;
            oneof.field_indices.push(field_pos);
        }

        fields.push(entry);
    }

    // Mark synthetic oneofs: any oneof whose sole member is a proto3
    // optional field.
    for oneof in &mut oneofs {
        if oneof.field_indices.len() == 1 {
            let fi = oneof.field_indices[0];
            let fproto = &proto.field[fi as usize];
            if fproto.proto3_optional.unwrap_or(false) {
                oneof.is_synthetic = true;
            }
        }
    }

    let entry = &mut pool.messages[index as usize];
    entry.fields = fields;
    entry.oneofs = oneofs;
    entry.by_number = by_number;
    entry.by_name = by_name;
    entry.by_json_name = by_json_name;

    Ok(())
}

fn build_oneof_entry(
    proto: &OneofDescriptorProto,
    message_full_name: &str,
    proto_index: u32,
) -> Result<OneofEntry, DescriptorError> {
    let name = proto
        .name
        .clone()
        .ok_or_else(|| DescriptorError::MissingName {
            location: message_full_name.to_string(),
        })?;
    let full_name = format!("{message_full_name}.{name}");
    Ok(OneofEntry {
        name: name.into_boxed_str(),
        full_name: full_name.into_boxed_str(),
        is_synthetic: false,
        field_indices: Vec::new(),
        proto_index,
    })
}

fn build_field_entry(
    pool: &PoolInner,
    file_proto: &FileDescriptorProto,
    message_full_name: &str,
    syntax: &str,
    proto: &FieldDescriptorProto,
    proto_field_index: u32,
) -> Result<FieldEntry, DescriptorError> {
    let name = proto
        .name
        .clone()
        .ok_or_else(|| DescriptorError::MissingName {
            location: message_full_name.to_string(),
        })?;
    let full_name = format!("{message_full_name}.{name}");
    let number = proto.number.ok_or_else(|| {
        DescriptorError::Validation(format!("field `{full_name}` is missing a number"))
    })?;
    if number <= 0 {
        return Err(DescriptorError::InvalidFieldNumber {
            message: message_full_name.to_string(),
            number,
            max: MAX_FIELD_NUMBER,
        });
    }
    let number_u = number as u32;
    if number_u > MAX_FIELD_NUMBER || RESERVED_RANGE.contains(&number_u) {
        return Err(DescriptorError::InvalidFieldNumber {
            message: message_full_name.to_string(),
            number,
            max: MAX_FIELD_NUMBER,
        });
    }

    let json_name = match proto.json_name.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => json_name_from_proto(&name),
    };

    let cardinality = match proto.label {
        Some(Label::LABEL_OPTIONAL) | None => Cardinality::Optional,
        Some(Label::LABEL_REQUIRED) => Cardinality::Required,
        Some(Label::LABEL_REPEATED) => Cardinality::Repeated,
    };

    // proto3 forbids `LABEL_REQUIRED`. Editions explicitly allow it via
    // `field_presence = LEGACY_REQUIRED`, so we only enforce in proto3.
    if matches!(cardinality, Cardinality::Required) && syntax == "proto3" {
        return Err(DescriptorError::Proto3RequiredField {
            field: full_name.clone(),
        });
    }

    let kind = resolve_kind(pool, file_proto, message_full_name, &full_name, proto)?;

    let oneof_index = proto.oneof_index.map(|i| i as u32);
    let supports_presence = compute_supports_presence(syntax, &cardinality, &kind, proto);
    let is_packed = compute_is_packed(syntax, &cardinality, &kind, proto);

    #[cfg(feature = "dynamic")]
    let parsed_default = parse_default_for_field(pool, &full_name, &cardinality, &kind, proto)?;

    Ok(FieldEntry {
        name: name.into_boxed_str(),
        full_name: full_name.into_boxed_str(),
        json_name: json_name.into_boxed_str(),
        number: number_u,
        kind,
        cardinality,
        supports_presence,
        is_packed,
        oneof_index,
        proto_field_index,
        #[cfg(feature = "dynamic")]
        parsed_default,
    })
}

#[cfg(feature = "dynamic")]
fn parse_default_for_field(
    pool: &PoolInner,
    field_full_name: &str,
    cardinality: &Cardinality,
    kind: &KindRef,
    proto: &FieldDescriptorProto,
) -> Result<Option<crate::dynamic::Value>, DescriptorError> {
    let raw = match proto.default_value.as_deref() {
        Some(s) => s,
        None => return Ok(None),
    };
    if matches!(cardinality, Cardinality::Repeated) {
        // The descriptor format allows repeated fields to declare a
        // (single) default; we ignore it for repeated semantics.
        return Ok(None);
    }
    let enum_entry = if let KindRef::Enum(idx) = kind {
        Some(&pool.enums[*idx as usize])
    } else {
        None
    };
    crate::dynamic::defaults::parse_default_value(raw, kind, enum_entry)
        .map(Some)
        .map_err(|message| DescriptorError::InvalidDefaultValue {
            field: field_full_name.to_string(),
            value: raw.to_string(),
            message,
        })
}

fn compute_supports_presence(
    syntax: &str,
    cardinality: &Cardinality,
    kind: &KindRef,
    proto: &FieldDescriptorProto,
) -> bool {
    if matches!(cardinality, Cardinality::Repeated) {
        return false;
    }
    if proto.oneof_index.is_some() {
        return true;
    }
    if matches!(kind, KindRef::Message(_)) {
        return true;
    }
    match syntax {
        "proto3" => proto.proto3_optional.unwrap_or(false),
        // proto2 + editions both track presence on every singular field.
        _ => true,
    }
}

fn compute_is_packed(
    syntax: &str,
    cardinality: &Cardinality,
    kind: &KindRef,
    proto: &FieldDescriptorProto,
) -> bool {
    if !matches!(cardinality, Cardinality::Repeated) {
        return false;
    }
    if !is_packable_kind(kind) {
        return false;
    }
    if let Some(opts) = proto.options.as_option()
        && let Some(packed) = opts.packed
    {
        return packed;
    }
    // proto3 packs by default; proto2 does not. Editions inherits proto2
    // semantics unless the `repeated_field_encoding` feature flips it,
    // which we cannot determine from the descriptor without resolving
    // FeatureSet defaults — stay conservative and treat unset as proto2.
    syntax == "proto3"
}

fn is_packable_kind(kind: &KindRef) -> bool {
    matches!(
        kind,
        KindRef::Double
            | KindRef::Float
            | KindRef::Int32
            | KindRef::Int64
            | KindRef::Uint32
            | KindRef::Uint64
            | KindRef::Sint32
            | KindRef::Sint64
            | KindRef::Fixed32
            | KindRef::Fixed64
            | KindRef::Sfixed32
            | KindRef::Sfixed64
            | KindRef::Bool
            | KindRef::Enum(_)
    )
}

fn resolve_kind(
    pool: &PoolInner,
    file_proto: &FileDescriptorProto,
    message_full_name: &str,
    field_full_name: &str,
    proto: &FieldDescriptorProto,
) -> Result<KindRef, DescriptorError> {
    if let Some(t) = proto.r#type {
        return match t {
            Type::TYPE_DOUBLE => Ok(KindRef::Double),
            Type::TYPE_FLOAT => Ok(KindRef::Float),
            Type::TYPE_INT64 => Ok(KindRef::Int64),
            Type::TYPE_UINT64 => Ok(KindRef::Uint64),
            Type::TYPE_INT32 => Ok(KindRef::Int32),
            Type::TYPE_FIXED64 => Ok(KindRef::Fixed64),
            Type::TYPE_FIXED32 => Ok(KindRef::Fixed32),
            Type::TYPE_BOOL => Ok(KindRef::Bool),
            Type::TYPE_STRING => Ok(KindRef::String),
            Type::TYPE_BYTES => Ok(KindRef::Bytes),
            Type::TYPE_UINT32 => Ok(KindRef::Uint32),
            Type::TYPE_SFIXED32 => Ok(KindRef::Sfixed32),
            Type::TYPE_SFIXED64 => Ok(KindRef::Sfixed64),
            Type::TYPE_SINT32 => Ok(KindRef::Sint32),
            Type::TYPE_SINT64 => Ok(KindRef::Sint64),
            Type::TYPE_GROUP | Type::TYPE_MESSAGE => resolve_named_kind(
                pool,
                file_proto,
                message_full_name,
                field_full_name,
                proto,
                NamedKind::Message,
            ),
            Type::TYPE_ENUM => resolve_named_kind(
                pool,
                file_proto,
                message_full_name,
                field_full_name,
                proto,
                NamedKind::Enum,
            ),
        };
    }
    // Some descriptors omit `type` for message/enum fields and rely on
    // `type_name` alone — try to recover.
    if proto.type_name.is_some() {
        return resolve_named_kind(
            pool,
            file_proto,
            message_full_name,
            field_full_name,
            proto,
            NamedKind::Either,
        );
    }
    Err(DescriptorError::MissingFieldType {
        field: field_full_name.to_string(),
    })
}

#[derive(Copy, Clone)]
enum NamedKind {
    Message,
    Enum,
    Either,
}

fn resolve_named_kind(
    pool: &PoolInner,
    file_proto: &FileDescriptorProto,
    message_full_name: &str,
    field_full_name: &str,
    proto: &FieldDescriptorProto,
    expect: NamedKind,
) -> Result<KindRef, DescriptorError> {
    let raw = proto
        .type_name
        .as_deref()
        .ok_or_else(|| DescriptorError::MissingTypeName {
            field: field_full_name.to_string(),
            kind: match expect {
                NamedKind::Message => "TYPE_MESSAGE",
                NamedKind::Enum => "TYPE_ENUM",
                NamedKind::Either => "TYPE_MESSAGE_OR_ENUM",
            },
        })?;

    let resolved =
        resolve_type_name(pool, file_proto, message_full_name, raw).ok_or_else(|| {
            DescriptorError::UnresolvedType {
                field: field_full_name.to_string(),
                type_name: raw.to_string(),
            }
        })?;

    match (expect, resolved) {
        (NamedKind::Message | NamedKind::Either, Definition::Message(idx)) => {
            Ok(KindRef::Message(idx))
        }
        (NamedKind::Enum | NamedKind::Either, Definition::Enum(idx)) => Ok(KindRef::Enum(idx)),
        (NamedKind::Message, Definition::Enum(_)) | (NamedKind::Enum, Definition::Message(_)) => {
            Err(DescriptorError::Validation(format!(
                "field `{field_full_name}` expected {} but `{raw}` resolved to a different kind",
                match expect {
                    NamedKind::Message => "a message",
                    NamedKind::Enum => "an enum",
                    NamedKind::Either => unreachable!(),
                }
            )))
        }
    }
}

/// Walk a [`FileDescriptorProto`] to a nested `DescriptorProto` by index
/// path. Each step is an index into `message_type` (root) or `nested_type`
/// (recursion).
pub(crate) fn resolve_message_proto<'a>(
    file: &'a FileDescriptorProto,
    path: &[u32],
) -> &'a DescriptorProto {
    // `path` is always non-empty by construction in `register_message`
    // (the call site pushes the top-level index before recursing). We
    // express that here by indexing rather than `expect`, since a panic
    // from `[0]` carries the same meaning without a stringly-typed
    // assertion.
    let mut cur = &file.message_type[path[0] as usize];
    for step in &path[1..] {
        cur = &cur.nested_type[*step as usize];
    }
    cur
}

/// Same as [`resolve_message_proto`] but for enums. The first path
/// component selects from `file.enum_type` if `path.len() == 1`, otherwise
/// from `file.message_type[path[0]].nested_type[path[1]]…enum_type[last]`.
pub(crate) fn resolve_enum_proto<'a>(
    file: &'a FileDescriptorProto,
    path: &[u32],
) -> &'a EnumDescriptorProto {
    if path.len() == 1 {
        return &file.enum_type[path[0] as usize];
    }
    let (msg_path, last) = path.split_at(path.len() - 1);
    let owning = resolve_message_proto(file, msg_path);
    &owning.enum_type[last[0] as usize]
}

/// Resolve a protobuf `type_name` (which may be fully-qualified with a
/// leading dot or relative) using the C++ scoping rules: search the
/// containing scope's nested namespace first, then walk outward.
fn resolve_type_name(
    pool: &PoolInner,
    _file: &FileDescriptorProto,
    scope_full_name: &str,
    type_name: &str,
) -> Option<Definition> {
    if let Some(rest) = type_name.strip_prefix('.') {
        return pool.names.get(rest).copied();
    }
    let mut scope = scope_full_name.to_string();
    loop {
        let candidate = if scope.is_empty() {
            type_name.to_string()
        } else {
            format!("{scope}.{type_name}")
        };
        if let Some(def) = pool.names.get(candidate.as_str()) {
            return Some(*def);
        }
        if scope.is_empty() {
            return None;
        }
        match scope.rsplit_once('.') {
            Some((head, _)) => scope.truncate(head.len()),
            None => scope.clear(),
        }
    }
}

fn validate_enum(pool: &PoolInner, index: EnumIndex) -> Result<(), DescriptorError> {
    let entry = &pool.enums[index as usize];
    let file = &pool.files[entry.file as usize].proto;
    if file.syntax.as_deref() == Some("proto3") {
        let has_zero = entry.values.iter().any(|v| v.number == 0);
        if !has_zero {
            return Err(DescriptorError::Proto3EnumMissingZero(
                entry.full_name.to_string(),
            ));
        }
    }
    Ok(())
}

/// Translate a snake_case proto name to lowerCamelCase for JSON.
pub(crate) fn json_name_from_proto(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for u in ch.to_uppercase() {
                out.push(u);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}
