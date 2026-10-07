#!/bin/bash
# Fixed archive 99e93d0f. No overlay and no product edit.
# Targeted smoke, focused Clippy, then only OO_A326_LIVE=1 --ignored.
# The compiled-pack live test is not run.
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
expect_sha() {
  file=$1
  want=$2
  got=$(sha256sum "$file" | awk '{print $1}')
  if [ "$got" != "$want" ]; then
    echo "sha256 mismatch for $file" >&2
    printf '%s\n' 2 > /evidence/archive.exit
    exit 2
  fi
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

if [ ! -f /src/crates/oo-gateway/tests/http_gateway_live.rs ]; then
  echo "archive is missing the maintained live test" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if grep -q 'fixture_artifact(' /src/crates/oo-gateway/tests/http_gateway_live.rs; then
  echo "archive still calls fixture_artifact" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
expect_sha /src/crates/oo-gateway/tests/http_gateway_live.rs 59e0a940f6fed9f63e7ebc0da37ddc189f287bab17e50ca794b5316eb41c9059
expect_sha /src/crates/oo-gateway/src/pack_catalog.rs d68ce8c113f9028d2dac659c332879c9eac875cc687f2fb28c7c02eaf880f5d4
expect_sha /src/Cargo.lock b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474
expect_sha /src/testing/real-db/gateway-compiled-pack/src/fixture.rs a408755dd5c22a5cb199cb269ce01267609cf1398f92f0de0baa70bd90c86e2c
expect_sha /src/testing/real-db/gateway-compiled-pack/CONTRACT.md 6fe5448908e1ab1686342ee5d728ba75732bf4b41c55766149d95d46c87f1729
cp /src/Cargo.lock /evidence/Cargo.lock.frozen
sha256sum /src/Cargo.lock /src/crates/oo-gateway/src/pack_catalog.rs \
  /src/crates/oo-gateway/tests/http_gateway_live.rs \
  /src/crates/oo-gateway/tests/http_gateway_pg_live.rs \
  /src/crates/oo-builder/src/canonical.rs \
  /src/testing/real-db/gateway-compiled-pack/src/fixture.rs \
  /src/testing/real-db/gateway-compiled-pack/fixtures/effective.json \
  /src/testing/real-db/gateway-compiled-pack/fixtures/visibility-picker.json \
  /src/testing/real-db/gateway-compiled-pack/fixtures/visibility-auditor.json \
  > /evidence/archive-sha256.txt
printf '%s\n' 'fixed-archive-no-overlay' > /evidence/archive.txt
printf '%s\n' 0 > /evidence/archive.exit
phase "archive_ok"

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
}
lock_unchanged

phase "smoke_default_begin"
rm -f /evidence/a326-live-marker.txt
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_live -- --test-threads=1 \
  > /evidence/smoke-default.stdout 2> /evidence/smoke-default.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke-default.exit
phase "smoke_default_exit=$smoke_exit"
lock_unchanged
if [ "$smoke_exit" -ne 0 ]; then
  exit "$smoke_exit"
fi
if ! grep -q '1 ignored' /evidence/smoke-default.stdout; then
  echo "default cargo test did not leave the entrypoint ignored" >&2
  printf '%s\n' 2 > /evidence/smoke-default.exit
  exit 2
fi
if [ -f /evidence/a326-live-marker.txt ]; then
  echo "default cargo test executed the ignored entrypoint" >&2
  printf '%s\n' 2 > /evidence/smoke-default.exit
  exit 2
fi
phase "smoke_ok"

if ! rustup component list --installed | grep -q '^clippy'; then
  phase "clippy_component_begin"
  rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  printf '%s\n' $? > /evidence/clippy-component.exit
fi
phase "clippy_begin"
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline \
  -p oo-gateway --bin oo-gateway --lib --test http_gateway_live -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
phase "clippy_exit=$clippy_exit"
lock_unchanged
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

phase "fixture_rustc_begin"
set +e
rustc --edition 2021 -O -o /target/tmp/gateway-pack-fixture \
  /src/testing/real-db/gateway-compiled-pack/src/fixture.rs \
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
export OO_A341_FIXTURE_DIR=/src/testing/real-db/gateway-compiled-pack/fixtures

phase "bundle_begin"
set +e
cargo build --manifest-path /src/Cargo.toml --locked --offline -p oo-builder --bin role-pack-bundle \
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

phase "a326_live_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_live -- --ignored --test-threads=1 \
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
printf '%s\n' 'not_rerun no_concrete_concern loader_and_compile_pack_unchanged' > /evidence/pg-live-not-rerun.txt

pat=/target/tmp/secret-patterns
: > "$pat"
printf '%s\n' 'ooct_' 'oost_' >> "$pat"
tr -d '\n' < /fixture/token-runtime.pass >> "$pat"
printf '\n' >> "$pat"
tr -d '\n' < /fixture/token-admin.pass >> "$pat"
printf '\n' >> "$pat"
if grep -F -q -f "$pat" /evidence/a326-live.stdout /evidence/a326-live.stderr /evidence/a326-live-marker.txt /evidence/smoke-default.stdout /evidence/smoke-default.stderr; then
  echo "generated logs contain a secret or a bearer prefix" >&2
  printf '%s\n' 8 > /evidence/secret-scan.exit
  exit 8
fi
rm -f "$pat"
printf '%s\n' 0 > /evidence/secret-scan.exit
printf '%s\n' 0 > /evidence/lock-unchanged.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
phase "inside_ok"
