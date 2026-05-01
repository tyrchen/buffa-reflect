.PHONY: build test lint verify example release update-submodule

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

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote
