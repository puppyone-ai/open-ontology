#!/bin/bash
# Focused oo-gateway parser-worker tests on Linux arm64 Rust 1.94.
# Writable copy of the pre-edit archive plus the owned overlay. No cargo update. No apt.
set -u
echo "oo_task=sp06-parser-worker-focused-2"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a187"
echo "oo_fixed=8a289f0c699abb8c707ca45ad45cb036abc3fc05"
echo "oo_platform=linux/arm64"
echo "phase=focused-2"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_NET_OFFLINE=true
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src

record() {
  mkdir -p /evidence/linux/focused-2/phases
  printf '%s\n' "$2" > "/evidence/linux/focused-2/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/focused-2/focused.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/focused-2/rustc-version.txt
rustc -Vv > /evidence/linux/focused-2/rustc-verbose.txt
cargo -V | tee /evidence/linux/focused-2/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

if [ ! -f crates/oo-gateway/src/parser_worker.rs ]; then
  echo "overlay parser_worker.rs is absent" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/src/bin/oo-sql-parser-worker.rs ]; then
  echo "overlay worker binary is absent" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/parser_worker.rs ]; then
  echo "overlay parser_worker test is absent" >&2
  record source 2
fi
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/focused-2/lock-before.sha256
mismatch=0
while read -r sum path; do
  [ -n "$sum" ] || continue
  got_sum=$(sha256sum "$path" | awk '{print $1}')
  if [ "$got_sum" != "$sum" ]; then
    echo "hash mismatch $path" >&2
    mismatch=1
  fi
done < /expected.sha256
if [ "$mismatch" -ne 0 ]; then
  record overlay-hash 1
fi
record overlay-hash 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a187-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a187-clang-root/usr/lib/llvm-19/lib/clang/19/include
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "libclang tree from the pre-edit smoke is absent" >&2
  record libclang 1
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/a187-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a187-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present"
} > /evidence/linux/focused-2/libclang.txt
record libclang 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/focused-2/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/focused-2/clippy-component.log
  record clippy-component 0
fi
printf '%s\n' "no" > /evidence/linux/focused-2/cargo-update-invoked.txt

echo "=== cargo test parser_worker sql_guard lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test parser_worker --test sql_guard -- --test-threads=4 \
  > /evidence/linux/focused-2/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== cargo clippy -p oo-gateway --all-targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings \
  > /evidence/linux/focused-2/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/focused-2/lock-after.sha256
if ! cmp -s /evidence/linux/focused-2/lock-before.sha256 /evidence/linux/focused-2/lock-after.sha256; then
  echo "Cargo.lock changed" > /evidence/linux/focused-2/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged" > /evidence/linux/focused-2/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/focused-2/focused.exit
echo "focused-2=ok"
exit 0
