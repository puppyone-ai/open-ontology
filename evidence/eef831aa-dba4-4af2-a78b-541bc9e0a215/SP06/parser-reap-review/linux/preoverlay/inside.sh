#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 focused smoke of sealed c27d6349.
# Runs the existing parser_worker tests only. The review overlay is absent.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export TMPDIR=/cargo-home/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
mkdir -p "$TMPDIR" /cargo-home/bin
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/smoke.exit; exit 2 ;;
esac

echo "=== source pins ==="
sha256sum \
  crates/oo-gateway/src/parser_worker.rs \
  crates/oo-gateway/src/bin/oo-sql-parser-worker.rs \
  crates/oo-gateway/tests/parser_worker.rs \
  contracts/gateway/parser-worker-v1.md \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  > /evidence/archive-sha256.txt
if ! cmp -s crates/oo-gateway/src/parser_worker.rs /blobs/parser_worker.rs \
  || ! cmp -s crates/oo-gateway/src/bin/oo-sql-parser-worker.rs /blobs/oo-sql-parser-worker.rs \
  || ! cmp -s crates/oo-gateway/tests/parser_worker.rs /blobs/parser_worker_tests.rs \
  || ! cmp -s contracts/gateway/parser-worker-v1.md /blobs/parser-worker-v1.md \
  || ! cmp -s crates/oo-gateway/src/query_executor.rs /blobs/query_executor.rs \
  || ! cmp -s crates/oo-gateway/src/lib.rs /blobs/lib.rs \
  || ! cmp -s crates/oo-gateway/Cargo.toml /blobs/oo-gateway-Cargo.toml \
  || ! cmp -s Cargo.lock /blobs/Cargo.lock
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f crates/oo-gateway/tests/parser_worker_a215_review.rs ]; then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
cp /evidence/archive-sha256.txt /evidence/lock-before.sha256

echo "=== rust tools ==="
rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
printf '%s\n' $? > /evidence/sha256-tool.exit
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
printf '%s\n' $? > /evidence/select-debs-tool.exit
if [ "$(cat /evidence/sha256-tool.exit)" != 0 ] || [ "$(cat /evidence/select-debs-tool.exit)" != 0 ]; then
  exit 1
fi
export PATH="/cargo-home/bin:${PATH}"

echo "=== libclang ==="
if [ ! -s /evidence/libclang-dir.txt ] || [ ! -d /cargo-home/a215-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  printf '%s\n' "$extract_exit" > /evidence/libclang.exit
  echo "PHASE libclang_exit=$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  echo "libclang_root=reused" > /evidence/libclang-extract.log
  printf '%s\n' 0 > /evidence/libclang.exit
fi
libdir=$(cat /evidence/libclang-dir.txt)
multiarch=/cargo-home/a215-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a215-clang-root/usr/lib/llvm-19/lib/clang/19/include
real=$(find /cargo-home/a215-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a215-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a215-clang-root/usr/bin:${PATH}"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/libclang-path.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$real" ]; then
  echo "libclang headers or libclang-19.so.19 missing" >&2
  printf '%s\n' 1 > /evidence/libclang-load.exit
  exit 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/libclang-ldd.txt; then
  printf '%s\n' 1 > /evidence/libclang-load.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/libclang-load.exit
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  exit "$fetch_exit"
fi

echo "=== cargo test parser_worker ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test parser_worker -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

before=$(sha256sum Cargo.lock | awk '{print $1}')
printf '%s\n' "$before" > /evidence/lock-after.sha256
want=$(sha256sum /blobs/Cargo.lock | awk '{print $1}')
if [ "$before" != "$want" ]; then
  echo "lock changed before=$want after=$before" > /evidence/lock-hash.txt
  printf '%s\n' 1 > /evidence/lock-hash.exit
  exit 1
fi
echo "unchanged $before" > /evidence/lock-hash.txt
printf '%s\n' 0 > /evidence/lock-hash.exit
if grep -nE 'unsafe_code|unsafe ' \
  crates/oo-gateway/src/parser_worker.rs \
  crates/oo-gateway/src/bin/oo-sql-parser-worker.rs \
  crates/oo-gateway/tests/parser_worker.rs > /evidence/unsafe-lines.txt
then
  echo "worker sources contain an unsafe token" >&2
  printf '%s\n' 2 > /evidence/unsafe.exit
  exit 2
fi
printf '%s\n' "no unsafe token" > /evidence/unsafe-lines.txt
printf '%s\n' 0 > /evidence/unsafe.exit
printf '%s\n' 0 > /evidence/smoke.exit
echo "preoverlay_smoke=ok"
