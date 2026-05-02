//! Build script: compile the `grpc.reflection.v1` proto into Rust types via
//! `tonic-build`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_client(false)
        .build_server(true)
        .compile_protos(
            &[
                "proto/grpc/reflection/v1/reflection.proto",
                "proto/grpc/reflection/v1alpha/reflection.proto",
            ],
            &["proto/"],
        )?;
    Ok(())
}
