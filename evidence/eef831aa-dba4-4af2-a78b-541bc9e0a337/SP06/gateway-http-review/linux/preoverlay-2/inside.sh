#!/bin/bash
# Read-only archive first: lock, embed bytes, 42 library tests, 8 loopback tests, Clippy.
# Private PostgreSQL must answer before the review HTTP test. The review test is
# copied onto a private tree after those checks. Cargo stays --locked --offline
# after the one fetch. This script does not rewrite the archive lock.
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
  echo "archive is missing the gateway" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
printf '%s\n' archive > /evidence/overlay.txt
sha256sum crates/oo-gateway/src/http_gateway.rs crates/oo-gateway/src/pack_catalog.rs \
  crates/oo-gateway/src/bin/oo-gateway.rs crates/oo-gateway/build.rs crates/oo-gateway/Cargo.toml \
  Cargo.lock Cargo.toml \
  > /evidence/overlay-sha256.txt

if [ ! -x /cargo-home/bin/sha256 ] || [ ! -x /cargo-home/bin/select-debs ]; then
  rustc --edition 2021 -O -o /cargo-home/bin/sha256 /tools/sha256.rs
  rustc --edition 2021 -O -o /cargo-home/bin/select-debs /tools/select-debs.rs
fi
if ! find /cargo-home/a337-clang-root/usr/lib -name 'libclang.so' -print -quit 2>/dev/null | grep -q .; then
  bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
  printf '%s\n' $? > /evidence/libclang.exit
else
  printf '%s\n' reused > /evidence/libclang.exit
  find /cargo-home/a337-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit | xargs dirname > /evidence/libclang-dir.txt
fi
if ! grep -qx '0' /evidence/libclang.exit && ! grep -qx 'reused' /evidence/libclang.exit; then
  echo "libclang extract failed" >&2
  exit 1
