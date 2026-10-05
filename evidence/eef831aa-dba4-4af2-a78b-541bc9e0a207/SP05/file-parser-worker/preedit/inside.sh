#!/bin/bash
# Focused pre-edit smoke of the approved JSONL and delimited readers.
# No PostgreSQL. No package edits. Private cargo home and target dir.
set -eu
echo "inside_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
rustc --version
cargo --version
echo "RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN-}"
cargo fetch --locked
cargo test --locked --offline -p oo-connectors --test jsonl_reader --test delimited -- --test-threads=8
echo "inside_ok=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
