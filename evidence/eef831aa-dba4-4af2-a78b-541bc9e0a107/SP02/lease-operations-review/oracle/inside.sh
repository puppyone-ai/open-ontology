#!/bin/bash
# Build and run the independent oracle inside the Rust 1.94 image.
# Offline first. A network build is recorded only if offline fails.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
mkdir -p /tmp/oracle/src /out
{
  echo "=== identity ==="
  rustc -Vv
  echo "rustc_exit=$?"
  cargo -V
  echo "cargo_v_exit=$?"
  uname -a
} > /out/rustc-Vv.txt
cp /oracle-src/Cargo.toml /oracle-src/Cargo.lock /tmp/oracle/
cp -a /oracle-src/src/. /tmp/oracle/src/
cd /tmp/oracle
echo "=== cargo build --offline ==="
set +e
cargo build --offline --manifest-path Cargo.toml > /out/cargo-offline.log 2>&1
off=$?
set -e
printf '%s\n' "$off" > /out/cargo-offline.exit
if [ "$off" -ne 0 ]; then
  echo "=== cargo build after offline failure ==="
  set +e
  cargo build --manifest-path Cargo.toml > /out/cargo-network.log 2>&1
  net=$?
  set -e
  printf '%s\n' "$net" > /out/cargo-network.exit
  if [ "$net" -ne 0 ]; then
    exit "$net"
  fi
fi
sha256sum /target/debug/lease_oracle > /out/binary.sha256
exec /target/debug/lease_oracle full
