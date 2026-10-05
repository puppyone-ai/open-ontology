#!/bin/bash
# Pre-edit oo-gateway smoke on Linux arm64 Rust 1.94.
# Immutable archive. No cargo update. No apt. No source edits.
set -u
echo "oo_task=sp06-parser-worker-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a187"
echo "oo_fixed=8a289f0c699abb8c707ca45ad45cb036abc3fc05"
echo "oo_platform=linux/arm64"
echo "phase=preedit"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src

record() {
  mkdir -p /evidence/linux/smoke/phases
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke/rustc-version.txt
rustc -Vv > /evidence/linux/smoke/rustc-verbose.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

if [ ! -f crates/oo-gateway/src/sql_guard.rs ]; then
  echo "sql_guard.rs is absent from the archive" >&2
  record source 2
fi
if [ -f crates/oo-gateway/src/parser_worker.rs ]; then
  echo "preedit archive already contains parser_worker.rs" >&2
  record source 2
fi
if [ -f crates/oo-gateway/src/bin/oo-sql-parser-worker.rs ]; then
  echo "preedit archive already contains the worker binary" >&2
  record source 2
fi
printf '%s\n' "parser_worker_absent" > /evidence/linux/smoke/overlay-absent.txt
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-before.sha256

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a187-clang-root/usr/lib ]; then
  set +e
  bash /libclang-extract.sh > /evidence/linux/smoke/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  echo "libclang_root=reused" > /evidence/linux/smoke/libclang-extract.log
  record libclang 0
fi
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a187-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a187-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a187-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a187-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  echo "BINDGEN_EXTRA_CLANG_ARGS=$BINDGEN_EXTRA_CLANG_ARGS"
  ls -l "$libdir"/libclang.so* || true
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
  command -v protoc || echo "protoc=absent"
} > /evidence/linux/smoke/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
if ! /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" > /evidence/linux/smoke/libclang-ldd.txt 2>&1; then
  record libclang-load 1
fi
if grep -q 'not found' /evidence/linux/smoke/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/smoke/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/smoke/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
if ! cmp -s /evidence/linux/smoke/lock-before.sha256 /evidence/linux/smoke/lock-after.sha256; then
  echo "fetch changed Cargo.lock" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=8 \
  > /evidence/linux/smoke/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"

echo "=== cargo test -p oo-gateway --test sql_guard ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test sql_guard -- --test-threads=8 \
  > /evidence/linux/smoke/sql-guard-test.log 2>&1
guard_exit=$?
set -e
record sql-guard-test "$guard_exit"

echo "=== cargo clippy -p oo-gateway --all-targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/linux/smoke/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "preedit_smoke=ok"
exit 0
