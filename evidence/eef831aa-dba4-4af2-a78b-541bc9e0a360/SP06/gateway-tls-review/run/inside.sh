#!/bin/bash
# Fresh archive smoke, then the frozen focused tests, then the review oracle.
# The review file is absent until PHASE review_overlay.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export MAKEFLAGS=-j1
export CMAKE_BUILD_PARALLEL_LEVEL=1
mkdir -p /target/tmp /evidence /cargo-home/bin /target/work
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

rm -rf /target/work
mkdir -p /target/work
cp -a /src/. /target/work/
cd /target/work
if [ -f crates/oo-gateway/tests/http_gateway_tls_review.rs ]; then
  echo "review test was already in the archive" >&2
  printf '%s\n' 2 > /evidence/pre-edit.exit
  exit 2
fi
find crates/oo-gateway/tests -maxdepth 1 -type f -printf '%f\n' | sort > /evidence/tests-before.txt
printf '%s\n' 0 > /evidence/pre-edit.exit
phase "PHASE pre_edit_review_absent"

sha256sum \
  crates/oo-gateway/src/http_gateway.rs \
  crates/oo-gateway/src/bin/oo-gateway.rs \
  crates/oo-gateway/tests/http_gateway_tls.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/work-sha256.txt
awk '{print $1}' /evidence/archive-sha256.txt > /tmp/archive.hashes
awk '{print $1}' /evidence/work-sha256.txt > /tmp/work.hashes
if ! cmp -s /tmp/archive.hashes /tmp/work.hashes; then
  echo "work tree does not match the mounted archive" >&2
  printf '%s\n' 2 > /evidence/archive-match.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/archive-match.exit
phase "PHASE archive_matched"

if [ ! -x /cargo-home/bin/sha256 ] || [ ! -x /cargo-home/bin/select-debs ]; then
  rustc --edition 2021 -O -o /cargo-home/bin/sha256 /tools/sha256.rs
  rustc --edition 2021 -O -o /cargo-home/bin/select-debs /tools/select-debs.rs
fi
if ! find /cargo-home/a326-clang-root/usr/lib -name 'libclang.so' -print -quit | grep -q .; then
  bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
  printf '%s\n' $? > /evidence/libclang.exit
else
  printf '%s\n' reused > /evidence/libclang.exit
  find /cargo-home/a326-clang-root/usr/lib -name 'libclang.so' -print -quit | xargs dirname > /evidence/libclang-dir.txt
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
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "llvm stdbool.h is absent" >&2
  printf '%s\n' 1 > /evidence/libclang-includes.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/libclang-includes.exit
phase "PHASE libclang_ready"

cp /target/work/Cargo.lock /evidence/Cargo.lock.frozen
sha256sum /target/work/Cargo.lock > /evidence/lock-sha256.txt
lock_unchanged() {
  if ! cmp -s /target/work/Cargo.lock /evidence/Cargo.lock.frozen; then
    echo "cargo rewrote Cargo.lock" >&2
    printf '%s\n' 6 > /evidence/lock-unchanged.exit
    exit 6
  fi
}

phase "PHASE smoke_begin"
set +e
cargo check --manifest-path /target/work/Cargo.toml --locked -p oo-gateway \
  > /evidence/smoke.stdout 2> /evidence/smoke.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke.exit
phase "PHASE smoke_exit=$smoke_exit"
lock_unchanged
if [ "$smoke_exit" -ne 0 ]; then
  exit "$smoke_exit"
fi
if [ -f crates/oo-gateway/tests/http_gateway_tls_review.rs ]; then
  echo "review test appeared during smoke" >&2
  exit 2
fi
phase "PHASE smoke_before_review_overlay"

if ! rustup component list --installed | grep -q '^clippy'; then
  phase "PHASE clippy_component_begin"
  rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  printf '%s\n' $? > /evidence/clippy-component.exit
  if [ "$(cat /evidence/clippy-component.exit)" != "0" ]; then
    exit 1
  fi
else
  printf '%s\n' present > /evidence/clippy-component.exit
fi

phase "PHASE clippy_begin"
set +e
cargo clippy --manifest-path /target/work/Cargo.toml --locked --keep-going -p oo-gateway \
  --lib --bin oo-gateway \
  --test http_gateway_loopback --test http_gateway_tls \
  --test http_gateway_pg_live --test http_gateway_live \
  -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
phase "PHASE clippy_exit=$clippy_exit"
lock_unchanged
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

phase "PHASE loopback_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked -p oo-gateway --test http_gateway_loopback -- --test-threads=1 \
  > /evidence/loopback-test.stdout 2> /evidence/loopback-test.stderr
