.PHONY: build release test check fmt lint install

build:
	cargo build --workspace

release:
	cargo build --release -p things

test:
	cargo test --workspace

check:
	cargo check --workspace

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

install:
	cargo install --path crates/things-cli
