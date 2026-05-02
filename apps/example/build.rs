//! Build script: compile the example .proto and decorate every generated
//! message with reflection metadata.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .files(&["proto/acme/api/v1/library.proto"])
        .includes(&["proto/"])
        .generate_view_reflection(true)
        .compile()?;
    Ok(())
}
