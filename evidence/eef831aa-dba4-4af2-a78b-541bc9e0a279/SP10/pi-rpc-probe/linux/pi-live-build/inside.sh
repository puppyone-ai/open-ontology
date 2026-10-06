#!/bin/bash
# Compile the observation probe with the pinned Rust 1.94 toolchain. No cargo.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
rustc -vV > /evidence/rustc-version.txt
rustc --edition 2024 -D warnings /src/mock_openai.rs -o /out/mock_openai > /evidence/mock-rustc.stdout 2> /evidence/mock-rustc.stderr
echo 0 > /evidence/mock-rustc.exit
rustc --edition 2024 -D warnings /src/observe.rs -o /out/observe > /evidence/observe-rustc.stdout 2> /evidence/observe-rustc.stderr
echo 0 > /evidence/observe-rustc.exit
echo "build-ok"
