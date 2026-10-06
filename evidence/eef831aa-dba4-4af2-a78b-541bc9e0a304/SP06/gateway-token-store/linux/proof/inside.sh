#!/bin/bash
# Overlay proof. The lock subset must pass before fetch. No generate-lockfile. No cargo update.
set -eu
echo "PHASE toolchain"
rustc --version
cargo --version
rustc --version | grep -q '1.94.0'
echo "PHASE tree"
test -f /src/Cargo.lock
test -f /src/tools/gateway-token-store/Cargo.toml
test -f /src/tools/gateway-token-store/Cargo.lock
test -f /src/tools/gateway-token-store/CONTRACT.md
test -f /src/rust-toolchain.toml
echo "PHASE subset"
rustc --edition 2024 -D warnings /evidence/tools/lock_subset.rs -o /target/lock_subset
set +e
/target/lock_subset /src/Cargo.lock /src/tools/gateway-token-store/Cargo.lock
subset_exit=$?
set -eu
if [ "$subset_exit" -ne 0 ]; then
  echo "PHASE subset_failed"
  exit 4
fi
echo "PHASE fetch"
cd /src/tools/gateway-token-store
cargo fetch --locked
echo "PHASE clippy_component"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0
fi
echo "PHASE test"
export CARGO_NET_OFFLINE=true
cargo test --locked --offline -- --test-threads=1
echo "PHASE clippy"
cargo clippy --locked --offline --all-targets -- -D warnings
echo "PHASE crate_manifest"
rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /target/sha256_manifest
/target/sha256_manifest /src/tools/gateway-token-store /evidence/linux/proof/crate-manifest.sha256
echo "PHASE proof_ok"
