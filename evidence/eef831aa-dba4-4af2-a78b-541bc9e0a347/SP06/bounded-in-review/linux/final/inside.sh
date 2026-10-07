#!/bin/bash
# Independent review run. Archive 6f87942 plus the review harness only.
# Existing sql_guard tests, then the review harness. Locked and offline.
set -u
echo "oo_task=sp06-bounded-in-review-final"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a347"
echo "oo_fixed=6f87942f1c66ba1a9787b7d7194e58a507af63ef"
echo "oo_platform=linux/arm64"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a347
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export CARGO_NET_OFFLINE=true
unset OO_A175_LIVE || true
unset OO_A343_LIVE || true
mkdir -p /evidence/linux/final/phases /cargo-home/bin /evidence/live
cd /src

record() {
  mkdir -p /evidence/linux/final/phases
  printf '%s\n' "$2" > "/evidence/linux/final/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/final/final.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/final/rustc-version.txt
rustc -Vv > /evidence/linux/final/rustc-verbose.txt
cargo -V | tee /evidence/linux/final/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -f crates/oo-gateway/tests/review_bounded_in.rs ]; then
  echo "review harness is missing" >&2
  record harness 2
fi
export PATH="/cargo-home/bin:${PATH}"
/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/sql_guard.rs \
  crates/oo-gateway/tests/bounded_in.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  crates/oo-gateway/tests/review_bounded_in.rs \
  > /evidence/linux/final/owned-sha256.txt
if ! cmp -s /evidence/linux/final/owned-sha256.txt /evidence/linux/final/expected-sha256.txt; then
  echo "mounted bytes differ from the review pin" >&2
  diff -u /evidence/linux/final/expected-sha256.txt /evidence/linux/final/owned-sha256.txt >&2 || true
  record pins 1
fi
record pins 0

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a347-clang-root/usr/lib ]; then
  echo "libclang root from the smoke is missing" >&2
  record libclang 1
fi
echo "libclang_root=reused" > /evidence/linux/final/libclang-extract.log
record libclang 0
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a347-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a347-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a347-clang-root/usr/bin:${PATH}"
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
real=$(find /cargo-home/a347-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a347-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ]; then
  echo "libclang-19.so.19 missing under the private clang root" >&2
  record libclang-load 1
fi
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/final/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/final/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0

echo "=== cargo test sql_guard ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test sql_guard -- --test-threads=1 \
  > /evidence/linux/final/sql-guard.log 2>&1
guard_exit=$?
set -e
record sql-guard "$guard_exit"

echo "=== cargo test review_bounded_in ==="
set +e
env OO_A347_LIVE=1 cargo test --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test review_bounded_in -- --test-threads=1 \
  > /evidence/linux/final/review.log 2>&1
review_exit=$?
set -e
record review "$review_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/final/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/final/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo clippy lib and review harness ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --lib --test review_bounded_in -- -D warnings \
  > /evidence/linux/final/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/final/lock-after.sha256
before=$(awk '$2=="Cargo.lock"{print $1}' /evidence/linux/final/owned-sha256.txt)
after=$(cat /evidence/linux/final/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/final/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/final/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/final/final.exit
echo "final=ok"
exit 0
