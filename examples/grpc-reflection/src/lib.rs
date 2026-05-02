//! `grpc.reflection.v1.ServerReflection` service backed by a
//! [`buffa_reflect::DescriptorPool`].
//!
//! Drop-in shape with `tonic-reflection` so consumers migrating from
//! `prost`/`tonic-reflection` find the same affordances:
//!
//! ```ignore
//! const FDS_BYTES: &[u8] = &[];
//! let pool = buffa_reflect::DescriptorPool::decode(FDS_BYTES).unwrap();
//! let refl = buffa_grpc_reflection::Builder::from_pool(pool).build_v1();
//! tonic::transport::Server::builder()
//!     .add_service(refl)
//!     .add_service(my_service)
//!     .serve(addr).await.unwrap();
//! ```

#![warn(missing_docs)]

pub mod proto {
    //! Generated tonic types for `grpc.reflection.v1`.
    #[allow(
        clippy::all,
        clippy::pedantic,
        missing_docs,
        unreachable_pub,
        non_snake_case
    )]
    pub mod v1 {
        tonic::include_proto!("grpc.reflection.v1");
    }
}

mod service;

pub use crate::service::ReflectionService;

use buffa_reflect::DescriptorPool;

/// Builder for the gRPC reflection service.
#[derive(Debug)]
pub struct Builder {
    pool: DescriptorPool,
    advertised_services: Option<Vec<String>>,
}

impl Builder {
    /// Construct from an existing pool.
    #[must_use]
    pub fn from_pool(pool: DescriptorPool) -> Self {
        Self {
            pool,
            advertised_services: None,
        }
    }

    /// Construct by decoding `FileDescriptorSet` bytes.
    ///
    /// # Errors
    ///
    /// See [`buffa_reflect::DescriptorError`].
    pub fn from_file_descriptor_set_bytes(
        bytes: &[u8],
    ) -> Result<Self, buffa_reflect::DescriptorError> {
        Ok(Self::from_pool(DescriptorPool::decode(bytes)?))
    }

    /// Limit advertised services. Default: every service in the pool.
    #[must_use]
    pub fn advertise_services(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.advertised_services = Some(names.into_iter().collect());
        self
    }

    /// Build the v1 reflection server, ready to register with
    /// `tonic::transport::Server::builder().add_service(...)`.
    #[must_use]
    pub fn build_v1(
        self,
    ) -> proto::v1::server_reflection_server::ServerReflectionServer<ReflectionService> {
        let svc = ReflectionService::new(self.pool, self.advertised_services);
        proto::v1::server_reflection_server::ServerReflectionServer::new(svc)
    }
}
