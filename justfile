default:
    just --list

build:
    cargo build

fmt:
    cargo fmt

lint:
    cargo clippy

test:
    cargo test

check: fmt lint test

