//! [`MessageDescriptor`] — handle to a single proto message in a pool.

use buffa_descriptor::generated::descriptor::DescriptorProto;

use crate::{
    field::FieldDescriptor,
    file::FileDescriptor,
    oneof::OneofDescriptor,
    pool::{DescriptorPool, MessageIndex},
};

/// Handle to one message in a [`DescriptorPool`].
#[derive(Clone, Debug)]
pub struct MessageDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) index: MessageIndex,
}

impl MessageDescriptor {
    /// Fully-qualified name (`<package>.<...>.<MessageName>`, no leading
    /// dot).
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.pool.inner.messages[self.index as usize].full_name
    }

    /// Leaf name (the part after the last `.`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.pool.inner.messages[self.index as usize].name
    }

    /// Owning file.
    #[must_use]
    pub fn parent_file(&self) -> FileDescriptor {
        let entry = &self.pool.inner.messages[self.index as usize];
        FileDescriptor {
            pool: self.pool.clone(),
            index: entry.file,
        }
    }

    /// Containing message, when this message is nested.
    #[must_use]
    pub fn parent_message(&self) -> Option<MessageDescriptor> {
        let entry = &self.pool.inner.messages[self.index as usize];
        entry.parent.map(|idx| MessageDescriptor {
            pool: self.pool.clone(),
            index: idx,
        })
    }

    /// All declared fields, in proto declaration order.
    pub fn fields(&self) -> impl ExactSizeIterator<Item = FieldDescriptor> + '_ {
        let pool = self.pool.clone();
        let owner = self.index;
        let len = self.pool.inner.messages[owner as usize].fields.len() as u32;
        (0..len).map(move |fi| FieldDescriptor {
            pool: pool.clone(),
            message: owner,
            index: fi,
        })
    }

    /// All oneof declarations, in declaration order. Includes synthetic
    /// oneofs introduced by proto3 `optional`; filter with
    /// [`OneofDescriptor::is_synthetic`] when only user-authored oneofs
    /// are wanted.
    pub fn oneofs(&self) -> impl ExactSizeIterator<Item = OneofDescriptor> + '_ {
        let pool = self.pool.clone();
        let owner = self.index;
        let len = self.pool.inner.messages[owner as usize].oneofs.len() as u32;
        (0..len).map(move |oi| OneofDescriptor {
            pool: pool.clone(),
            message: owner,
            index: oi,
        })
    }

    /// Lookup a field by its proto name.
    #[must_use]
    pub fn get_field_by_name(&self, name: &str) -> Option<FieldDescriptor> {
        let entry = &self.pool.inner.messages[self.index as usize];
        entry.by_name.get(name).map(|fi| FieldDescriptor {
            pool: self.pool.clone(),
            message: self.index,
            index: *fi,
        })
    }

    /// Lookup a field by its JSON name.
    #[must_use]
    pub fn get_field_by_json_name(&self, name: &str) -> Option<FieldDescriptor> {
        let entry = &self.pool.inner.messages[self.index as usize];
        entry.by_json_name.get(name).map(|fi| FieldDescriptor {
            pool: self.pool.clone(),
            message: self.index,
            index: *fi,
        })
    }

    /// Lookup a field by its tag number.
    #[must_use]
    pub fn get_field_by_number(&self, number: u32) -> Option<FieldDescriptor> {
        let entry = &self.pool.inner.messages[self.index as usize];
        entry.by_number.get(&number).map(|fi| FieldDescriptor {
            pool: self.pool.clone(),
            message: self.index,
            index: *fi,
        })
    }

    /// True iff this is a `map<K,V>` entry message synthesized by protoc.
    #[must_use]
    pub fn is_map_entry(&self) -> bool {
        self.pool.inner.messages[self.index as usize].is_map_entry
    }

    /// Raw [`DescriptorProto`] backing this descriptor (for source-info,
    /// proto2 default values, options, etc.).
    #[must_use]
    pub fn descriptor_proto(&self) -> &DescriptorProto {
        let entry = &self.pool.inner.messages[self.index as usize];
        let file = &self.pool.inner.files[entry.file as usize];
        crate::pool_build::resolve_message_proto(&file.proto, &entry.proto_path)
    }
}

impl PartialEq for MessageDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner) && self.index == other.index
    }
}

impl Eq for MessageDescriptor {}
