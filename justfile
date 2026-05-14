alias i := install

default:
    just --list

build:
    cargo build

fmt:
    cargo fmt

lint:
    cargo clippy

install:
    cargo install --path .

test:
    cargo test

check: fmt lint test

