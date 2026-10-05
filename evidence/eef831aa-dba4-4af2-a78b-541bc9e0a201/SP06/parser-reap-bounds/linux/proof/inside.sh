#!/bin/bash
# Focused proof of the bounded parser teardown. Archive already has the edited sources.
set -u
echo "oo_task=sp06-parser-reap-bounds-proof"
echo "phase=proof"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/proof
cd /src

libdir=/cargo-home/a201-clang-root/usr/lib/llvm-19/lib
multiarch=/cargo-home/a201-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a201-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a201-clang-root/usr/bin:${PATH}"

rustc --version > /evidence/linux/proof/rustc-version.txt
if [ -e crates/oo-gateway/tests/parser_worker_a201_red.rs ]; then
  echo "red harness is still on the proof tree" >&2
  exit 2
fi
grep -n "GROUP_KILL_BIN" crates/oo-gateway/src/parser_worker.rs > /evidence/linux/proof/overlay-marker.txt

echo "=== parser_worker tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test parser_worker -- --test-threads=4 \
  > /evidence/linux/proof/test.log 2>&1
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/linux/proof/test.exit
echo "phase_exit=$test_exit phase=test"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== clippy ==="
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/linux/proof/clippy.exit
echo "phase_exit=$clippy_exit phase=clippy"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

grep -n "unsafe" crates/oo-gateway/src/parser_worker.rs crates/oo-gateway/src/bin/oo-sql-parser-worker.rs crates/oo-gateway/tests/parser_worker.rs \
  > /evidence/linux/proof/unsafe-lines.txt || true
echo "phase_exit=0 phase=proof"
exit 0
