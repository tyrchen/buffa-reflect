//! Cross-implementation equivalence check.
//!
//! Runs `protoc` over the bundled `proto/` tree to produce a
//! `FileDescriptorSet`, then loads the same bytes through both
//! `prost_reflect::DescriptorPool` and `buffa_reflect::DescriptorPool`.
//! Walks every file / message / enum / field / oneof / enum-value and
//! asserts structural equivalence (same FQNs, field numbers, json names,
//! cardinality, presence semantics, packed flag, kind shape, …).
//!
//! Run from the workspace root:
//!
//! ```sh
//! cd examples/equivalence && cargo run --release
//! ```
//!
//! The same comparisons live under `tests/equivalence.rs` so `cargo test`
//! can drive them in CI.

use std::path::PathBuf;

use buffa_reflect_equivalence::{collect_buffa, collect_prost, fixture_dir, run_protoc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::tempdir()?;
    let fds_path: PathBuf = tmp.path().join("zoo.binpb");
    run_protoc(&fixture_dir(), &fds_path)?;
    let fds_bytes = std::fs::read(&fds_path)?;

    let prost_view = collect_prost(&fds_bytes)?;
    let buffa_view = collect_buffa(&fds_bytes)?;

    println!(
        "prost-reflect: {} files / {} messages / {} enums",
        prost_view.files.len(),
        prost_view.messages.len(),
        prost_view.enums.len(),
    );
    println!(
        "buffa-reflect: {} files / {} messages / {} enums",
        buffa_view.files.len(),
        buffa_view.messages.len(),
        buffa_view.enums.len(),
    );

    let diffs = prost_view.diff(&buffa_view);
    if diffs.is_empty() {
        println!("\n✅ semantically identical");
        Ok(())
    } else {
        println!("\n❌ {} divergence(s):", diffs.len());
        for d in &diffs {
            println!("  - {d}");
        }
        Err(format!("{} divergence(s) detected", diffs.len()).into())
    }
}