loop_exit=$?
set -e
printf '%s\n' "$loop_exit" > /evidence/loopback-test.exit
phase "PHASE loopback_exit=$loop_exit"
lock_unchanged
if [ "$loop_exit" -ne 0 ]; then
  exit "$loop_exit"
fi

phase "PHASE tls_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked -p oo-gateway --test http_gateway_tls -- --test-threads=1 \
  > /evidence/tls-test.stdout 2> /evidence/tls-test.stderr
tls_exit=$?
set -e
printf '%s\n' "$tls_exit" > /evidence/tls-test.exit
phase "PHASE tls_exit=$tls_exit"
lock_unchanged
if [ "$tls_exit" -ne 0 ]; then
  exit "$tls_exit"
fi
if ! grep -q '5 passed' /evidence/tls-test.stdout || ! grep -q '1 ignored' /evidence/tls-test.stdout; then
  echo "focused tls count was not 5 passed and 1 ignored" >&2
  printf '%s\n' 2 > /evidence/tls-count.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/tls-count.exit

phase "PHASE fixture_begin"
set +e
rustc --edition 2021 -O -o /target/tmp/gateway-pack-fixture /target/work/testing/real-db/gateway-compiled-pack/src/fixture.rs \
  > /evidence/fixture-rustc.stdout 2> /evidence/fixture-rustc.stderr
fixture_exit=$?
set -e
printf '%s\n' "$fixture_exit" > /evidence/fixture-rustc.exit
phase "PHASE fixture_exit=$fixture_exit"
if [ "$fixture_exit" -ne 0 ]; then
  exit "$fixture_exit"
fi

phase "PHASE bundle_begin"
set +e
cargo build --manifest-path /target/work/Cargo.toml --locked -p oo-builder --bin role-pack-bundle \
  > /evidence/bundle.stdout 2> /evidence/bundle.stderr
bundle_exit=$?
set -e
printf '%s\n' "$bundle_exit" > /evidence/bundle.exit
phase "PHASE bundle_exit=$bundle_exit"
lock_unchanged
if [ "$bundle_exit" -ne 0 ]; then
  exit "$bundle_exit"
fi
test -x /target/debug/role-pack-bundle

export OO_A350_FIXTURE_BIN=/target/tmp/gateway-pack-fixture
export OO_A350_ROLE_PACK_BIN=/target/debug/role-pack-bundle
export OO_A360_FIXTURE_BIN=/target/tmp/gateway-pack-fixture
export OO_A360_ROLE_PACK_BIN=/target/debug/role-pack-bundle

phase "PHASE a350_live_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked -p oo-gateway --test http_gateway_tls -- --ignored --test-threads=1 \
  verified_peer_metadata_uses_the_compiled_pack_and_the_leaf_uri \
  > /evidence/a350-live.stdout 2> /evidence/a350-live.stderr
a350_exit=$?
set -e
printf '%s\n' "$a350_exit" > /evidence/a350-live.exit
phase "PHASE a350_live_exit=$a350_exit"
lock_unchanged
if [ "$a350_exit" -ne 0 ]; then
  exit "$a350_exit"
fi

phase "PHASE review_overlay"
cp /review/review_tls.rs /target/work/crates/oo-gateway/tests/http_gateway_tls_review.rs
find crates/oo-gateway/tests -maxdepth 1 -type f -printf '%f\n' | sort > /evidence/tests-after.txt
sha256sum /target/work/crates/oo-gateway/tests/http_gateway_tls_review.rs > /evidence/review-test-sha256.txt
if ! cmp -s /review/review_tls.rs /target/work/crates/oo-gateway/tests/http_gateway_tls_review.rs; then
  echo "review overlay mismatch" >&2
  exit 2
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

phase "PHASE review_live_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked -p oo-gateway --test http_gateway_tls_review -- --ignored --test-threads=1 \
  > /evidence/review-live.stdout 2> /evidence/review-live.stderr
review_live_exit=$?
set -e
printf '%s\n' "$review_live_exit" > /evidence/review-live.exit
phase "PHASE review_live_exit=$review_live_exit"
lock_unchanged
if [ "$review_live_exit" -ne 0 ]; then
  exit "$review_live_exit"
fi

grep -q '1.94.0' /evidence/rustc-version.txt
grep -E 'axum = \{ version = "=0.8.9"' Cargo.toml > /evidence/pins.txt
grep -E 'rustls = \{ version = "=0.23.45"' crates/oo-gateway/Cargo.toml >> /evidence/pins.txt
grep -E 'tokio-rustls = \{ version = "=0.26.5"' crates/oo-gateway/Cargo.toml >> /evidence/pins.txt
printf '%s\n' 0 > /evidence/lock-unchanged.exit
printf '%s\n' 0 > /evidence/inside.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
phase "PHASE inside_ok"
