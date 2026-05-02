//! [`ServiceDescriptor`] and [`MethodDescriptor`] — handles to gRPC
//! services and their methods.
//!
//! Services + methods are walked at pool-build time and cached on the
//! pool. Cross-file `input_type` / `output_type` references resolve
//! through the same name table the field resolver uses.

use buffa_descriptor::generated::descriptor::{MethodDescriptorProto, ServiceDescriptorProto};

use crate::{
    file::FileDescriptor,
    message::MessageDescriptor,
    pool::{DescriptorPool, FileIndex, MessageIndex},
};

/// Internal index into [`crate::pool::PoolInner::services`].
pub(crate) type ServiceIndex = u32;

/// Internal entry for a service in the pool.
#[derive(Clone)]
pub(crate) struct ServiceEntry {
    pub(crate) full_name: Box<str>,
    pub(crate) name: Box<str>,
    pub(crate) file: FileIndex,
    pub(crate) proto_index: u32,
    pub(crate) methods: Vec<MethodEntry>,
}

#[derive(Clone)]
pub(crate) struct MethodEntry {
    pub(crate) name: Box<str>,
    pub(crate) full_name: Box<str>,
    pub(crate) input: MessageIndex,
    pub(crate) output: MessageIndex,
    pub(crate) is_client_streaming: bool,
    pub(crate) is_server_streaming: bool,
    pub(crate) proto_index: u32,
}

/// Handle to one service in a [`DescriptorPool`].
#[derive(Clone, Debug)]
pub struct ServiceDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) index: ServiceIndex,
}

impl ServiceDescriptor {
    fn entry(&self) -> &ServiceEntry {
        &self.pool.inner.services[self.index as usize]
    }

    /// Leaf name (the part after the last `.`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.entry().name
    }

    /// Fully-qualified name (`<package>.<ServiceName>`).
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.entry().full_name
    }

    /// Owning file.
    #[must_use]
    pub fn parent_file(&self) -> FileDescriptor {
        FileDescriptor {
            pool: self.pool.clone(),
            index: self.entry().file,
        }
    }

    /// All declared methods, in proto declaration order.
    pub fn methods(&self) -> impl ExactSizeIterator<Item = MethodDescriptor> + '_ {
        let len = self.entry().methods.len() as u32;
        let pool = self.pool.clone();
        let svc = self.index;
        (0..len).map(move |idx| MethodDescriptor {
            pool: pool.clone(),
            service: svc,
            index: idx,
        })
    }

    /// Raw [`ServiceDescriptorProto`] for advanced use.
    #[must_use]
    pub fn descriptor_proto(&self) -> &ServiceDescriptorProto {
        let entry = self.entry();
        let file = &self.pool.inner.files[entry.file as usize];
        &file.proto.service[entry.proto_index as usize]
    }
}

impl PartialEq for ServiceDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner) && self.index == other.index
    }
}

impl Eq for ServiceDescriptor {}

/// Handle to one method on a [`ServiceDescriptor`].
#[derive(Clone, Debug)]
pub struct MethodDescriptor {
    pub(crate) pool: DescriptorPool,
    pub(crate) service: ServiceIndex,
    pub(crate) index: u32,
}

impl MethodDescriptor {
    fn entry(&self) -> &MethodEntry {
        &self.pool.inner.services[self.service as usize].methods[self.index as usize]
    }

    /// Leaf name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.entry().name
    }

    /// Fully-qualified name (`<service.full_name>.<MethodName>`).
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.entry().full_name
    }

    /// Resolved input message descriptor.
    #[must_use]
    pub fn input(&self) -> MessageDescriptor {
        MessageDescriptor {
            pool: self.pool.clone(),
            index: self.entry().input,
        }
    }

    /// Resolved output message descriptor.
    #[must_use]
    pub fn output(&self) -> MessageDescriptor {
        MessageDescriptor {
            pool: self.pool.clone(),
            index: self.entry().output,
        }
    }

    /// True iff the method's input is a stream.
    #[must_use]
    pub fn is_client_streaming(&self) -> bool {
        self.entry().is_client_streaming
    }

    /// True iff the method's output is a stream.
    #[must_use]
    pub fn is_server_streaming(&self) -> bool {
        self.entry().is_server_streaming
    }

    /// Owning service.
    #[must_use]
    pub fn parent_service(&self) -> ServiceDescriptor {
        ServiceDescriptor {
            pool: self.pool.clone(),
            index: self.service,
        }
    }

    /// Raw [`MethodDescriptorProto`] for advanced use.
    #[must_use]
    pub fn descriptor_proto(&self) -> &MethodDescriptorProto {
        let svc_entry = &self.pool.inner.services[self.service as usize];
        let file = &self.pool.inner.files[svc_entry.file as usize];
        &file.proto.service[svc_entry.proto_index as usize].method
            [self.entry().proto_index as usize]
    }
}

impl PartialEq for MethodDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.pool.inner, &other.pool.inner)
            && self.service == other.service
            && self.index == other.index
    }
}

impl Eq for MethodDescriptor {}

impl DescriptorPool {
    /// Iterate every service in the pool.
    pub fn services(&self) -> impl ExactSizeIterator<Item = ServiceDescriptor> + '_ {
        let pool = self.clone();
        (0..self.inner.services.len() as u32).map(move |idx| ServiceDescriptor {
            pool: pool.clone(),
            index: idx,
        })
    }

    /// Resolve a fully-qualified service name (with or without leading dot).
    #[must_use]
    pub fn get_service_by_name(&self, full_name: &str) -> Option<ServiceDescriptor> {
        let key = full_name.strip_prefix('.').unwrap_or(full_name);
        self.inner
            .service_names
            .get(key)
            .map(|idx| ServiceDescriptor {
                pool: self.clone(),
                index: *idx,
            })
    }
}

impl FileDescriptor {
    /// Services declared in this file.
    pub fn services(&self) -> impl ExactSizeIterator<Item = ServiceDescriptor> + '_ {
        let pool = self.pool.clone();
        let entry = &self.pool.inner.files[self.index as usize];
        entry
            .service_indices
            .clone()
            .into_iter()
            .map(move |idx| ServiceDescriptor {
                pool: pool.clone(),
                index: idx,
            })
    }
}
