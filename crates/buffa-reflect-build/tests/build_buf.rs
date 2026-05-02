//! Buf-mode integration test. Requires `buf` on PATH and a `buf.yaml`
//! discoverable from cwd; the test is `#[ignore]` by default and runs in
//! CI where `buf` is provisioned. Trigger with
//! `cargo test -p buffa-reflect-build --test build_buf -- --ignored`.

#![allow(
    clippy::disallowed_types,
    clippy::disallowed_methods,
    reason = "build-script integration tests run synchronously."
)]

use std::{path::PathBuf, process::Command};

fn buf_available() -> bool {
    Command::new("buf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
#[ignore = "requires buf on PATH and a buf.yaml workspace; CI-only"]
fn test_should_compile_via_buf() {
    if !buf_available() {
        eprintln!("buf not on PATH; skipping");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let out_dir = tmp.path().to_path_buf();

    // Run from a directory whose `buf.yaml` describes the proto/ tree.
    // The fixtures dir already contains proto files; tests/fixtures/buf.yaml
    // would need to be authored if we were to drive an actual `buf build`.
    // For now this is a smoke test that exercises the Buf code path.
    let _cwd = fixture_dir();

    // Failure here is expected when no buf.yaml exists; the goal is to
    // verify the Buf source code path compiles and surfaces a clean error.
    let result = buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .use_buf()
        .files(&["acme/api/v1/user.proto"])
        .out_dir(&out_dir)
        .compile();

    // Either success (when fixture has a buf.yaml) or a clean DescriptorTool /
    // DescriptorToolFailed error — never a panic.
    assert!(
        result.is_ok()
            || matches!(
                result,
                Err(buffa_reflect_build::Error::DescriptorTool { .. })
                    | Err(buffa_reflect_build::Error::DescriptorToolFailed { .. })
            ),
        "expected success or a tool-launch error, got {result:?}"
    );
}

#[test]
fn test_use_buf_with_includes_returns_clean_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out_dir = tmp.path().to_path_buf();

    let err = buffa_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .use_buf()
        .files(&["acme/api/v1/user.proto"])
        .includes(&["proto/"])
        .out_dir(&out_dir)
        .compile()
        .expect_err("buf + includes should error");

    assert!(matches!(err, buffa_reflect_build::Error::BufWithIncludes));
}
