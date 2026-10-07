#!/bin/bash
# Fixed archive 6001e18e smoke, then the review file. Cargo stays --locked.
# The product tree is not edited. /tmp is noexec. Scratch is on the target volume.
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

if [ ! -f /src/crates/oo-gateway/src/pack_catalog.rs ] || [ ! -f /src/crates/oo-gateway/tests/http_gateway_pg_live.rs ]; then
  echo "archive is missing the fixed gateway sources" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/testing/real-db/gateway-compiled-pack/src/fixture.rs ]; then
  echo "archive is missing the compiler fixture" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -f /src/crates/oo-gateway/tests/review_pack.rs ]; then
  echo "archive already contains the review test" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
printf '%s\n' present > /evidence/archive.txt
cp /src/Cargo.lock /evidence/Cargo.lock.frozen
sha256sum /src/Cargo.lock /src/crates/oo-gateway/src/pack_catalog.rs \
  /src/crates/oo-gateway/tests/http_gateway_live.rs \
  /src/crates/oo-gateway/tests/http_gateway_pg_live.rs \
  /src/crates/oo-gateway/tests/http_gateway_loopback.rs \
  /src/crates/oo-builder/src/canonical.rs \
  /src/testing/real-db/gateway-compiled-pack/CONTRACT.md \
  /src/testing/real-db/gateway-compiled-pack/src/fixture.rs \
  > /evidence/archive-sha256.txt
grep -n 'fixture_artifact' /src/crates/oo-gateway/tests/http_gateway_live.rs > /evidence/old-entrypoint.txt

if [ ! -x /cargo-home/bin/sha256 ] || [ ! -x /cargo-home/bin/select-debs ]; then
  rustc --edition 2021 -O -o /cargo-home/bin/sha256 /tools/sha256.rs
  rustc --edition 2021 -O -o /cargo-home/bin/select-debs /tools/select-debs.rs
fi
if ! find /cargo-home/a345-clang-root/usr/lib -name 'libclang.so' -print -quit | grep -q .; then
  bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
  printf '%s\n' $? > /evidence/libclang.exit
else
  printf '%s\n' reused > /evidence/libclang.exit
  find /cargo-home/a345-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/libclang-dir.txt
fi
if ! grep -q 'reused' /evidence/libclang.exit && ! grep -qx '0' /evidence/libclang.exit; then
  phase "libclang_exit=$(cat /evidence/libclang.exit)"
  exit 1
