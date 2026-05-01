//! Drive both reflection implementations off the same FDS and assert
//! semantic equivalence.

use buffa_reflect_equivalence::{collect_buffa, collect_prost, fixture_dir, run_protoc};

fn protoc_available() -> bool {
    std::process::Command::new("protoc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn buffa_reflect_matches_prost_reflect() {
    if !protoc_available() {
        eprintln!("protoc not on PATH; skipping");
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let fds_path = tmp.path().join("zoo.binpb");
    run_protoc(&fixture_dir(), &fds_path).expect("protoc");
    let fds_bytes = std::fs::read(&fds_path).expect("read fds");

    let prost_view = collect_prost(&fds_bytes).expect("prost-reflect collect");
    let buffa_view = collect_buffa(&fds_bytes).expect("buffa-reflect collect");

    let diffs = prost_view.diff(&buffa_view);
    if !diffs.is_empty() {
        panic!("found {} divergence(s):\n{}", diffs.len(), diffs.join("\n"));
    }
}
