#!/bin/bash
# Independent process proof. The tree at /src is a private copy of c22cf08
# plus the review overlay. The immutable archive is not mounted.
set -u
echo "oo_task=sp06-parser-worker-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a195"
echo "oo_fixed=c22cf08aa5973ae2e2d95e0d2b72f98487f8555a"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export A195_ORACLE=/evidence/linux/proof/oracle.txt
mkdir -p /evidence/linux/proof/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/proof/phases
  printf '%s\n' "$2" > "/evidence/linux/proof/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/proof/proof.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/tests/parser_worker_a195_review.rs ]; then
  echo "proof tree is missing the review overlay" >&2
  record overlay 2
fi
record overlay 0

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/parser_worker.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/bin/oo-sql-parser-worker.rs \
  crates/oo-gateway/tests/parser_worker.rs \
  crates/oo-gateway/src/query_executor.rs \
  contracts/gateway/parser-worker-v1.md \
  Cargo.lock \
  crates/oo-gateway/Cargo.toml \
  > /evidence/linux/proof/owned-sha256.txt
record owned-hash $?
if ! cmp -s /evidence/linux/smoke/owned-sha256.txt /evidence/linux/proof/owned-sha256.txt; then
  echo "proof tree product hashes differ from the smoked archive" >&2
  record product-hash 2
fi
record product-hash 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a195-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a195-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
real=$(find /cargo-home/a195-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a195-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a195-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a195-clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/proof/libclang-path.txt
if [ -z "$real" ] || [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "private libclang root is incomplete" >&2
  record libclang 1
fi
record libclang 0

: > /evidence/linux/proof/oracle.txt
echo "=== cargo test --test parser_worker_a195_review ==="
set +e
timeout -k 20 600 cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test parser_worker_a195_review -- --test-threads=1 --nocapture \
  > /evidence/linux/proof/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/proof/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo clippy -p oo-gateway --all-targets ==="
set +e
timeout -k 20 600 cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --all-targets -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/proof/lock-after.sha256
before=$(cat /evidence/linux/smoke/lock-after.sha256)
after=$(cat /evidence/linux/proof/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/proof/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/proof/lock-hash.txt
record lock-hash 0
printf '%s\n' "no" > /evidence/linux/proof/cargo-update-invoked.txt
printf '%s\n' 0 > /evidence/linux/proof/proof.exit
echo "proof=ok"
exit 0
