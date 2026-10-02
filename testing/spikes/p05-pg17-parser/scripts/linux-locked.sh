#!/bin/bash
# Linux arm64 Rust 1.94 locked test and clippy for the P5 parser spike.
# Mount only this directory at /src. Do not run if protoc is installed.
set -euo pipefail

echo "oo_owner=a065"
echo "oo_scope=sp01-p5-parser-locked"
echo "oo_toolchain_expected=1.94.0"
rustc -Vv
uname -a
cat /etc/os-release
got=$(rustc --version)
echo "rustc_version=$got"
echo "$got" | grep -q 'rustc 1.94.0 '

if command -v protoc >/dev/null 2>&1; then
  echo "protoc_present=yes" >&2
  echo "refusing protobuf regeneration" >&2
  exit 3
fi
echo "protoc_present=no"

if ! command -v clang >/dev/null 2>&1 || ! ldconfig -p 2>/dev/null | grep -q 'libclang'; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-install-recommends clang libclang-dev llvm-dev pkg-config ca-certificates
fi
clang --version
echo "memory_max=$(cat /sys/fs/cgroup/memory.max 2>/dev/null || true)"
echo "cpu_max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || true)"
echo "pids_max=$(cat /sys/fs/cgroup/pids.max 2>/dev/null || true)"

if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy
fi

cd /src
cargo update -p cc --precise 1.4.7
cargo update -p find-msvc-tools --precise 0.1.13
cargo update -p lazy_static --precise 1.5.0
cargo fetch --locked

cargo test --locked --offline
cargo test --locked --offline --features test-hooks
cargo clippy --all-targets --locked --offline -- -D warnings
cargo clippy --all-targets --locked --offline --features test-hooks -- -D warnings
echo "locked_phase_exit=0"
