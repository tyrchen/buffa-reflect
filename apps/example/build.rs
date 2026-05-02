//! Build script: compile the example .proto and decorate every generated
//! message with reflection metadata.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // View reflection is auto-enabled because file_descriptor_set_bytes
    // is set; pass .generate_view_reflection(false) to opt out.
    buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .files(&["proto/acme/api/v1/library.proto"])
        .includes(&["proto/"])
        .compile()?;
    Ok(())
}
