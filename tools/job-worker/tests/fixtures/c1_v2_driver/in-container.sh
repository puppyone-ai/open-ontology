#!/bin/bash
# Usage (inside container): CMD env var holds the shell command to run in /work.
set -uo pipefail
export RUSTUP_TOOLCHAIN=1.94.0 CARGO_TARGET_DIR=/target CARGO_TERM_COLOR=never
ln -sfn /cargo-vol/registry /usr/local/cargo/registry
rm -rf /work && mkdir -p /work
cp -a /source/Cargo.toml /source/Cargo.lock /source/rust-toolchain.toml /work/ 2>/dev/null
cp -a /source/crates /source/tools /work/
cd /work
bash -c "$CMD"
echo "EXIT_CODE=$?"
