#!/bin/bash
# Focused proof on the fixed archive plus the independent review overlay.
# Secrets come from mounted files. This script does not print them.
set -u
echo "oo_task=sp06-query-executor-shutdown-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a191"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/proof/phases /evidence/live /cargo-home/bin
rustc -Vv | tee /evidence/linux/proof/rustc-version.txt
cargo -V | tee /evidence/linux/proof/cargo-version.txt
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
if [ ! -x /cargo-home/bin/leakscan ]; then
  rustc --edition 2021 /tools/leakscan.rs -o /cargo-home/bin/leakscan
fi
export PATH="/cargo-home/bin:${PATH}"
cd /src
if [ ! -f crates/oo-gateway/tests/query_executor_a191_review.rs ]; then
  echo "review overlay missing from the proof archive" >&2
  exit 2
fi
if [ -f crates/oo-gateway/src/parser_worker.rs ]; then
  echo "archive contains parser_worker.rs" >&2
  exit 2
fi
if grep -n 'unsafe' \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/tests/query_executor_a191_review.rs \
  > /evidence/linux/proof/unsafe-lines.txt; then
  echo "proof tree contains unsafe" >&2
  exit 2
fi
printf '%s\n' 0 > /evidence/linux/proof/phases/unsafe.exit
/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/tests/query_executor_pg_live.rs \
  crates/oo-gateway/tests/query_executor_a191_review.rs \
  contracts/gateway/query-executor-v1.md \
  crates/oo-gateway/src/lib.rs \
  > /evidence/linux/proof/owned-sha256.txt
if [ "$?" -ne 0 ]; then
  echo "sha256 file oracle failed" >&2
  exit 1
fi

record() {
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
z3=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu/libz3.so.4
if [ -n "$so" ] && [ -e "$z3" ]; then
  printf '%s\n' reused > /evidence/linux/proof/libclang-reused.txt
  record libclang 0
else
  rm -rf /cargo-home/clang-root
  set +e
  bash /extract-libclang.sh > /evidence/linux/proof/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
  so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
fi
if [ -z "$so" ]; then
  echo "libclang.so missing" >&2
  exit 2
fi
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/proof/libclang-path.txt

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  > /evidence/linux/proof/lib-test.log 2>&1
test_exit=$?
set -e
record lib-test "$test_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  printf '%s\n' present > /evidence/linux/proof/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo clippy review overlay ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_a191_review -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

echo "=== live review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_a191_review -- --nocapture \
  > /evidence/linux/proof/live.log 2>&1
live_exit=$?
set -e
printf '%s\n' "$live_exit" > /evidence/linux/proof/phases/live.exit
echo "phase_exit=$live_exit phase=live"

set +e
/cargo-home/bin/leakscan /evidence /secrets/admin.pass /secrets/login.pass \
  > /evidence/linux/proof/leak-scan.txt 2> /evidence/linux/proof/leak-scan.err
leak_exit=$?
set -e
printf '%s\n' "$leak_exit" > /evidence/linux/proof/phases/leakscan.exit
echo "phase_exit=$leak_exit phase=leakscan"
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi
if [ "$leak_exit" -ne 0 ]; then
  exit "$leak_exit"
fi
echo "proof_phases_ok"
