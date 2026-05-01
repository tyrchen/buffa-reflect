//! `trybuild`-driven UI tests for the four documented diagnostic shapes.
//!
//! These tests are inherently brittle to compiler-error-message wording.
//! Run with `TRYBUILD=overwrite cargo test -p buffa-reflect-derive ui`
//! after upgrading toolchains.

#[test]
fn ui_should_report_recognizable_diagnostics() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/missing_binding.rs");
    t.compile_fail("tests/ui/both_bindings.rs");
    t.compile_fail("tests/ui/missing_message_name.rs");
    t.compile_fail("tests/ui/bad_attribute_shape.rs");
}
