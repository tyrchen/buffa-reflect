//! [`FileDescriptor`] — handle to one `FileDescriptorProto` in a pool.

use buffa_descriptor::generated::descriptor::FileDescriptorProto;

use crate::enumeration::EnumDescriptor;
use crate::message::MessageDescriptor;
use crate::pool::{DescriptorPool, FileIndex};

/// Handle to a single proto file inside a [`DescriptorPool`].
#[derive(Clone, Debug)]
pub struct FileDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) index: FileIndex,
}

impl FileDescriptor {
    /// File name as it appears in the descriptor (e.g.
    /// `acme/api/v1/user.proto`).
    #[must_use]
    pub fn name(&self) -> &str {
        let entry = &self.pool.inner.files[self.index as usize];
        entry.proto.name.as_deref().unwrap_or("")
    }

    /// Proto package declared by the file (`acme.api.v1` for the example
    /// above), or the empty string if the file declares none.
    #[must_use]
    pub fn package(&self) -> &str {
        let entry = &self.pool.inner.files[self.index as usize];
        entry.proto.package.as_deref().unwrap_or("")
    }

    /// Proto syntax (`proto2`, `proto3`, or `editions`).
    ///
    /// Defaults to `"proto2"` when unset, matching the protoc convention.
    #[must_use]
    pub fn syntax(&self) -> &str {
        let entry = &self.pool.inner.files[self.index as usize];
        entry.proto.syntax.as_deref().unwrap_or("proto2")
    }

    /// Top-level messages declared in this file.
    pub fn messages(&self) -> impl ExactSizeIterator<Item = MessageDescriptor> + '_ {
        let pool = self.pool.clone();
        let entry = &self.pool.inner.files[self.index as usize];
        entry
            .messages
            .clone()
            .into_iter()
            .map(move |idx| MessageDescriptor {
                pool: pool.clone(),
                index: idx,
            })
    }

    /// Top-level enums declared in this file.
    pub fn enums(&self) -> impl ExactSizeIterator<Item = EnumDescriptor> + '_ {
        let pool = self.pool.clone();
        let entry = &self.pool.inner.files[self.index as usize];
        entry
            .enums
            .clone()
            .into_iter()
            .map(move |idx| EnumDescriptor {
                pool: pool.clone(),
                index: idx,
            })
    }

    /// Owning [`DescriptorPool`].
    #[must_use]
    pub fn parent_pool(&self) -> DescriptorPool {
        self.pool.clone()
    }

    /// Raw [`FileDescriptorProto`] for advanced use.
    #[must_use]
    pub fn descriptor_proto(&self) -> &FileDescriptorProto {
        &self.pool.inner.files[self.index as usize].proto
    }
}

impl PartialEq for FileDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner) && self.index == other.index
    }
}

impl Eq for FileDescriptor {}
