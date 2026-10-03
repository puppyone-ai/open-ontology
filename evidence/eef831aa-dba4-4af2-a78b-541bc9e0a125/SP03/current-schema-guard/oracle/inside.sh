#!/bin/bash
# Public-API oracle against a copy of the working tree. The copy is not the frozen archive.
set -eu
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export RUSTUP_TOOLCHAIN=1.94.0
rm -rf /tmp/src
mkdir -p /target/phases /tmp/src
cp /src/Cargo.toml /src/Cargo.lock /src/rust-toolchain.toml /tmp/src/
mkdir -p /tmp/src/.cargo
cp /src/.cargo/config.toml /tmp/src/.cargo/config.toml
cp -a /src/crates /tmp/src/crates
cp /oracle/a125_oracle.rs /tmp/src/crates/oo-connectors/tests/a125_oracle.rs
test -f /tmp/src/crates/oo-connectors/src/snapshot.rs
cd /tmp/src
echo "=== identity ==="
uname -m
rustc -Vv
cargo -V
echo "tree=working-copy"
echo "=== cargo test --locked --offline -p oo-connectors --test a125_oracle ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a125_oracle -- --test-threads=1 --nocapture
ec=$?
set -e
echo "oracle_exit=$ec"
printf '%s\n' "$ec" > /target/phases/oracle.exit
exit "$ec"
