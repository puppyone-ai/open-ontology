#!/bin/bash
# Focused gateway build after the pre-edit smoke. One builder, offline after the pin fetch.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src
mkdir -p /evidence/linux/phases /evidence/linux/lock

record() {
  printf '%s\n' "$2" > "/evidence/linux/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/compile.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/rustc-compile.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

# apt cannot drop to _apt under cap-drop ALL. Extract the debs into /tmp instead.
if [ ! -s /evidence/linux/libclang-dir.txt ]; then
  set +e
  bash /evidence/linux/libclang-extract.sh > /evidence/linux/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
else
  record libclang 0
fi
# /tmp is a noexec tmpfs. dlopen of libclang from there fails with
# "failed to map segment from shared object". The private cargo volume is exec.
rawdir=$(cat /evidence/linux/libclang-dir.txt)
if [ "${rawdir#/tmp/}" != "$rawdir" ]; then
  cp /evidence/linux/libclang-dir.txt /evidence/linux/libclang-dir-noexec.txt
  if [ ! -d /cargo-home/a175-clang-root/usr/lib ]; then
    cp -a /tmp/clang-root /cargo-home/a175-clang-root
  fi
  libdir="/cargo-home/a175-clang-root${rawdir#/tmp/clang-root}"
  printf '%s\n' "$libdir" > /evidence/linux/libclang-dir.txt
else
  libdir="$rawdir"
fi
multiarch=/cargo-home/a175-clang-root/usr/lib/aarch64-linux-gnu
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
# libclang's real .so lives under aarch64-linux-gnu, so it does not find the
# LLVM 19 resource directory next to the llvm-19 prefix. Point bindgen at it.
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I/cargo-home/a175-clang-root/usr/lib/llvm-19/lib/clang/19/include -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a175-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a175-clang-root/usr/share/pkgconfig"
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
fi
if grep -q 'not found' /evidence/linux/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  record clippy-component 0
fi

lock_pairs() {
  awk '
    /^\[\[package\]\]/ {
      if (name != "" && ver != "") print name " " ver
      name = ""
      ver = ""
    }
    $1 == "name" && name == "" { gsub(/"/, "", $3); name = $3 }
    $1 == "version" && ver == "" { gsub(/"/, "", $3); ver = $3 }
    END { if (name != "" && ver != "") print name " " ver }
  ' Cargo.lock | sort
}

# The first precise update failed because pg_query was not in the lock yet.
# That log is update-absent.101.log. Do not run a broad cargo update.
if [ -f /evidence/linux/lock/update.log ] && [ ! -f /evidence/linux/lock/update-absent.101.log ]; then
  cp /evidence/linux/lock/update.log /evidence/linux/lock/update-absent.101.log
fi
lock_pairs > /evidence/linux/lock/before.txt
cp Cargo.lock /tmp/Cargo.lock.a175-before
# `cargo fetch` has no -p flag (the usage error is fetch-pin-usage.log).
# An unlocked fetch adds the manifest delta. It is not `cargo update`.
if ! grep -q 'name = "pg_query"' Cargo.lock; then
  set +e
  cargo fetch > /evidence/linux/lock/fetch-pin.log 2>&1
  fetch_exit=$?
  set -e
  record fetch-pin "$fetch_exit"
else
  record fetch-pin 0
fi
set +e
cargo update -p pg_query --precise 6.2.1 > /evidence/linux/lock/update.log 2>&1
update_exit=$?
set -e
record update "$update_exit"
lock_pairs > /evidence/linux/lock/after.txt
comm -23 /evidence/linux/lock/before.txt /evidence/linux/lock/after.txt > /evidence/linux/lock/removed.txt
comm -13 /evidence/linux/lock/before.txt /evidence/linux/lock/after.txt > /evidence/linux/lock/added.txt
if [ -s /evidence/linux/lock/removed.txt ]; then
  echo "existing Cargo.lock versions changed; restoring" >&2
  cp /tmp/Cargo.lock.a175-before Cargo.lock
  record lock-delta 1
fi
record lock-delta 0

set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=8 \
  > /evidence/linux/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"

set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test sql_guard -- --test-threads=8 \
  > /evidence/linux/sql-guard-test.log 2>&1
guard_exit=$?
set -e
record sql-guard-test "$guard_exit"

set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/linux/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

set +e
cargo run -p oo-arch-check --locked --offline --manifest-path Cargo.toml \
  > /evidence/linux/arch-check.log 2>&1
arch_exit=$?
set -e
record arch-check "$arch_exit"

printf '%s\n' 0 > /evidence/linux/compile.exit
echo "compile_ok"
