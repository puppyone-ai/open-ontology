#!/bin/bash
# Independent overlay on the sealed c27d6349 archive.
# Runs only parser_worker_a215_review, then clippy on that test.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export TMPDIR=/cargo-home/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export OO_A215_ORACLE_DIR=/evidence/oracle
mkdir -p "$TMPDIR" "$OO_A215_ORACLE_DIR" /cargo-home/bin
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac

echo "=== pins ==="
sha256sum \
  crates/oo-gateway/src/parser_worker.rs \
  crates/oo-gateway/src/bin/oo-sql-parser-worker.rs \
  crates/oo-gateway/tests/parser_worker.rs \
  crates/oo-gateway/tests/parser_worker_a215_review.rs \
  contracts/gateway/parser-worker-v1.md \
  crates/oo-gateway/src/query_executor.rs \
  Cargo.lock \
  > /evidence/source-sha256.txt
if ! cmp -s crates/oo-gateway/src/parser_worker.rs /blobs/parser_worker.rs \
  || ! cmp -s crates/oo-gateway/src/bin/oo-sql-parser-worker.rs /blobs/oo-sql-parser-worker.rs \
  || ! cmp -s crates/oo-gateway/tests/parser_worker.rs /blobs/parser_worker_tests.rs \
  || ! cmp -s contracts/gateway/parser-worker-v1.md /blobs/parser-worker-v1.md \
  || ! cmp -s crates/oo-gateway/src/query_executor.rs /blobs/query_executor.rs \
  || ! cmp -s crates/oo-gateway/tests/parser_worker_a215_review.rs /evidence/expect-review.rs \
  || ! cmp -s Cargo.lock /blobs/Cargo.lock
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  printf '%s\n' not_run > /evidence/fetch.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
echo "PHASE blob_cmp_exit=0"

libdir=$(find /cargo-home/a215-clang-root/usr/lib -name 'libclang.so' -print -quit)
real=$(find /cargo-home/a215-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
llvm_inc=/cargo-home/a215-clang-root/usr/lib/llvm-19/lib/clang/19/include
multiarch=/cargo-home/a215-clang-root/usr/lib/aarch64-linux-gnu
if [ -z "$libdir" ] || [ -z "$real" ] || [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "libclang from the preoverlay volume is missing" >&2
  printf '%s\n' 1 > /evidence/libclang.exit
  exit 1
fi
export LIBCLANG_PATH="$(dirname "$libdir")"
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a215-clang-root/usr/bin:${PATH}"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/libclang-path.txt
printf '%s\n' 0 > /evidence/libclang.exit
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo test independent overlay ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test parser_worker_a215_review -- --test-threads=1 --nocapture \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$test_exit"
fi

echo "=== cargo clippy overlay test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --test parser_worker_a215_review -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

before=$(sha256sum Cargo.lock | awk '{print $1}')
want=$(sha256sum /blobs/Cargo.lock | awk '{print $1}')
printf '%s\n' "$before" > /evidence/lock-after.sha256
if [ "$before" != "$want" ]; then
  echo "lock changed" > /evidence/lock-hash.txt
  printf '%s\n' 1 > /evidence/lock-hash.exit
  exit 1
fi
echo "unchanged $before" > /evidence/lock-hash.txt
printf '%s\n' 0 > /evidence/lock-hash.exit
printf '%s\n' 0 > /evidence/overlay.exit
echo "overlay=ok"
