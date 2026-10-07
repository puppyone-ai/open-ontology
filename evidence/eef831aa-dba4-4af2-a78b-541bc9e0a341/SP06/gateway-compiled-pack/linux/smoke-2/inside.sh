#!/bin/bash
# Immutable cd8f754 smoke, then the owned overlay. Cargo stays --locked.
# Libclang discovery matches the a326 extractor with a341 cargo paths.
# /tmp is noexec. Binaries and scratch live on the target volume.
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
phase() {
  printf '%s\n' "$1" >> /evidence/phase.txt
  echo "PHASE $1"
}

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
  phase "rustc_exit=4"
  exit 4
fi
printf '%s\n' 0 > /evidence/rustc.exit
phase "rustc_exit=0"

if [ ! -f /src/crates/oo-gateway/src/http_gateway.rs ] || [ ! -f /src/crates/oo-gateway/build.rs ]; then
  echo "archive is missing the gateway" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -f /src/crates/oo-gateway/tests/http_gateway_pg_live.rs ]; then
  echo "archive already contains the new live test" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
printf '%s\n' present > /evidence/archive.txt
cp /src/Cargo.lock /evidence/Cargo.lock.frozen
sha256sum /src/Cargo.lock /src/crates/oo-gateway/src/pack_catalog.rs /src/crates/oo-builder/src/canonical.rs \
  /src/crates/oo-builder/src/role_pack.rs > /evidence/archive-sha256.txt

if [ ! -x /cargo-home/bin/sha256 ] || [ ! -x /cargo-home/bin/select-debs ]; then
  rustc --edition 2021 -O -o /cargo-home/bin/sha256 /tools/sha256.rs
  rustc --edition 2021 -O -o /cargo-home/bin/select-debs /tools/select-debs.rs
fi
if ! find /cargo-home/a341-clang-root/usr/lib -name 'libclang.so' -print -quit | grep -q .; then
  bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
  printf '%s\n' $? > /evidence/libclang.exit
else
  printf '%s\n' reused > /evidence/libclang.exit
  find /cargo-home/a341-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/libclang-dir.txt
fi
if ! grep -q 'reused' /evidence/libclang.exit && ! grep -qx '0' /evidence/libclang.exit; then
  phase "libclang_exit=$(cat /evidence/libclang.exit)"
  exit 1
