#!/bin/bash
# Pre-edit smoke of the immutable archive, then overlay only http_gateway_live.rs.
# Cargo stays --locked. Libclang discovery matches the a341 extractor.
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
if ! grep -q 'fixture_artifact' /src/crates/oo-gateway/tests/http_gateway_live.rs; then
  echo "archive live test is not the pre-edit entrypoint" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
grep -n 'fixture_artifact' /src/crates/oo-gateway/tests/http_gateway_live.rs > /evidence/archive-entrypoint.txt
printf '%s\n' present > /evidence/archive.txt
cp /src/Cargo.lock /evidence/Cargo.lock.frozen
sha256sum /src/Cargo.lock /src/crates/oo-gateway/src/pack_catalog.rs \
  /src/crates/oo-gateway/tests/http_gateway_live.rs \
  /src/crates/oo-builder/src/canonical.rs /src/crates/oo-builder/src/role_pack.rs \
  > /evidence/archive-sha256.txt

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

phase "baseline_old_live_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_live -- --test-threads=1 \
  > /evidence/baseline-old-live.stdout 2> /evidence/baseline-old-live.stderr
base_old_exit=$?
set -e
printf '%s\n' "$base_old_exit" > /evidence/baseline-old-live.exit
phase "baseline_old_live_exit=$base_old_exit"
lock_unchanged
if [ "$base_old_exit" -ne 0 ]; then
  exit "$base_old_exit"
fi
if ! grep -q 'ignored' /evidence/baseline-old-live.stdout; then
  echo "pre-edit live test was not left ignored" >&2
  printf '%s\n' 2 > /evidence/baseline-old-live.exit
  exit 2
fi

if cmp -s /src/crates/oo-gateway/tests/http_gateway_live.rs /overlay/http_gateway_live.rs; then
  echo "overlay matches the archive live test" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
if grep -q 'fixture_artifact(' /overlay/http_gateway_live.rs; then
  echo "overlay still calls fixture_artifact" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
rm -rf /target/work
mkdir -p /target/work
cp -a /src/. /target/work/
cp /overlay/http_gateway_live.rs /target/work/crates/oo-gateway/tests/http_gateway_live.rs
if ! cmp -s /src/crates/oo-gateway/src/pack_catalog.rs /target/work/crates/oo-gateway/src/pack_catalog.rs; then
  echo "work pack_catalog drifted from the archive" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
sha256sum /overlay/http_gateway_live.rs /src/crates/oo-gateway/src/pack_catalog.rs \
  /fixture-src/src/fixture.rs /fixture-src/fixtures/effective.json \
  /fixture-src/fixtures/visibility-picker.json /fixture-src/fixtures/visibility-auditor.json \
  /fixture-src/fixtures/visibility-hidden.json \
  > /evidence/overlay-sha256.txt
printf '%s\n' overlaid > /evidence/overlay.txt
phase "overlay_ok"
lock_unchanged

set +e
rustc --edition 2021 -O -o /target/tmp/gateway-pack-fixture /fixture-src/src/fixture.rs \
  > /evidence/fixture-rustc.stdout 2> /evidence/fixture-rustc.stderr
fixture_rustc_exit=$?
set -e
printf '%s\n' "$fixture_rustc_exit" > /evidence/fixture-rustc.exit
phase "fixture_rustc_exit=$fixture_rustc_exit"
if [ "$fixture_rustc_exit" -ne 0 ]; then
  exit "$fixture_rustc_exit"
fi
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

if ! rustup component list --installed | grep -q '^clippy'; then
  phase "clippy_component_begin"
  rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  printf '%s\n' $? > /evidence/clippy-component.exit
fi
phase "clippy_begin"
set +e
cargo clippy --manifest-path /target/work/Cargo.toml --locked --offline --keep-going \
  -p oo-gateway --bin oo-gateway --lib \
  --test http_gateway_live --test http_gateway_loopback --test http_gateway_pg_live \
  -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
phase "clippy_exit=$clippy_exit"
lock_unchanged
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

phase "a326_live_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_live -- --ignored --test-threads=1 \
  > /evidence/a326-live.stdout 2> /evidence/a326-live.stderr
a326_exit=$?
set -e
printf '%s\n' "$a326_exit" > /evidence/a326-live.exit
phase "a326_live_exit=$a326_exit"
lock_unchanged
if [ "$a326_exit" -ne 0 ]; then
  exit "$a326_exit"
fi
if ! grep -q '^completed=1$' /evidence/a326-live-marker.txt; then
  echo "a326 live marker did not record completion" >&2
  printf '%s\n' 7 > /evidence/a326-live-marker.exit
  exit 7
fi
printf '%s\n' 0 > /evidence/a326-live-marker.exit

phase "a341_live_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_pg_live -- --ignored --test-threads=1 \
  > /evidence/a341-live.stdout 2> /evidence/a341-live.stderr
a341_exit=$?
set -e
printf '%s\n' "$a341_exit" > /evidence/a341-live.exit
phase "a341_live_exit=$a341_exit"
lock_unchanged
if [ "$a341_exit" -ne 0 ]; then
  exit "$a341_exit"
fi
if ! grep -q '^completed=1$' /evidence/a341-live-marker.txt; then
  echo "compiled-pack marker did not record completion" >&2
  printf '%s\n' 7 > /evidence/a341-live-marker.exit
  exit 7
fi
printf '%s\n' 0 > /evidence/a341-live-marker.exit

if grep -E 'ooct_|oost_' /evidence/a326-live.stdout /evidence/a326-live.stderr /evidence/a341-live.stdout /evidence/a341-live.stderr >/dev/null; then
  echo "bearer prefix in a live log" >&2
  printf '%s\n' 8 > /evidence/secret-scan.exit
  exit 8
fi
printf '%s\n' 0 > /evidence/secret-scan.exit
printf '%s\n' 0 > /evidence/lock-unchanged.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
phase "inside_ok"
