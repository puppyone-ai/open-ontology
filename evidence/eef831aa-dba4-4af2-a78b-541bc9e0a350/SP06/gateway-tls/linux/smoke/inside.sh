#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke. The archive is the fixed tree.
# http_gateway.rs is already in that tree. This script does not edit it.
# Shape follows a326 linux/smoke-2/inside.sh. Delta: the gateway source must
# be present, cargo check is --locked and online into the private cargo volume,
# and openssl presence is recorded. No lock rewrite.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export MAKEFLAGS=-j1
export CMAKE_BUILD_PARALLEL_LEVEL=1
mkdir -p /target/tmp /evidence /cargo-home/bin
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
  command -v openssl || true
  openssl version || true
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

if [ ! -f crates/oo-gateway/src/http_gateway.rs ]; then
  echo "archive is missing the gateway" >&2
  printf '%s\n' 2 > /evidence/gateway-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/gateway-present.exit
sha256sum crates/oo-gateway/src/http_gateway.rs crates/oo-gateway/src/bin/oo-gateway.rs > /evidence/gateway-sha256.txt
if [ -f /evidence/fixed-http-sha256.txt ]; then
  want=$(awk 'NR==1 {print $1}' /evidence/fixed-http-sha256.txt)
  got=$(awk 'NR==1 {print $1}' /evidence/gateway-sha256.txt)
  if [ "$want" != "$got" ]; then
    echo "http_gateway.rs does not match the fixed blob" >&2
    printf '%s\n' 2 > /evidence/fixed-blob.exit
    exit 2
  fi
fi
printf '%s\n' 0 > /evidence/fixed-blob.exit
echo "PHASE fixed_blob=0"

echo "=== rustc helper tools ==="
set +e
rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256 > /evidence/sha256-tool.stdout 2> /evidence/sha256-tool.stderr
sha_tool=$?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs > /evidence/select-debs-tool.stdout 2> /evidence/select-debs-tool.stderr
sel_tool=$?
set -e
printf '%s\n' "$sha_tool" > /evidence/sha256-tool.exit
printf '%s\n' "$sel_tool" > /evidence/select-debs-tool.exit
echo "PHASE sha256_tool_exit=$sha_tool"
echo "PHASE select_debs_tool_exit=$sel_tool"
if [ "$sha_tool" -ne 0 ] || [ "$sel_tool" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/arch-check.exit
  printf '%s\n' not_run > /evidence/gateway-check.exit
  exit 1
fi

echo "=== extract libclang ==="
set +e
bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
extract_exit=$?
set -e
printf '%s\n' "$extract_exit" > /evidence/libclang.exit
echo "PHASE libclang_exit=$extract_exit"
if [ "$extract_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/arch-check.exit
  printf '%s\n' not_run > /evidence/gateway-check.exit
  exit "$extract_exit"
fi

libdir=$(cat /evidence/libclang-dir.txt)
multiarch=/cargo-home/a326-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a326-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
devso=$(find /cargo-home/a326-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a326-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a326-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  echo "llvm_inc=$llvm_inc"
  echo "gcc_inc=$gcc_inc"
  ls -l "$libdir"/libclang.so* || true
  if [ -f "$llvm_inc/stdbool.h" ]; then
    echo "stdbool.h=present"
  else
    echo "stdbool.h=absent"
  fi
} > /evidence/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "llvm stdbool.h is absent" >&2
  printf '%s\n' 1 > /evidence/libclang-includes.exit
  printf '%s\n' not_run > /evidence/arch-check.exit
  printf '%s\n' not_run > /evidence/gateway-check.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/libclang-includes.exit

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

sha256sum /src/Cargo.lock > /evidence/lock-sha256.txt

if [ "$arch_exit" -ne 0 ]; then
  exit "$arch_exit"
fi
if [ "$gw_exit" -ne 0 ]; then
  exit "$gw_exit"
fi
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
echo "PHASE inside_ok"
exit 0
