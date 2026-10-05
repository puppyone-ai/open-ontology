#!/bin/bash
# Pre-edit public oo-pipeline and oo-gateway lib smoke on Linux arm64 Rust 1.94.
# The mounted tree is the fixed archive 0ee7cf86. No cargo update. Lock stays --locked.
set -u
echo "oo_task=sp11-pg-vertical-integration-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a203"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=preedit"
echo "fixed_base=0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-pipeline/Cargo.toml ] || [ ! -f crates/oo-gateway/Cargo.toml ]; then
  echo "snapshot missing cargo inputs" >&2
  exit 2
fi
if [ ! -f crates/oo-pipeline/src/full_import.rs ] || [ ! -f crates/oo-gateway/src/policy.rs ]; then
  echo "ancestor pipeline or gateway sources missing" >&2
  exit 2
fi
if [ -f crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "preedit saw the uncommitted query CLI; refusing the moving tree" >&2
  exit 2
fi
mkdir -p /evidence/linux/phases
git_sha=$(cat /src/.fixed-base-sha 2>/dev/null || true)
printf '%s\n' "$git_sha" > /evidence/linux/fixed-base.txt

record() {
  printf '%s\n' "$2" > "/evidence/linux/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

if [ ! -s /evidence/linux/libclang-dir.txt ]; then
  set +e
  bash /evidence/linux/libclang-extract.sh > /evidence/linux/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  record libclang 0
fi
rawdir=$(cat /evidence/linux/libclang-dir.txt)
if [ "${rawdir#/tmp/}" != "$rawdir" ]; then
  cp /evidence/linux/libclang-dir.txt /evidence/linux/libclang-dir-noexec.txt
  if [ ! -d /cargo-home/a203-clang-root/usr/lib ]; then
    cp -a /tmp/clang-root /cargo-home/a203-clang-root
  fi
  libdir="/cargo-home/a203-clang-root${rawdir#/tmp/clang-root}"
  printf '%s\n' "$libdir" > /evidence/linux/libclang-dir.txt
else
  libdir="$rawdir"
fi
multiarch=/cargo-home/a203-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a203-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a203-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a203-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  echo "BINDGEN_EXTRA_CLANG_ARGS=$BINDGEN_EXTRA_CLANG_ARGS"
  ls -l "$libdir"/libclang.so* || true
  pkg-config --modversion libclang || true
  /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" || true
} > /evidence/linux/libclang.txt
if ! /lib/ld-linux-aarch64.so.1 --list "${multiarch}/libclang-19.so.19" > /evidence/linux/libclang-ldd.txt 2>&1; then
  record libclang-load 1
  exit 1
fi
if grep -q 'not found' /evidence/linux/libclang-ldd.txt; then
  record libclang-load 1
  exit 1
fi
record libclang-load 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke-fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

run_phase() {
  local name="$1"
  local logfile="$2"
  shift 2
  echo "=== $* ==="
  set +e
  "$@" > "/evidence/linux/${logfile}" 2>&1
  local status=$?
  set -e
  record "$name" "$status"
  tail -30 "/evidence/linux/${logfile}" || true
  return "$status"
}

fail=0
run_phase test-pipeline lib-pipeline.log \
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib \
  || fail=$?
run_phase test-gateway lib-gateway.log \
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib \
  || fail=$?
run_phase clippy-pipeline clippy-pipeline.log \
  cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib -- -D warnings \
  || fail=$?
run_phase clippy-gateway clippy-gateway.log \
  cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- -D warnings \
  || fail=$?
exit "$fail"
