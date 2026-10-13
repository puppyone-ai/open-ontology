#!/bin/bash
# Runs inside the Linux arm64 Rust 1.94 builder. Not a host compile.
set -euo pipefail
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
rustc -vV > /evidence/rustc-vv.txt
cd /src
cargo build --jobs 1 > /evidence/cargo-build.stdout 2> /evidence/cargo-build.stderr
cargo build --locked --jobs 1 > /evidence/cargo-locked.stdout 2> /evidence/cargo-locked.stderr
sha256sum /target/debug/oracle-driver-probe /target/debug/oracle-arrow-decode > /evidence/binaries.sha256
