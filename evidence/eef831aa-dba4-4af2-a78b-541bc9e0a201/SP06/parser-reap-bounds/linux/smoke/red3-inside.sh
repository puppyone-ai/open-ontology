#!/bin/bash
# Bounded std probe, then the unfixed a195 red hang. Attempt 2 is left intact.
# A probe failure does not skip the red harness.
set -u
echo "oo_task=sp06-parser-reap-bounds-red3"
echo "phase=red-attempt-3"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export PATH="/cargo-home/bin:${PATH}"
mkdir -p /evidence/linux/smoke/red-attempt-3
cd /src

libdir=/cargo-home/a201-clang-root/usr/lib/llvm-19/lib
multiarch=/cargo-home/a201-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a201-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a201-clang-root/usr/bin:${PATH}"

echo "=== std api probe 3 ==="
set +e
rustc --edition 2024 -D warnings /tools/std-api-probe.rs -o /cargo-home/bin/std-api-probe \
  > /evidence/linux/smoke/red-attempt-3/std-api-probe-compile.log 2>&1
compile_exit=$?
printf '%s\n' "$compile_exit" > /evidence/linux/smoke/red-attempt-3/std-api-compile.exit
echo "phase_exit=$compile_exit phase=std-api-compile-3"
if [ "$compile_exit" -eq 0 ]; then
  if command -v timeout >/dev/null 2>&1; then
    timeout --signal=TERM 25 /cargo-home/bin/std-api-probe \
      > /evidence/linux/smoke/red-attempt-3/std-api-probe.txt 2>&1
    probe_exit=$?
  else
    /cargo-home/bin/std-api-probe > /evidence/linux/smoke/red-attempt-3/std-api-probe.txt 2>&1
    probe_exit=$?
  fi
else
  probe_exit=1
  echo "std-api-probe did not compile" > /evidence/linux/smoke/red-attempt-3/std-api-probe.txt
fi
printf '%s\n' "$probe_exit" > /evidence/linux/smoke/red-attempt-3/std-api.exit
echo "phase_exit=$probe_exit phase=std-api-3"

echo "=== red grandchild stdout attempt 3 ==="
cp /evidence/linux/smoke/red/parser_worker_a201_red.rs crates/oo-gateway/tests/parser_worker_a201_red.rs
export A201_ORACLE=/evidence/linux/smoke/red-attempt-3/oracle.txt
: > "$A201_ORACLE"
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test parser_worker_a201_red -- --test-threads=1 --nocapture \
  > /evidence/linux/smoke/red-attempt-3/test.log 2>&1
red_exit=$?
printf '%s\n' "$red_exit" > /evidence/linux/smoke/red-attempt-3/red.exit
echo "phase_exit=$red_exit phase=red-3"
exit "$red_exit"
