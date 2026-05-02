.PHONY: build test lint verify example publish-dry publish release update-submodule

# Build everything in the workspace.
build:
	@cargo build --workspace --all-targets

# Run all unit + integration tests via nextest.
test:
	@cargo build --workspace
	@cargo nextest run --workspace --all-features

# fmt + clippy across the workspace.
lint:
	@cargo +nightly fmt --all -- --check
	@cargo clippy --workspace --all-targets -- -D warnings

# Lint, build docs, and run tests.  Use this before opening a PR.
verify: lint test
	@cargo doc --workspace --no-deps

# Run the end-to-end demo binary.
example:
	@cargo run -p buffa-reflect-example

# Dry-run packaging for the two crates whose deps live entirely on crates.io.
# `buffa-reflect` itself can only be dry-run after `buffa-reflect-derive` is
# uploaded, so it is excluded here.
publish-dry:
	@cargo publish -p buffa-reflect-derive --dry-run --allow-dirty
	@cargo publish -p buffa-reflect-build --dry-run --allow-dirty

# Publish all three crates to crates.io in dependency order. The runtime crate
# pulls the freshly published `buffa-reflect-derive` from the index; cargo
# waits for it to become available between steps.
publish: verify
	@cargo publish -p buffa-reflect-derive
	@cargo publish -p buffa-reflect
	@cargo publish -p buffa-reflect-build

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote
