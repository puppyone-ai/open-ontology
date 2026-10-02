#!/bin/bash
# Linux arm64 Rust 1.94 minimal native compile and parse smoke.
# Runs inside the pinned builder. Not an admission suite.
set -euo pipefail

echo "oo_owner=a065"
echo "oo_scope=sp01-p5-parser-smoke"
echo "oo_toolchain_expected=1.94.0"
rustc -Vv
uname -a
cat /etc/os-release
got=$(rustc --version)
echo "rustc_version=$got"
echo "$got" | grep -q 'rustc 1.94.0 '

if command -v protoc >/dev/null 2>&1; then
  echo "protoc_present=yes" >&2
  echo "refusing protobuf regeneration during smoke" >&2
  exit 3
fi
echo "protoc_present=no"

if ! command -v clang >/dev/null 2>&1 || ! ldconfig -p 2>/dev/null | grep -q 'libclang'; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-install-recommends clang libclang-dev llvm-dev pkg-config ca-certificates
fi
clang --version
pkg-config --modversion libclang || true
echo "libclang_path=${LIBCLANG_PATH:-unset}"

cd /src
if [ ! -f Cargo.lock ]; then
  cargo generate-lockfile
fi
cargo fetch --locked
cargo run --locked --offline --bin oo-p05-pg17-parser-smoke
echo "smoke_phase_exit=0"
