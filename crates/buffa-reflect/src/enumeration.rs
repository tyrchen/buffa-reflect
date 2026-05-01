//! [`EnumDescriptor`] and [`EnumValueDescriptor`].

use buffa_descriptor::generated::descriptor::EnumDescriptorProto;

use crate::{
    file::FileDescriptor,
    message::MessageDescriptor,
    pool::{DescriptorPool, EnumIndex},
};

/// Handle to one enum declaration in a [`DescriptorPool`].
#[derive(Clone, Debug)]
pub struct EnumDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) index: EnumIndex,
}

impl EnumDescriptor {
    fn entry(&self) -> &crate::pool::EnumEntry {
        &self.pool.inner.enums[self.index as usize]
    }

    /// Fully-qualified name (`<package>.<...>.<EnumName>`, no leading dot).
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.entry().full_name
    }

    /// Leaf name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.entry().name
    }

    /// Owning file.
    #[must_use]
    pub fn parent_file(&self) -> FileDescriptor {
        FileDescriptor {
            pool: self.pool.clone(),
            index: self.entry().file,
        }
    }

    /// Containing message, when this enum is nested.
    #[must_use]
    pub fn parent_message(&self) -> Option<MessageDescriptor> {
        self.entry().parent.map(|idx| MessageDescriptor {
            pool: self.pool.clone(),
            index: idx,
        })
    }

    /// Iterate over the declared variants.
    pub fn values(&self) -> impl ExactSizeIterator<Item = EnumValueDescriptor> + '_ {
        let pool = self.pool.clone();
        let owner = self.index;
        let len = self.entry().values.len() as u32;
        (0..len).map(move |vi| EnumValueDescriptor {
            pool: pool.clone(),
            owner,
            index: vi,
        })
    }

    /// Lookup a variant by its proto name.
    #[must_use]
    pub fn get_value_by_name(&self, name: &str) -> Option<EnumValueDescriptor> {
        let entry = self.entry();
        entry.by_name.get(name).map(|vi| EnumValueDescriptor {
            pool: self.pool.clone(),
            owner: self.index,
            index: *vi,
        })
    }

    /// Lookup a variant by its number.
    #[must_use]
    pub fn get_value_by_number(&self, number: i32) -> Option<EnumValueDescriptor> {
        let entry = self.entry();
        entry.by_number.get(&number).map(|vi| EnumValueDescriptor {
            pool: self.pool.clone(),
            owner: self.index,
            index: *vi,
        })
    }

    /// Raw [`EnumDescriptorProto`] for advanced use.
    #[must_use]
    pub fn descriptor_proto(&self) -> &EnumDescriptorProto {
        let entry = self.entry();
        let file = &self.pool.inner.files[entry.file as usize];
        crate::pool_build::resolve_enum_proto(&file.proto, &entry.proto_path)
    }
}

impl PartialEq for EnumDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner) && self.index == other.index
    }
}

impl Eq for EnumDescriptor {}

/// Handle to one variant of an [`EnumDescriptor`].
#[derive(Clone, Debug)]
pub struct EnumValueDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) owner: EnumIndex,
    pub(crate) index: u32,
}

impl EnumValueDescriptor {
    fn entry(&self) -> &crate::pool::EnumValueEntry {
        &self.pool.inner.enums[self.owner as usize].values[self.index as usize]
    }

    /// Variant name as written in the .proto.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.entry().name
    }

    /// `<enum.full_name>.<variant>`, no leading dot.
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.entry().full_name
    }

    /// Numeric value of the variant.
    #[must_use]
    pub fn number(&self) -> i32 {
        self.entry().number
    }

    /// Owning enum.
    #[must_use]
    pub fn parent_enum(&self) -> EnumDescriptor {
        EnumDescriptor {
            pool: self.pool.clone(),
            index: self.owner,
        }
    }
}

impl PartialEq for EnumValueDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner)
            && self.owner == other.owner
            && self.index == other.index
    }
}

impl Eq for EnumValueDescriptor {}
