#!/bin/bash
# Focused oo-gateway tests and Clippy against the copied consumer lock.
# The lock is already the five-triple consumer lock. Cargo runs --locked --offline
# and must not rewrite it. Libclang discovery matches linux/smoke-2/inside.sh.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export MAKEFLAGS=-j1
export CMAKE_BUILD_PARALLEL_LEVEL=1
mkdir -p /target/tmp /evidence /cargo-home/bin
export TMPDIR=/target/tmp
cd /src

{
  echo "=== identity ==="
  rustc -vV
  cargo -V
  rustup show
  uname -a
  id
} > /evidence/identity.txt 2>&1
rustc -vV > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 4 > /evidence/rustc.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/rustc.exit
echo "PHASE rustc_exit=0"

if [ ! -f crates/oo-gateway/src/http_gateway.rs ] || [ ! -f crates/oo-gateway/build.rs ]; then
  echo "product overlay is missing" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
printf '%s\n' present > /evidence/overlay.txt
sha256sum crates/oo-gateway/src/http_gateway.rs crates/oo-gateway/src/pack_catalog.rs \
  crates/oo-gateway/src/bin/oo-gateway.rs crates/oo-gateway/build.rs crates/oo-gateway/Cargo.toml \
  crates/oo-gateway/tests/http_gateway_loopback.rs crates/oo-gateway/tests/http_gateway_live.rs \
  Cargo.lock \
  > /evidence/overlay-sha256.txt

if [ ! -x /cargo-home/bin/sha256 ] || [ ! -x /cargo-home/bin/select-debs ]; then
  rustc --edition 2021 -O -o /cargo-home/bin/sha256 /tools/sha256.rs
  rustc --edition 2021 -O -o /cargo-home/bin/select-debs /tools/select-debs.rs
fi
if ! find /cargo-home/a326-clang-root/usr/lib -name 'libclang.so' -print -quit | grep -q .; then
  bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
  printf '%s\n' $? > /evidence/libclang.exit
else
  printf '%s\n' reused > /evidence/libclang.exit
  find /cargo-home/a326-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/libclang-dir.txt
fi
libdir=$(cat /evidence/libclang-dir.txt)
multiarch=/cargo-home/a326-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a326-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
devso=$(find /cargo-home/a326-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a326-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a326-clang-root/usr/share/pkgconfig"
printf 'LIBCLANG_PATH=%s\nllvm_inc=%s\n' "$LIBCLANG_PATH" "$llvm_inc" > /evidence/libclang.txt
test -f "$llvm_inc/stdbool.h"
echo "PHASE libclang_ready"

rustc --edition 2021 -O -o /target/tmp/lock-consumer /tools/lock-consumer.rs
echo "PHASE lock_consumer_built"
set +e
/target/tmp/lock-consumer /evidence/Cargo.lock.pre-consumer /src/Cargo.lock \
  > /evidence/lock-consumer.txt 2> /evidence/lock-consumer.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-consumer.exit
echo "PHASE lock_consumer_exit=$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  exit "$lock_exit"
fi
if grep -E '^edge .+ -' /evidence/lock-consumer.txt >/dev/null; then
  echo "consumer lock removed an existing dependency edge" >&2
  printf '%s\n' 5 > /evidence/edge-removal.exit
  exit 5
fi
missing=0
grep -q '^edge hyper 1.11.1 +httpdate$' /evidence/lock-consumer.txt || missing=1
grep -q '^edge tower 0.5.3 +tokio-util$' /evidence/lock-consumer.txt || missing=1
grep -q '^edge tower 0.5.3 +tracing$' /evidence/lock-consumer.txt || missing=1
grep -q '^edge tower-http 0.6.11 +http-body-util$' /evidence/lock-consumer.txt || missing=1
grep -q '^edge tower-http 0.6.11 +tokio$' /evidence/lock-consumer.txt || missing=1
if [ "$missing" -ne 0 ]; then
  echo "consumer lock is missing a required feature edge" >&2
  printf '%s\n' 5 > /evidence/edge-removal.exit
  exit 5
fi
printf '%s\n' 0 > /evidence/edge-removal.exit
echo "PHASE edges_kept"
cp /src/Cargo.lock /evidence/Cargo.lock.frozen
sha256sum /src/Cargo.lock /evidence/Cargo.lock.pre-consumer > /evidence/lock-sha256.txt

lock_unchanged() {
  if ! cmp -s /src/Cargo.lock /evidence/Cargo.lock.frozen; then
    echo "cargo rewrote Cargo.lock" >&2
    printf '%s\n' 6 > /evidence/lock-unchanged.exit
    exit 6
  fi
}

echo "=== cargo test lib ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=1 \
  > /evidence/lib-test.stdout 2> /evidence/lib-test.stderr
lib_exit=$?
set -e
printf '%s\n' "$lib_exit" > /evidence/lib-test.exit
echo "PHASE lib_test_exit=$lib_exit"
lock_unchanged

echo "=== cargo test loopback ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_loopback -- --test-threads=1 \
  > /evidence/loopback-test.stdout 2> /evidence/loopback-test.stderr
loop_exit=$?
set -e
printf '%s\n' "$loop_exit" > /evidence/loopback-test.exit
echo "PHASE loopback_exit=$loop_exit"
lock_unchanged

echo "=== cargo test live compile ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_live -- --test-threads=1 \
  > /evidence/live-compile.stdout 2> /evidence/live-compile.stderr
live_exit=$?
set -e
printf '%s\n' "$live_exit" > /evidence/live-compile.exit
echo "PHASE live_compile_exit=$live_exit"
lock_unchanged

if [ "$lib_exit" -ne 0 ]; then
  exit "$lib_exit"
fi
if [ "$loop_exit" -ne 0 ]; then
  exit "$loop_exit"
fi
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi

if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy
fi
echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
lock_unchanged
printf '%s\n' 0 > /evidence/lock-unchanged.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
echo "PHASE inside_ok"
