default:
    just --list

build:
    cargo build

fmt:
    cargo fmt

lint:
    cargo clippy

smoke:
    cargo build
    bash tests/smoke.sh

