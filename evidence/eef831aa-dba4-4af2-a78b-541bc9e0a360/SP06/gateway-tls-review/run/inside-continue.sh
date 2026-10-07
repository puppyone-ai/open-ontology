#!/bin/bash
# Continuation only. The archive smoke, author Clippy, loopback, TLS default,
# fixture, bundle, and a350 live test already passed and are not run again.
# Clippy is installed only when this container's toolchain does not have it.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export MAKEFLAGS=-j1
export CMAKE_BUILD_PARALLEL_LEVEL=1
export PATH="/usr/local/cargo/bin:/cargo-home/bin:${PATH}"
mkdir -p /target/tmp /evidence /cargo-home/bin
export TMPDIR=/target/tmp

phase() {
  printf '%s\n' "$*"
  printf '%s\n' "$*" >> /evidence/phases.txt
}

{
  echo "=== identity ==="
  rustc -vV
  cargo -V
  rustup show
  uname -a
  id
  openssl version
} > /evidence/identity.txt 2>&1
rustc -vV > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 4 > /evidence/rustc.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/rustc.exit
phase "PHASE rustc_exit=0"

if [ ! -d /target/work/crates/oo-gateway ]; then
  echo "reused work tree is absent" >&2
  printf '%s\n' 2 > /evidence/work-tree.exit
  exit 2
fi
cd /target/work
sha256sum \
  crates/oo-gateway/src/http_gateway.rs \
  crates/oo-gateway/src/bin/oo-gateway.rs \
  crates/oo-gateway/tests/http_gateway_tls.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/work-sha256.txt
if ! cmp -s /prior/work-sha256.txt /evidence/work-sha256.txt; then
  echo "reused work tree does not match the smoke archive hashes" >&2
  printf '%s\n' 2 > /evidence/work-tree.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/work-tree.exit
phase "PHASE work_tree_reused"

if [ ! -x /target/debug/role-pack-bundle ] || [ ! -x /target/tmp/gateway-pack-fixture ]; then
  echo "reused pack binaries are absent" >&2
  printf '%s\n' 2 > /evidence/bins.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/bins.exit
phase "PHASE bins_reused"

if ! find /cargo-home/a326-clang-root/usr/lib -name 'libclang.so' -print -quit | grep -q .; then
  echo "reused libclang is absent" >&2
  printf '%s\n' 2 > /evidence/libclang.exit
  exit 2
fi
printf '%s\n' reused > /evidence/libclang.exit
libdir=$(find /cargo-home/a326-clang-root/usr/lib -name 'libclang.so' -print -quit | xargs dirname)
multiarch=/cargo-home/a326-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a326-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a326-clang-root/usr/share/pkgconfig"
printf 'LIBCLANG_PATH=%s\nllvm_inc=%s\n' "$LIBCLANG_PATH" "$llvm_inc" > /evidence/libclang.txt
phase "PHASE libclang_reused"

cp /target/work/Cargo.lock /evidence/Cargo.lock.frozen
sha256sum /target/work/Cargo.lock > /evidence/lock-sha256.txt
lock_unchanged() {
  if ! cmp -s /target/work/Cargo.lock /evidence/Cargo.lock.frozen; then
    echo "cargo rewrote Cargo.lock" >&2
    printf '%s\n' 6 > /evidence/lock-unchanged.exit
    exit 6
  fi
}

rustup component list --installed > /evidence/clippy-component.list
if grep -q '^clippy' /evidence/clippy-component.list; then
  printf '%s\n' present > /evidence/clippy-component.exit
  phase "PHASE clippy_component_present"
else
  phase "PHASE clippy_component_absent_install"
  rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  printf '%s\n' $? > /evidence/clippy-component.exit
  if [ "$(cat /evidence/clippy-component.exit)" != "0" ]; then
    exit 1
  fi
fi

phase "PHASE review_overlay"
cp /review/review_tls.rs /target/work/crates/oo-gateway/tests/http_gateway_tls_review.rs
sha256sum /target/work/crates/oo-gateway/tests/http_gateway_tls_review.rs > /evidence/review-test-sha256.txt
if ! cmp -s /review/review_tls.rs /target/work/crates/oo-gateway/tests/http_gateway_tls_review.rs; then
  echo "review overlay mismatch" >&2
  exit 2
fi
if cmp -s /prior/work-sha256.txt /evidence/work-sha256.txt; then
  phase "PHASE product_hashes_unchanged_before_overlay"
fi

phase "PHASE review_clippy_begin"
set +e
cargo clippy --manifest-path /target/work/Cargo.toml --locked --keep-going -p oo-gateway \
  --test http_gateway_tls_review -- -D warnings \
  > /evidence/review-clippy.stdout 2> /evidence/review-clippy.stderr
review_clippy_exit=$?
set -e
printf '%s\n' "$review_clippy_exit" > /evidence/review-clippy.exit
phase "PHASE review_clippy_exit=$review_clippy_exit"
lock_unchanged
if [ "$review_clippy_exit" -ne 0 ]; then
  exit "$review_clippy_exit"
fi

phase "PHASE review_local_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked -p oo-gateway --test http_gateway_tls_review -- --test-threads=1 \
  > /evidence/review-local.stdout 2> /evidence/review-local.stderr
review_local_exit=$?
set -e
printf '%s\n' "$review_local_exit" > /evidence/review-local.exit
phase "PHASE review_local_exit=$review_local_exit"
lock_unchanged
if [ "$review_local_exit" -ne 0 ]; then
  exit "$review_local_exit"
fi

export OO_A360_FIXTURE_BIN=/target/tmp/gateway-pack-fixture
export OO_A360_ROLE_PACK_BIN=/target/debug/role-pack-bundle

phase "PHASE review_live_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked -p oo-gateway --test http_gateway_tls_review -- --ignored --test-threads=1 \
  verified_leaf_does_not_select_the_token_tuple \
  > /evidence/review-live.stdout 2> /evidence/review-live.stderr
review_live_exit=$?
set -e
printf '%s\n' "$review_live_exit" > /evidence/review-live.exit
phase "PHASE review_live_exit=$review_live_exit"
lock_unchanged
if [ "$review_live_exit" -ne 0 ]; then
  exit "$review_live_exit"
fi

sha256sum \
  crates/oo-gateway/src/http_gateway.rs \
  crates/oo-gateway/src/bin/oo-gateway.rs \
  crates/oo-gateway/tests/http_gateway_tls.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/work-sha256-after.txt
if ! cmp -s /prior/work-sha256.txt /evidence/work-sha256-after.txt; then
  echo "product files changed during the review tests" >&2
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/product-unchanged.exit
grep -q '1.94.0' /evidence/rustc-version.txt
grep -E 'axum = \{ version = "=0.8.9"' Cargo.toml > /evidence/pins.txt
grep -E 'rustls = \{ version = "=0.23.45"' crates/oo-gateway/Cargo.toml >> /evidence/pins.txt
grep -E 'tokio-rustls = \{ version = "=0.26.5"' crates/oo-gateway/Cargo.toml >> /evidence/pins.txt
printf '%s\n' 0 > /evidence/lock-unchanged.exit
printf '%s\n' 0 > /evidence/inside.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
phase "PHASE inside_ok"
