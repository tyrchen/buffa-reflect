//! [`OneofDescriptor`] — handle to a oneof declaration on a message.

use buffa_descriptor::generated::descriptor::OneofDescriptorProto;

use crate::{
    field::FieldDescriptor,
    message::MessageDescriptor,
    pool::{DescriptorPool, MessageIndex},
};

/// Handle to one oneof declaration in a [`MessageDescriptor`].
#[derive(Clone, Debug)]
pub struct OneofDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) message: MessageIndex,
    pub(crate) index: u32,
}

impl OneofDescriptor {
    fn entry(&self) -> &crate::pool::OneofEntry {
        &self.pool.inner.messages[self.message as usize].oneofs[self.index as usize]
    }

    /// Oneof name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.entry().name
    }

    /// `<message.full_name>.<oneof_name>`, no leading dot.
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.entry().full_name
    }

    /// True iff this oneof was synthesized for a proto3 `optional` field.
    /// Synthetic oneofs are part of the descriptor model but do not generate
    /// any user-facing oneof Rust enum.
    #[must_use]
    pub fn is_synthetic(&self) -> bool {
        self.entry().is_synthetic
    }

    /// Owning message.
    #[must_use]
    pub fn parent_message(&self) -> MessageDescriptor {
        MessageDescriptor {
            pool: self.pool.clone(),
            index: self.message,
        }
    }

    /// Iterate over the fields belonging to this oneof.
    pub fn fields(&self) -> impl ExactSizeIterator<Item = FieldDescriptor> + '_ {
        let pool = self.pool.clone();
        let owner = self.message;
        let entry = self.entry();
        entry
            .field_indices
            .clone()
            .into_iter()
            .map(move |fi| FieldDescriptor {
                pool: pool.clone(),
                message: owner,
                index: fi,
            })
    }

    /// Raw [`OneofDescriptorProto`] for advanced use.
    #[must_use]
    pub fn descriptor_proto(&self) -> &OneofDescriptorProto {
        let msg_entry = &self.pool.inner.messages[self.message as usize];
        let file = &self.pool.inner.files[msg_entry.file as usize];
        let msg_proto =
            crate::pool_build::resolve_message_proto(&file.proto, &msg_entry.proto_path);
        &msg_proto.oneof_decl[self.entry().proto_index as usize]
    }
}

impl PartialEq for OneofDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner)
            && self.message == other.message
            && self.index == other.index
    }
}

impl Eq for OneofDescriptor {}
