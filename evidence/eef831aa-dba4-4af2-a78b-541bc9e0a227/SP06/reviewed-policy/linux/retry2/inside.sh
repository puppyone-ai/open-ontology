#!/bin/bash
# Overlay proof for the reviewed policy adapter on fixed ed661809 plus owned sources.
# Linux arm64 Rust 1.94. Builds oo-builder inside this archive. No cargo update. No apt. No PostgreSQL.
# Origin: a227 linux/red/inside.sh, itself from a218 smoke inside.sh a27b5d7d091584df6e0406f570c6570d215f1ab2.
# Changes: require the adapter sources, build both bins, run reviewed_policy, then clippy -D warnings.
set -u
echo "oo_task=sp06-reviewed-policy-overlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a227"
echo "oo_fixed=ed661809cf6c1e8df37af5842838dd1eb839c98a"
echo "oo_platform=linux/arm64"
echo "phase=overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/retry2/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/retry2/phases
  printf '%s\n' "$2" > "/evidence/linux/retry2/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/retry2/overlay.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/retry2/rustc-version.txt
rustc -Vv > /evidence/linux/retry2/rustc-verbose.txt
cargo -V | tee /evidence/linux/retry2/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
record select-debs-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/src/reviewed_policy.rs ]; then
  echo "overlay archive is missing reviewed_policy.rs" >&2
  record source 2
fi
if ! grep -q 'mod reviewed_policy' crates/oo-gateway/src/lib.rs; then
  echo "overlay lib.rs does not export reviewed_policy" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/reviewed_policy.rs ]; then
  echo "overlay archive is missing the reviewed_policy test" >&2
  record source 2
fi
if [ ! -f contracts/gateway/reviewed-policy-v1.md ]; then
  echo "overlay archive is missing the reviewed-policy contract" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "overlay_present" > /evidence/linux/retry2/overlay-present.txt

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/tests/reviewed_policy.rs \
  contracts/gateway/reviewed-policy-v1.md \
  Cargo.lock \
  > /evidence/linux/retry2/owned-sha256.txt
record owned-hash $?
/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/retry2/lock-before.sha256

if [ -d /cargo-home/a227-clang-root/usr/lib ]; then
  libfound=$(find /cargo-home/a227-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
  test -n "$libfound"
  dirname "$libfound" > /evidence/linux/retry2/libclang-dir.txt
  echo "libclang_root=reused" > /evidence/linux/retry2/libclang-extract.log
  record libclang 0
else
  set +e
  bash /extract-libclang.sh > /evidence/linux/retry2/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  record libclang "$extract_exit"
fi
libdir=$(cat /evidence/linux/retry2/libclang-dir.txt)
multiarch=/cargo-home/a227-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a227-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a227-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a227-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  echo "LD_LIBRARY_PATH=$LD_LIBRARY_PATH"
  ls -l "$libdir"/libclang.so* || true
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/retry2/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
real=$(find /cargo-home/a227-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a227-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ]; then
  echo "libclang-19.so.19 missing under the private clang root" >&2
  record libclang-load 1
fi
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/retry2/libclang-path.txt
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/retry2/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/retry2/libclang-ldd.txt; then
  record libclang-load 1
fi
if ! grep -q 'libLLVM' /evidence/linux/retry2/libclang-ldd.txt; then
  echo "libclang did not resolve LLVM" >&2
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/retry2/cargo-update-invoked.txt

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/retry2/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/retry2/clippy-component.log 2>&1
clippy_add=$?
set -e
record clippy-component "$clippy_add"

echo "=== cargo build oo-builder bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos \
  > /evidence/linux/retry2/build.log 2>&1
build_exit=$?
set -e
record build "$build_exit"
export OO_A227_OO_BUILDER=/target/debug/oo-builder
export OO_A227_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos
if [ ! -x "$OO_A227_OO_BUILDER" ] || [ ! -x "$OO_A227_REVIEWED_EFFECTIVE" ]; then
  echo "builder bins are missing" >&2
  record bins 2
fi
printf '%s\n' "$OO_A227_OO_BUILDER" > /evidence/linux/retry2/bin-oo-builder.txt
printf '%s\n' "$OO_A227_REVIEWED_EFFECTIVE" > /evidence/linux/retry2/bin-reviewed-effective.txt

echo "=== cargo test -p oo-gateway --test reviewed_policy ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1 \
  > /evidence/linux/retry2/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== cargo clippy -p oo-gateway --lib --test reviewed_policy -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test reviewed_policy -- -D warnings \
  > /evidence/linux/retry2/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/retry2/lock-after.sha256
if ! cmp -s /evidence/linux/retry2/lock-before.sha256 /evidence/linux/retry2/lock-after.sha256; then
  echo "Cargo.lock changed during the proof" >&2
  record lock 2
fi
record lock 0
printf '%s\n' 0 > /evidence/linux/retry2/overlay.exit
echo "overlay_proof=pass"
exit 0
