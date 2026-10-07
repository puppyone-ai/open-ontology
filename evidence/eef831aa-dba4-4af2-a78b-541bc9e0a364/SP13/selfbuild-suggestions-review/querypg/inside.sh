#!/bin/bash
# Build query-pg from the fixed-base archive. Locked. No lock rewrite.
# Libclang discovery matches the reviewed a326 extractor, unchanged.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export MAKEFLAGS=-j1
export CMAKE_BUILD_PARALLEL_LEVEL=1
mkdir -p /target/tmp /cargo-home/bin /out
export TMPDIR=/target/tmp
cd /src
if [ -f rust-toolchain.toml ]; then
  echo "FAILED toolchain file is present"
  exit 1
fi
rustc -vV > /out/rustc.txt 2> /out/rustc.stderr
printf '%s\n' "$?" > /out/rustc.exit
if ! grep -q '1.94.0' /out/rustc.txt; then
  echo "FAILED rustc"
  exit 1
fi
rustc --edition 2021 -O -o /cargo-home/bin/sha256 /tools/sha256.rs > /out/sha256-tool.txt 2> /out/sha256-tool.stderr
printf '%s\n' "$?" > /out/sha256-tool.exit
rustc --edition 2021 -O -o /cargo-home/bin/select-debs /tools/select-debs.rs > /out/select-debs-tool.txt 2> /out/select-debs-tool.stderr
printf '%s\n' "$?" > /out/select-debs-tool.exit
if [ "$(cat /out/sha256-tool.exit)" != "0" ] || [ "$(cat /out/select-debs-tool.exit)" != "0" ]; then
  echo "FAILED helper tools"
  exit 1
fi
bash /extract-libclang.sh > /out/libclang-extract.txt 2> /out/libclang-extract.stderr
printf '%s\n' "$?" > /out/libclang.exit
if [ "$(cat /out/libclang.exit)" != "0" ]; then
  echo "FAILED libclang=$(cat /out/libclang.exit)"
  exit 1
fi
libdir=$(cat /evidence/libclang-dir.txt 2>/dev/null || true)
if [ -z "$libdir" ]; then
  libdir=$(find /cargo-home/a326-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname)
fi
multiarch=/cargo-home/a326-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a326-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a326-clang-root/usr/bin:${PATH}"
printf 'LIBCLANG_PATH=%s\nllvm_inc=%s\n' "$LIBCLANG_PATH" "$llvm_inc" > /out/libclang.txt
cp /src/Cargo.lock /out/Cargo.lock.frozen
sha256sum /src/Cargo.lock > /out/lock-sha256.txt
sha256sum crates/oo-gateway/src/bin/query-pg.rs > /out/query-pg-sha256.txt
cargo fetch --locked --manifest-path /src/Cargo.toml > /out/fetch.txt 2> /out/fetch.stderr
printf '%s\n' "$?" > /out/fetch.exit
if [ "$(cat /out/fetch.exit)" != "0" ]; then
  echo "FAILED fetch=$(cat /out/fetch.exit)"
  exit 1
fi
cargo build --offline --locked --manifest-path /src/Cargo.toml -p oo-gateway --bin query-pg > /out/build.txt 2> /out/build.stderr
printf '%s\n' "$?" > /out/build.exit
if ! cmp -s /src/Cargo.lock /out/Cargo.lock.frozen; then
  echo "FAILED lock rewritten"
  exit 1
fi
if [ "$(cat /out/build.exit)" != "0" ] || [ ! -x /target/debug/query-pg ]; then
  echo "FAILED build=$(cat /out/build.exit)"
  exit 1
fi
sha256sum /target/debug/query-pg > /out/binary-sha256.txt
echo "DONE query-pg"
exit 0