fi
libdir=$(cat /evidence/libclang-dir.txt)
multiarch=/cargo-home/a341-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a341-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
devso=$(find /cargo-home/a341-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a341-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a341-clang-root/usr/share/pkgconfig"
printf 'LIBCLANG_PATH=%s\nllvm_inc=%s\n' "$LIBCLANG_PATH" "$llvm_inc" > /evidence/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "libclang headers missing" >&2
  printf '%s\n' 1 > /evidence/libclang.exit
  phase "libclang_exit=1"
  exit 1
fi
phase "libclang_ready"

phase "fetch_begin"
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
phase "fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

lock_unchanged() {
  if ! cmp -s /src/Cargo.lock /evidence/Cargo.lock.frozen; then
    echo "cargo rewrote the archive Cargo.lock" >&2
    printf '%s\n' 6 > /evidence/lock-unchanged.exit
    exit 6
  fi
  if [ -f /target/work/Cargo.lock ] && ! cmp -s /target/work/Cargo.lock /evidence/Cargo.lock.frozen; then
    echo "cargo rewrote the work Cargo.lock" >&2
    printf '%s\n' 6 > /evidence/lock-unchanged.exit
    exit 6
  fi
}

phase "baseline_pack_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --lib pack_catalog -- --test-threads=1 \
  > /evidence/baseline-pack.stdout 2> /evidence/baseline-pack.stderr
base_pack_exit=$?
set -e
printf '%s\n' "$base_pack_exit" > /evidence/baseline-pack.exit
phase "baseline_pack_exit=$base_pack_exit"
lock_unchanged
if [ "$base_pack_exit" -ne 0 ]; then
  exit "$base_pack_exit"
fi
if ! grep -q 'pack_catalog::' /evidence/baseline-pack.stdout; then
  echo "baseline filter matched no pack_catalog test" >&2
  printf '%s\n' 2 > /evidence/baseline-pack.exit
  exit 2
fi

phase "baseline_loopback_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_loopback -- --test-threads=1 \
  > /evidence/baseline-loopback.stdout 2> /evidence/baseline-loopback.stderr
base_loop_exit=$?
set -e
printf '%s\n' "$base_loop_exit" > /evidence/baseline-loopback.exit
phase "baseline_loopback_exit=$base_loop_exit"
lock_unchanged
if [ "$base_loop_exit" -ne 0 ]; then
  exit "$base_loop_exit"
fi

if cmp -s /src/crates/oo-gateway/src/pack_catalog.rs /overlay/pack_catalog.rs; then
  echo "overlay matches the archive pack catalog" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
rm -rf /target/work
mkdir -p /target/work
cp -a /src/. /target/work/
cp /overlay/pack_catalog.rs /target/work/crates/oo-gateway/src/pack_catalog.rs
cp /overlay/http_gateway_pg_live.rs /target/work/crates/oo-gateway/tests/http_gateway_pg_live.rs
sha256sum /overlay/pack_catalog.rs /overlay/http_gateway_pg_live.rs \
  /overlay/gateway-compiled-pack/src/fixture.rs /overlay/gateway-compiled-pack/CONTRACT.md \
  /overlay/gateway-compiled-pack/fixtures/effective.json \
  /overlay/gateway-compiled-pack/fixtures/visibility-picker.json \
  /overlay/gateway-compiled-pack/fixtures/visibility-auditor.json \
  /overlay/gateway-compiled-pack/fixtures/visibility-hidden.json \
  > /evidence/overlay-sha256.txt
printf '%s\n' overlaid > /evidence/overlay.txt
phase "overlay_ok"
lock_unchanged

set +e
rustc --edition 2021 -O -o /target/tmp/gateway-pack-fixture /overlay/gateway-compiled-pack/src/fixture.rs \
  > /evidence/fixture-rustc.stdout 2> /evidence/fixture-rustc.stderr
fixture_rustc_exit=$?
set -e
printf '%s\n' "$fixture_rustc_exit" > /evidence/fixture-rustc.exit
phase "fixture_rustc_exit=$fixture_rustc_exit"
if [ "$fixture_rustc_exit" -ne 0 ]; then
  exit "$fixture_rustc_exit"
fi
phase "fixture_built"
export OO_A341_FIXTURE_BIN=/target/tmp/gateway-pack-fixture
export OO_A341_ROLE_PACK_BIN=/target/debug/role-pack-bundle

phase "bundle_begin"
set +e
cargo build --manifest-path /target/work/Cargo.toml --locked --offline -p oo-builder --bin role-pack-bundle \
  > /evidence/bundle.stdout 2> /evidence/bundle.stderr
bundle_exit=$?
set -e
printf '%s\n' "$bundle_exit" > /evidence/bundle.exit
phase "bundle_exit=$bundle_exit"
lock_unchanged
if [ "$bundle_exit" -ne 0 ]; then
  exit "$bundle_exit"
fi
test -x /target/debug/role-pack-bundle

phase "pack_test_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --lib pack_catalog -- --test-threads=1 \
  > /evidence/pack-test.stdout 2> /evidence/pack-test.stderr
pack_exit=$?
set -e
printf '%s\n' "$pack_exit" > /evidence/pack-test.exit
phase "pack_test_exit=$pack_exit"
lock_unchanged
if [ "$pack_exit" -ne 0 ]; then
  exit "$pack_exit"
fi
if ! grep -q 'pack_catalog::' /evidence/pack-test.stdout; then
  echo "overlay filter matched no pack_catalog test" >&2
  printf '%s\n' 2 > /evidence/pack-test.exit
  exit 2
fi

phase "loopback_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_loopback -- --test-threads=1 \
  > /evidence/loopback-test.stdout 2> /evidence/loopback-test.stderr
loop_exit=$?
set -e
printf '%s\n' "$loop_exit" > /evidence/loopback-test.exit
phase "loopback_exit=$loop_exit"
lock_unchanged
if [ "$loop_exit" -ne 0 ]; then
  exit "$loop_exit"
fi

phase "live_compile_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_pg_live -- --test-threads=1 \
  > /evidence/live-compile.stdout 2> /evidence/live-compile.stderr
live_compile_exit=$?
set -e
printf '%s\n' "$live_compile_exit" > /evidence/live-compile.exit
phase "live_compile_exit=$live_compile_exit"
lock_unchanged
if [ "$live_compile_exit" -ne 0 ]; then
  exit "$live_compile_exit"
fi

if ! rustup component list --installed | grep -q '^clippy'; then
  phase "clippy_component_begin"
  rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  printf '%s\n' $? > /evidence/clippy-component.exit
fi
phase "clippy_begin"
set +e
cargo clippy --manifest-path /target/work/Cargo.toml --locked --offline --keep-going -p oo-gateway --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
phase "clippy_exit=$clippy_exit"
lock_unchanged
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

phase "live_test_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_pg_live -- --ignored --test-threads=1 \
  > /evidence/live-test.stdout 2> /evidence/live-test.stderr
live_exit=$?
set -e
printf '%s\n' "$live_exit" > /evidence/live-test.exit
phase "live_test_exit=$live_exit"
lock_unchanged
printf '%s\n' 0 > /evidence/lock-unchanged.exit
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi
if ! grep -q '^completed=1$' /evidence/live-marker.txt; then
  echo "live marker did not record completion" >&2
  printf '%s\n' 7 > /evidence/live-marker.exit
  exit 7
fi
printf '%s\n' 0 > /evidence/live-marker.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
phase "inside_ok"