fi
libdir=$(cat /evidence/libclang-dir.txt)
multiarch=/cargo-home/a345-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a345-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
devso=$(find /cargo-home/a345-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a345-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a345-clang-root/usr/share/pkgconfig"
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

phase "smoke_pack_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --lib pack_catalog -- --test-threads=1 \
  > /evidence/smoke-pack.stdout 2> /evidence/smoke-pack.stderr
smoke_pack_exit=$?
set -e
printf '%s\n' "$smoke_pack_exit" > /evidence/smoke-pack.exit
phase "smoke_pack_exit=$smoke_pack_exit"
lock_unchanged
if [ "$smoke_pack_exit" -ne 0 ]; then
  exit "$smoke_pack_exit"
fi
if ! grep -q '6 passed' /evidence/smoke-pack.stdout; then
  echo "fixed archive pack_catalog did not pass 6 tests" >&2
  printf '%s\n' 2 > /evidence/smoke-pack.exit
  exit 2
fi

phase "smoke_loopback_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_loopback -- --test-threads=1 \
  > /evidence/smoke-loopback.stdout 2> /evidence/smoke-loopback.stderr
smoke_loop_exit=$?
set -e
printf '%s\n' "$smoke_loop_exit" > /evidence/smoke-loopback.exit
phase "smoke_loopback_exit=$smoke_loop_exit"
lock_unchanged
if [ "$smoke_loop_exit" -ne 0 ]; then
  exit "$smoke_loop_exit"
fi
if ! grep -q '8 passed' /evidence/smoke-loopback.stdout; then
  echo "fixed archive loopback did not pass 8 tests" >&2
  printf '%s\n' 2 > /evidence/smoke-loopback.exit
  exit 2
fi

phase "smoke_pg_compile_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_pg_live -- --test-threads=1 \
  > /evidence/smoke-pg-live.stdout 2> /evidence/smoke-pg-live.stderr
smoke_pg_exit=$?
set -e
printf '%s\n' "$smoke_pg_exit" > /evidence/smoke-pg-live.exit
phase "smoke_pg_compile_exit=$smoke_pg_exit"
lock_unchanged
if [ "$smoke_pg_exit" -ne 0 ]; then
  exit "$smoke_pg_exit"
fi
if ! grep -q '1 ignored' /evidence/smoke-pg-live.stdout; then
  echo "approved live test was not left ignored" >&2
  printf '%s\n' 2 > /evidence/smoke-pg-live.exit
  exit 2
fi

phase "smoke_old_default_begin"
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_live -- --test-threads=1 \
  > /evidence/smoke-old-live.stdout 2> /evidence/smoke-old-live.stderr
smoke_old_exit=$?
set -e
printf '%s\n' "$smoke_old_exit" > /evidence/smoke-old-live.exit
phase "smoke_old_default_exit=$smoke_old_exit"
lock_unchanged
if [ "$smoke_old_exit" -ne 0 ]; then
  exit "$smoke_old_exit"
fi
if ! grep -q '0 passed; 0 failed; 1 ignored' /evidence/smoke-old-live.stdout; then
  echo "default run did not ignore the maintained entrypoint" >&2
  printf '%s\n' 2 > /evidence/smoke-old-live.exit
  exit 2
fi

if ! rustup component list --installed | grep -q '^clippy'; then
  phase "clippy_component_begin"
  rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  printf '%s\n' $? > /evidence/clippy-component.exit
fi
phase "smoke_clippy_begin"
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/smoke-clippy.stdout 2> /evidence/smoke-clippy.stderr
smoke_clippy_exit=$?
set -e
printf '%s\n' "$smoke_clippy_exit" > /evidence/smoke-clippy.exit
phase "smoke_clippy_exit=$smoke_clippy_exit"
lock_unchanged
if [ "$smoke_clippy_exit" -ne 0 ]; then
  exit "$smoke_clippy_exit"
fi
if [ -f /src/crates/oo-gateway/tests/review_pack.rs ]; then
  echo "review test appeared in the archive during smoke" >&2
  exit 2
fi
phase "smoke_ok"

rm -rf /target/work
mkdir -p /target/work
cp -a /src/. /target/work/
cp /review/review_pack.rs /target/work/crates/oo-gateway/tests/review_pack.rs
sha256sum /review/review_pack.rs > /evidence/review-test-sha256.txt
if [ -f /src/crates/oo-gateway/tests/review_pack.rs ]; then
  echo "copy wrote into the archive" >&2
  exit 2
fi
printf '%s\n' 'review_test_only' > /evidence/overlay.txt
phase "review_test_copied"
lock_unchanged

set +e
rustc --edition 2021 -O -o /target/tmp/gateway-pack-fixture /src/testing/real-db/gateway-compiled-pack/src/fixture.rs \
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

phase "pg_live_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_pg_live -- --ignored --test-threads=1 \
  > /evidence/pg-live.stdout 2> /evidence/pg-live.stderr
pg_live_exit=$?
set -e
printf '%s\n' "$pg_live_exit" > /evidence/pg-live.exit
phase "pg_live_exit=$pg_live_exit"
lock_unchanged
if [ "$pg_live_exit" -ne 0 ]; then
  exit "$pg_live_exit"
fi
if ! grep -q '^completed=1$' /evidence/live-marker.txt; then
  echo "approved live marker did not record completion" >&2
  printf '%s\n' 7 > /evidence/live-marker.exit
  exit 7
fi
printf '%s\n' 0 > /evidence/live-marker.exit

phase "old_live_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_live -- --ignored --test-threads=1 \
  > /evidence/old-live.stdout 2> /evidence/old-live.stderr
old_live_exit=$?
set -e
printf '%s\n' "$old_live_exit" > /evidence/old-live.exit
phase "old_live_exit=$old_live_exit"
lock_unchanged
{
  echo "entrypoint=crates/oo-gateway/tests/http_gateway_live.rs"
  echo "test=issued_token_reads_its_pack_and_failures_stay_closed"
  echo "env=OO_A326_LIVE=1"
  echo "exit=${old_live_exit}"
} > /evidence/old-live-assessment.txt
if [ "$old_live_exit" -eq 0 ]; then
  echo "result=passed" >> /evidence/old-live-assessment.txt
  echo "maintained entrypoint passed" >&2
  exit 9
fi
if grep -q 'legacy_unsupported' /evidence/old-live.stdout /evidence/old-live.stderr; then
  echo "result=legacy_unsupported" >> /evidence/old-live-assessment.txt
else
  echo "result=unexpected" >> /evidence/old-live-assessment.txt
  echo "maintained entrypoint failed for a different reason" >&2
  exit "$old_live_exit"
fi

phase "review_pack_begin"
set +e
cargo test --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --test review_pack -- --test-threads=1 \
  > /evidence/review-pack.stdout 2> /evidence/review-pack.stderr
review_exit=$?
set -e
printf '%s\n' "$review_exit" > /evidence/review-pack.exit
phase "review_pack_exit=$review_exit"
lock_unchanged
if [ "$review_exit" -ne 0 ]; then
  exit "$review_exit"
fi
if ! grep -q '^binding_reason=binding$' /evidence/review-summary.txt; then
  echo "review summary missing the binding refusal" >&2
  printf '%s\n' 2 > /evidence/review-pack.exit
  exit 2
fi

phase "clippy_begin"
set +e
cargo clippy --manifest-path /target/work/Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
phase "clippy_exit=$clippy_exit"
lock_unchanged
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

umask 077
pat=/target/tmp/secret-patterns
{
  tr -d '\r\n' < /fixture/token-runtime.pass
  printf '\n'
  tr -d '\r\n' < /fixture/token-admin.pass
  printf '\n'
  printf '%s\n' 'ooct_' 'oost_'
} > "$pat"
set +e
find /evidence -type f \( -name '*.stdout' -o -name '*.stderr' -o -name '*.log' -o -name 'live-marker.txt' -o -name 'old-live-marker.txt' -o -name 'review-summary.txt' \) -print0 \
  | xargs -0 grep -F -f "$pat" -q
scan_exit=$?
set -e
rm -f "$pat"
if [ "$scan_exit" -eq 0 ]; then
  echo "generated logs contain a secret or a bearer prefix" >&2
  printf '%s\n' 8 > /evidence/secret-scan.exit
  exit 8
fi
printf '%s\n' 0 > /evidence/secret-scan.exit
printf '%s\n' 0 > /evidence/lock-unchanged.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
phase "inside_ok"
