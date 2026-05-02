//! `ServerReflection` request handlers.

use std::pin::Pin;

use buffa::Message as _;
use buffa_reflect::DescriptorPool;
use tokio_stream::{Stream, StreamExt as _};
use tonic::{Request, Response, Status, Streaming};

use crate::proto::v1::{
    ErrorResponse, ExtensionNumberResponse, FileDescriptorResponse, ListServiceResponse,
    ServerReflectionRequest, ServerReflectionResponse, ServiceResponse,
    server_reflection_request::MessageRequest, server_reflection_response::MessageResponse,
    server_reflection_server::ServerReflection,
};

/// `ServerReflection` impl backed by a [`DescriptorPool`].
#[derive(Debug)]
pub struct ReflectionService {
    pool: DescriptorPool,
    advertised: Option<Vec<String>>,
}

impl ReflectionService {
    /// Wrap a pool. `advertised` overrides the auto-discovered service
    /// list when set.
    #[must_use]
    pub fn new(pool: DescriptorPool, advertised: Option<Vec<String>>) -> Self {
        Self { pool, advertised }
    }

    /// Test helper: synchronously dispatch one request and return the
    /// matching response without going through the streaming transport
    /// machinery. Suitable for unit tests.
    #[must_use]
    pub fn handle_one(&self, request: ServerReflectionRequest) -> ServerReflectionResponse {
        handle(&self.pool, self.advertised.as_deref(), request)
    }
}

#[tonic::async_trait]
impl ServerReflection for ReflectionService {
    type ServerReflectionInfoStream =
        Pin<Box<dyn Stream<Item = Result<ServerReflectionResponse, Status>> + Send + 'static>>;

    async fn server_reflection_info(
        &self,
        request: Request<Streaming<ServerReflectionRequest>>,
    ) -> Result<Response<Self::ServerReflectionInfoStream>, Status> {
        let mut inbound = request.into_inner();
        let pool = self.pool.clone();
        let advertised = self.advertised.clone();
        let stream = async_stream::try_stream! {
            while let Some(req) = inbound.next().await {
                let req = req?;
                let resp = handle(&pool, advertised.as_deref(), req);
                yield resp;
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }
}

fn handle(
    pool: &DescriptorPool,
    advertised: Option<&[String]>,
    req: ServerReflectionRequest,
) -> ServerReflectionResponse {
    let host = req.host.clone();
    let original = Some(req.clone());
    let body = match req.message_request {
        Some(MessageRequest::FileByFilename(name)) => file_by_name(pool, &name),
        Some(MessageRequest::FileContainingSymbol(sym)) => file_containing_symbol(pool, &sym),
        Some(MessageRequest::FileContainingExtension(_)) => {
            // Phase 1 doesn't ship extensions; surface a clean error.
            error(2, "extensions are not supported by buffa-reflect")
        }
        Some(MessageRequest::AllExtensionNumbersOfType(t)) => MessageResponse::AllExtensionNumbersResponse(
            ExtensionNumberResponse {
                base_type_name: t,
                extension_number: Vec::new(),
            },
        ),
        Some(MessageRequest::ListServices(_)) => list_services(pool, advertised),
        None => error(3, "missing message_request"),
    };
    ServerReflectionResponse {
        valid_host: host,
        original_request: original,
        message_response: Some(body),
    }
}

fn list_services(pool: &DescriptorPool, advertised: Option<&[String]>) -> MessageResponse {
    let mut names: Vec<String> = match advertised {
        Some(list) => list.to_vec(),
        None => pool.services().map(|s| s.full_name().to_string()).collect(),
    };
    names.sort();
    MessageResponse::ListServicesResponse(ListServiceResponse {
        service: names
            .into_iter()
            .map(|name| ServiceResponse { name })
            .collect(),
    })
}

fn file_by_name(pool: &DescriptorPool, name: &str) -> MessageResponse {
    match pool.get_file_by_name(name) {
        Some(f) => MessageResponse::FileDescriptorResponse(FileDescriptorResponse {
            file_descriptor_proto: vec![encode_file(&f)],
        }),
        None => error(5, &format!("file `{name}` not found")),
    }
}

fn file_containing_symbol(pool: &DescriptorPool, sym: &str) -> MessageResponse {
    let key = sym.strip_prefix('.').unwrap_or(sym);
    if let Some(svc) = pool.get_service_by_name(key) {
        let f = svc.parent_file();
        return MessageResponse::FileDescriptorResponse(FileDescriptorResponse {
            file_descriptor_proto: vec![encode_file(&f)],
        });
    }
    if let Some(m) = pool.get_message_by_name(key) {
        let f = m.parent_file();
        return MessageResponse::FileDescriptorResponse(FileDescriptorResponse {
            file_descriptor_proto: vec![encode_file(&f)],
        });
    }
    if let Some(e) = pool.get_enum_by_name(key) {
        let f = e.parent_file();
        return MessageResponse::FileDescriptorResponse(FileDescriptorResponse {
            file_descriptor_proto: vec![encode_file(&f)],
        });
    }
    error(5, &format!("symbol `{sym}` not found"))
}

fn encode_file(file: &buffa_reflect::FileDescriptor) -> Vec<u8> {
    // buffa's FileDescriptorProto is wire-compatible with the tonic
    // (prost) FileDescriptorProto, but it lives in a different Rust
    // type. Round-trip the bytes through the buffa type's encode and
    // hand the bytes verbatim — clients use whatever proto library
    // they choose to decode them.
    file.descriptor_proto().encode_to_vec()
}

fn error(code: i32, msg: &str) -> MessageResponse {
    MessageResponse::ErrorResponse(ErrorResponse {
        error_code: code,
        error_message: msg.to_string(),
    })
}
