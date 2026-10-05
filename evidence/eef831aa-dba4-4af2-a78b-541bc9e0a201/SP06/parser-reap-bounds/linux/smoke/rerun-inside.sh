#!/bin/bash
# Corrected std probe and the a195 red hang. Smoke already passed.
# Does not overwrite the first probe or the first red compile log.
set -u
echo "oo_task=sp06-parser-reap-bounds-red2"
echo "phase=red-attempt-2"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export PATH="/cargo-home/bin:${PATH}"
mkdir -p /evidence/linux/smoke/red-attempt-2
cd /src

libdir=/cargo-home/a201-clang-root/usr/lib/llvm-19/lib
multiarch=/cargo-home/a201-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a201-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a201-clang-root/usr/bin:${PATH}"

echo "=== std api probe 2 ==="
set +e
rustc --edition 2024 -D warnings /tools/std-api-probe.rs -o /cargo-home/bin/std-api-probe \
  > /evidence/linux/smoke/red-attempt-2/std-api-probe-compile.log 2>&1
compile_exit=$?
printf '%s\n' "$compile_exit" > /evidence/linux/smoke/red-attempt-2/std-api-compile.exit
echo "phase_exit=$compile_exit phase=std-api-compile-2"
if [ "$compile_exit" -eq 0 ]; then
  /cargo-home/bin/std-api-probe > /evidence/linux/smoke/red-attempt-2/std-api-probe.txt 2>&1
  probe_exit=$?
else
  probe_exit=1
  echo "std-api-probe did not compile" > /evidence/linux/smoke/red-attempt-2/std-api-probe.txt
fi
printf '%s\n' "$probe_exit" > /evidence/linux/smoke/red-attempt-2/std-api.exit
echo "phase_exit=$probe_exit phase=std-api-2"
set -e

echo "=== red grandchild stdout attempt 2 ==="
cp /evidence/linux/smoke/red/parser_worker_a201_red.rs crates/oo-gateway/tests/parser_worker_a201_red.rs
export A201_ORACLE=/evidence/linux/smoke/red-attempt-2/oracle.txt
: > "$A201_ORACLE"
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test parser_worker_a201_red -- --test-threads=1 --nocapture \
  > /evidence/linux/smoke/red-attempt-2/test.log 2>&1
red_exit=$?
set -e
printf '%s\n' "$red_exit" > /evidence/linux/smoke/red-attempt-2/red.exit
echo "phase_exit=$red_exit phase=red-2"
if [ "$probe_exit" -ne 0 ]; then
  exit "$probe_exit"
fi
exit "$red_exit"
