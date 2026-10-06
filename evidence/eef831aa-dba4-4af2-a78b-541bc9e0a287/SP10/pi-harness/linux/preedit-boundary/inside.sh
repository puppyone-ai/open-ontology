#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 MSRV smoke of the existing Pi RPC spike.
# The crate rust-version is 1.94.0. This runs the twelve boundary tests only.
# Immutable git archive. No source edit. No clippy. No PostgreSQL. No network.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
export CARGO_TERM_COLOR=never
cd /work
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -a
cat /etc/os-release
id
echo "=== toolchain ==="
rustup show > /proof/toolchain.txt
rustc -vV >> /proof/toolchain.txt
echo "=== package rust-version ==="
awk '/^rust-version/{print}' Cargo.toml
if [ ! -f tests/boundary.rs ]; then
  echo "boundary.rs missing from the fixed archive" >&2
  printf '%s\n' 2 > /proof/test.exit
  exit 2
fi
echo "=== boundary tests ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /proof/test.stdout 2> /proof/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /proof/test.exit
echo "PHASE test_exit=$test_exit"
exit "$test_exit"
