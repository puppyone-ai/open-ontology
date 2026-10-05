#!/bin/bash
# Clippy on the same archive and review overlay. Does not rerun the live oracle.
set -u
echo "oo_task=sp06-cancel-transport-bounds-review-clippy"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export RUSTUP_HOME=/cargo-home/rustup
mkdir -p /evidence/linux/proof/phases "$RUSTUP_HOME"
cd /src
so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/clang-root/usr/share/pkgconfig"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  add_exit=$?
  printf '%s\n' "$add_exit" > /evidence/linux/proof/phases/clippy-component.exit
  if [ "$add_exit" -ne 0 ]; then
    exit "$add_exit"
  fi
else
  printf '%s\n' present > /evidence/linux/proof/clippy-component.log
  printf '%s\n' 0 > /evidence/linux/proof/phases/clippy-component.exit
fi
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test query_executor_pg_live -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/linux/proof/phases/clippy.exit
echo "clippy_exit=$clippy_exit"
exit "$clippy_exit"
