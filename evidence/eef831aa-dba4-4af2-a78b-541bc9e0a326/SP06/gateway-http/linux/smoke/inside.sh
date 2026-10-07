#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of frozen e8d297f.
# The archive is mounted read-only at /src. No product overlay. No lock rewrite.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export MAKEFLAGS=-j1
export CMAKE_BUILD_PARALLEL_LEVEL=1
mkdir -p /target/tmp /evidence
export TMPDIR=/target/tmp
cd /src

{
  echo "=== identity ==="
  rustc -vV
  echo "rustc_vv_exit=$?"
  cargo -V
  echo "cargo_v_exit=$?"
  rustup show
  uname -a
  cat /etc/os-release
  id
} > /evidence/identity.txt 2>&1

rustc -vV > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
rustup show > /evidence/rustup-show.txt

if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 4 > /evidence/rustc.exit
  printf '%s\n' not_run > /evidence/arch-check.exit
  printf '%s\n' not_run > /evidence/gateway-check.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/rustc.exit
echo "PHASE rustc_exit=0"

if [ -f crates/oo-gateway/src/http_gateway.rs ] \
  || [ -f crates/oo-gateway/src/http_error.rs ] \
  || [ -f crates/oo-gateway/build.rs ]
then
  echo "product overlay present in pre-edit archive" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
echo "PHASE overlay_absent=0"

sha256sum \
  Cargo.toml \
  Cargo.lock \
  rust-toolchain.toml \
  crates/oo-gateway/Cargo.toml \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/bin/oo-gateway.rs \
  > /evidence/source-sha256.txt

echo "=== cargo check oo-arch-check ==="
set +e
cargo check --manifest-path /src/Cargo.toml --locked -p oo-arch-check \
  > /evidence/arch-check.stdout 2> /evidence/arch-check.stderr
arch_exit=$?
set -e
printf '%s\n' "$arch_exit" > /evidence/arch-check.exit
echo "PHASE arch_check_exit=$arch_exit"

echo "=== cargo check oo-gateway ==="
set +e
cargo check --manifest-path /src/Cargo.toml --locked -p oo-gateway \
  > /evidence/gateway-check.stdout 2> /evidence/gateway-check.stderr
gw_exit=$?
set -e
printf '%s\n' "$gw_exit" > /evidence/gateway-check.exit
echo "PHASE gateway_check_exit=$gw_exit"

if [ "$arch_exit" -ne 0 ]; then
  exit "$arch_exit"
fi
if [ "$gw_exit" -ne 0 ]; then
  exit "$gw_exit"
fi
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
echo "PHASE inside_ok"
exit 0
