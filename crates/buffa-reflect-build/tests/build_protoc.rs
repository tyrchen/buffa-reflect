//! End-to-end builder test: drive `protoc` through the Builder, then
//! verify the artifacts buffa-reflect-build was supposed to write.
//!
//! Build-script integration tests are synchronous by nature; the workspace
//! lint banning sync `Command` / `std::fs` is suppressed here for the same
//! reason the crate root suppresses it.

#![allow(
    clippy::disallowed_types,
    clippy::disallowed_methods,
    reason = "build-script integration tests run synchronously."
)]

use std::{path::PathBuf, process::Command};

fn protoc_available() -> bool {
    Command::new("protoc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proto")
}

#[test]
fn test_should_compile_via_protoc_and_emit_descriptor_set() {
    if !protoc_available() {
        eprintln!("protoc not on PATH; skipping test");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let out_dir = tmp.path().to_path_buf();

    let proto = fixture_dir().join("acme/api/v1/user.proto");
    let include = fixture_dir();

    buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .files(&[&proto])
        .includes(&[&include])
        .out_dir(&out_dir)
        .compile()
        .expect("compile succeeds");

    // FDS artifact lands in out_dir.
    let fds_path = out_dir.join("file_descriptor_set.bin");
    assert!(fds_path.exists(), "FDS bytes were not written");

    // Locate the generated package file (acme.api.v1.rs or similar).
    let mut found_pkg_file = false;
    for entry in std::fs::read_dir(&out_dir).expect("readdir") {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| n.ends_with(".rs"))
        {
            let content = std::fs::read_to_string(&path).expect("read");
            // Match the *owned* User struct, not UserView<'a>.
            if content.contains("pub struct User {") {
                found_pkg_file = true;
                assert!(
                    content.contains("::buffa_reflect::ReflectMessage"),
                    "generated file should derive ReflectMessage:\n{content}"
                );
                assert!(
                    content.contains(r#"message_name = "acme.api.v1.User""#),
                    "generated file should contain User's FQN attribute:\n{content}"
                );
                assert!(
                    content.contains(r#"message_name = "acme.api.v1.User.Profile""#),
                    "generated file should contain Profile's FQN attribute:\n{content}"
                );
                assert!(
                    content.contains(
                        r#"file_descriptor_set_bytes = "crate::FILE_DESCRIPTOR_SET_BYTES""#
                    ),
                    "generated file should contain bytes binding:\n{content}"
                );
            }
        }
    }
    assert!(
        found_pkg_file,
        "no generated .rs file contained `pub struct User`"
    );
}

#[test]
fn test_should_compile_via_precompiled_descriptor_set() {
    if !protoc_available() {
        eprintln!("protoc not on PATH; skipping test");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let out_dir = tmp.path().to_path_buf();

    let proto = fixture_dir().join("acme/api/v1/user.proto");
    let include = fixture_dir();

    // Pre-build a descriptor set ourselves with raw protoc.
    let pre_fds = tmp.path().join("pre.binpb");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg("--include_source_info")
        .arg(format!("--descriptor_set_out={}", pre_fds.display()))
        .arg(format!("--proto_path={}", include.display()))
        .arg(&proto)
        .status()
        .expect("protoc spawn");
    assert!(status.success(), "raw protoc failed");

    buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .descriptor_set(&pre_fds)
        .files(&["acme/api/v1/user.proto"])
        .out_dir(&out_dir)
        .compile()
        .expect("compile succeeds");

    let fds_path = out_dir.join("file_descriptor_set.bin");
    assert!(fds_path.exists(), "FDS bytes were not written");
}

#[test]
fn test_descriptor_set_round_trips_through_buffa_reflect() {
    if !protoc_available() {
        eprintln!("protoc not on PATH; skipping test");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let out_dir = tmp.path().to_path_buf();

    buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .files(&[fixture_dir().join("acme/api/v1/user.proto")])
        .includes(&[fixture_dir()])
        .out_dir(&out_dir)
        .compile()
        .expect("compile");

    let fds_bytes = std::fs::read(out_dir.join("file_descriptor_set.bin")).unwrap();
    let pool = buffa_reflect::DescriptorPool::decode(&fds_bytes).expect("pool decodes");

    let user = pool
        .get_message_by_name("acme.api.v1.User")
        .expect("User present");
    assert_eq!(user.full_name(), "acme.api.v1.User");
    assert!(user.fields().count() >= 7);

    let profile = pool
        .get_message_by_name("acme.api.v1.User.Profile")
        .expect("Profile present");
    assert_eq!(profile.name(), "Profile");

    let role = pool
        .get_enum_by_name("acme.api.v1.Role")
        .expect("Role present");
    assert_eq!(role.values().count(), 3);
}