fi
libdir=$(cat /evidence/libclang-dir.txt)
multiarch=/cargo-home/a337-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=$(find /cargo-home/a337-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit)
gcc_inc=$(echo /usr/lib/gcc/aarch64-linux-gnu/*/include)
devso=$(find /cargo-home/a337-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a337-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a337-clang-root/usr/share/pkgconfig"
printf 'LIBCLANG_PATH=%s\nllvm_inc=%s\n' "$LIBCLANG_PATH" "$llvm_inc" > /evidence/libclang.txt
test -f "$llvm_inc/stdbool.h"
echo "PHASE libclang_ready"

rustc --edition 2021 -O -o /target/tmp/scope-review /tools/scope_review.rs
set +e
/target/tmp/scope-review /src > /evidence/scope-review.txt 2> /evidence/scope-review.stderr
scope_exit=$?
set -e
printf '%s\n' "$scope_exit" > /evidence/scope-review.exit
echo "PHASE scope_review_exit=$scope_exit"
if [ "$scope_exit" -ne 0 ]; then
  exit "$scope_exit"
fi
: > /evidence/embed-hash.txt
while read -r kind rel digest; do
  if [ "$kind" != hash ]; then
    continue
  fi
  got=$(/cargo-home/bin/sha256 file "/src/tools/gateway-token-store/$rel" | awk '{print $1}')
  printf '%s %s %s\n' "$rel" "$digest" "$got" >> /evidence/embed-hash.txt
  if [ "$got" != "$digest" ]; then
    echo "embed hash mismatch for $rel" >&2
    printf '%s\n' 8 > /evidence/embed-hash.exit
    exit 8
  fi
done < /evidence/scope-review.txt
rows=$(wc -l < /evidence/embed-hash.txt | tr -d ' ')
if [ "$rows" -ne 13 ]; then
  echo "embed hash rows=$rows" >&2
  printf '%s\n' 8 > /evidence/embed-hash.exit
  exit 8
fi
printf '%s\n' 0 > /evidence/embed-hash.exit
echo "PHASE embed_hash_exit=0"

rustc --edition 2021 -O -o /target/tmp/lock-consumer /tools/lock-consumer.rs
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
grep '^edge ' /evidence/lock-consumer.txt | sort > /evidence/edges-got.txt
cat > /evidence/edges-want.txt <<'EOF'
edge hyper 1.11.1 +httpdate
edge tower 0.5.3 +tokio-util
edge tower 0.5.3 +tracing
edge tower-http 0.6.11 +http-body-util
edge tower-http 0.6.11 +tokio
EOF
if ! cmp -s /evidence/edges-got.txt /evidence/edges-want.txt; then
  echo "feature edges are not the approved set" >&2
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

echo "=== cargo fetch ==="
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
lock_unchanged
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
if ! rustup component list --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy > /evidence/clippy-install.stdout 2> /evidence/clippy-install.stderr
  clippy_add=$?
  set -e
  printf '%s\n' "$clippy_add" > /evidence/clippy-install.exit
  if [ "$clippy_add" -ne 0 ]; then
    echo "clippy install failed" >&2
    exit "$clippy_add"
  fi
else
  printf '%s\n' present > /evidence/clippy-install.exit
fi

echo "=== cargo test lib ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=1 \
  > /evidence/lib-test.stdout 2> /evidence/lib-test.stderr
lib_exit=$?
set -e
printf '%s\n' "$lib_exit" > /evidence/lib-test.exit
echo "PHASE lib_test_exit=$lib_exit"
lock_unchanged
if [ "$lib_exit" -ne 0 ]; then
  exit "$lib_exit"
fi
if ! grep -q 'test result: ok. 42 passed' /evidence/lib-test.stdout; then
  echo "library result was not 42 passed" >&2
  printf '%s\n' 9 > /evidence/lib-count.exit
  exit 9
fi
printf '%s\n' 0 > /evidence/lib-count.exit

echo "=== cargo test loopback ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --test http_gateway_loopback -- --test-threads=1 \
  > /evidence/loopback-test.stdout 2> /evidence/loopback-test.stderr
loop_exit=$?
set -e
printf '%s\n' "$loop_exit" > /evidence/loopback-test.exit
echo "PHASE loopback_exit=$loop_exit"
lock_unchanged
if [ "$loop_exit" -ne 0 ]; then
  exit "$loop_exit"
fi
if ! grep -q 'test result: ok. 8 passed' /evidence/loopback-test.stdout; then
  echo "loopback result was not 8 passed" >&2
  printf '%s\n' 9 > /evidence/loopback-count.exit
  exit 9
fi
printf '%s\n' 0 > /evidence/loopback-count.exit

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
lock_unchanged
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

echo "=== postgres live ==="
set +e
timeout 5 bash -c 'echo >/dev/tcp/oo-a337-pg/5432'
pg_exit=$?
set -e
printf '%s\n' "$pg_exit" > /evidence/pg-live.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/pg-live.txt
echo "PHASE pg_live_exit=$pg_exit"
if [ "$pg_exit" -ne 0 ]; then
  echo "private postgres is not accepting connections" >&2
  exit 4
fi

echo "=== arch check ==="
set +e
cargo run --manifest-path /src/Cargo.toml --locked --offline -p oo-arch-check -- --manifest /src/Cargo.toml \
  > /evidence/arch-check.stdout 2> /evidence/arch-check.stderr
arch_exit=$?
set -e
printf '%s\n' "$arch_exit" > /evidence/arch-check.exit
echo "PHASE arch_check_exit=$arch_exit"
lock_unchanged
if [ "$arch_exit" -ne 0 ]; then
  exit "$arch_exit"
fi
if ! grep -q 'architecture check: pass' /evidence/arch-check.stdout; then
  echo "arch check did not pass" >&2
  printf '%s\n' 10 > /evidence/arch-check.exit
  exit 10
fi

echo "=== review tree ==="
rm -rf /review-tree
cp -a /src /review-tree
cp /tools/review_http.rs /review-tree/crates/oo-gateway/tests/review_http.rs
diff -rq /src /review-tree > /evidence/review-tree-delta.txt || true
if [ "$(wc -l < /evidence/review-tree-delta.txt | tr -d ' ')" -ne 1 ] || ! grep -q 'review_http.rs' /evidence/review-tree-delta.txt; then
  echo "review tree delta is not the one test file" >&2
  printf '%s\n' 11 > /evidence/review-tree.exit
  exit 11
fi
printf '%s\n' 0 > /evidence/review-tree.exit
if ! cmp -s /src/Cargo.lock /review-tree/Cargo.lock; then
  echo "review tree lock differs" >&2
  exit 11
fi
echo "PHASE review_tree_ready"

echo "=== clippy review test ==="
set +e
cargo clippy --manifest-path /review-tree/Cargo.toml --locked --offline -p oo-gateway --test review_http -- -D warnings \
  > /evidence/review-clippy.stdout 2> /evidence/review-clippy.stderr
review_clippy_exit=$?
set -e
printf '%s\n' "$review_clippy_exit" > /evidence/review-clippy.exit
echo "PHASE review_clippy_exit=$review_clippy_exit"
lock_unchanged
if ! cmp -s /review-tree/Cargo.lock /evidence/Cargo.lock.frozen; then
  echo "review clippy rewrote the copied lock" >&2
  exit 6
fi
if [ "$review_clippy_exit" -ne 0 ]; then
  exit "$review_clippy_exit"
fi

echo "=== independent http ==="
set +e
cargo test --manifest-path /review-tree/Cargo.toml --locked --offline -p oo-gateway --test review_http -- --ignored --test-threads=1 \
  > /evidence/review-http.stdout 2> /evidence/review-http.stderr
review_exit=$?
set -e
printf '%s\n' "$review_exit" > /evidence/review-http.exit
echo "PHASE review_http_exit=$review_exit"
lock_unchanged
if [ "$review_exit" -ne 0 ]; then
  exit "$review_exit"
fi
if ! grep -q '^completed=1$' /evidence/live-marker.txt; then
  echo "live marker did not record completion" >&2
  printf '%s\n' 7 > /evidence/live-marker.exit
  exit 7
fi
printf '%s\n' 0 > /evidence/live-marker.exit
if grep -E 'ooct_|oost_' /evidence/review-http.stdout /evidence/review-http.stderr /evidence/review-http.txt >/dev/null; then
  echo "review log contains a bearer prefix" >&2
  printf '%s\n' 12 > /evidence/bearer-log.exit
  exit 12
fi
printf '%s\n' 0 > /evidence/bearer-log.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/inside-ok.txt
echo "PHASE inside_ok"
