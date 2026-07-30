alias i := install

skills_dir := home_directory() / ".claude/skills"

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

# Symlink the skill so edits in this repo take effect immediately.
skill-link:
    mkdir -p "{{ skills_dir }}"
    rm -rf "{{ skills_dir }}/flowy"
    ln -s "{{ justfile_directory() }}/skills/flowy" "{{ skills_dir }}/flowy"

# Copy the skill, pinning it against further edits here.
skill-install:
    mkdir -p "{{ skills_dir }}"
    rm -rf "{{ skills_dir }}/flowy"
    cp -R "{{ justfile_directory() }}/skills/flowy" "{{ skills_dir }}/flowy"

test:
    cargo test

check: fmt lint test

