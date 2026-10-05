#!/bin/bash
# Review harness on a fresh archive of baf70c5c. Product bytes are the commit. No PostgreSQL.
set -u
echo "oo_task=sp06-reviewed-policy-review-overlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a233"
echo "oo_fixed=baf70c5c7b6d01183a178f53c77f65da4a7bb497"
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
mkdir -p /evidence/linux/overlay/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/overlay/phases
  printf '%s\n' "$2" > "/evidence/linux/overlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/overlay/overlay.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/overlay/rustc-version.txt
rustc -Vv > /evidence/linux/overlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/overlay/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -f crates/oo-gateway/tests/reviewed_policy_a233_review.rs ]; then
  echo "review harness is missing" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "review_harness_present" > /evidence/linux/overlay/overlay-present.txt
cmp -s crates/oo-gateway/src/reviewed_policy.rs /blobs/reviewed_policy.rs \
  && cmp -s crates/oo-gateway/src/lib.rs /blobs/lib.rs \
  && cmp -s crates/oo-gateway/tests/reviewed_policy.rs /blobs/reviewed_policy_test.rs \
  && cmp -s contracts/gateway/reviewed-policy-v1.md /blobs/reviewed-policy-v1.md \
  && cmp -s contracts/builder/reviewed-effective-oos-v1.md /blobs/reviewed-effective-oos-v1.md \
  && cmp -s crates/oo-gateway/Cargo.toml /blobs/oo-gateway-Cargo.toml \
  && cmp -s Cargo.lock /blobs/Cargo.lock \
  && cmp -s crates/oo-gateway/tests/reviewed_policy_a233_review.rs /evidence/overlay/reviewed_policy_a233_review.rs
record blob-cmp $?

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/tests/reviewed_policy.rs \
  crates/oo-gateway/tests/reviewed_policy_a233_review.rs \
  contracts/gateway/reviewed-policy-v1.md \
  Cargo.lock \
  > /evidence/linux/overlay/owned-sha256.txt
record owned-hash $?
/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/overlay/lock-before.sha256

if [ ! -d /cargo-home/a233-clang-root/usr/lib ]; then
  echo "smoke should have left the private clang root" >&2
  record libclang 2
fi
libfound=$(find /cargo-home/a233-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libfound"
dirname "$libfound" > /evidence/linux/overlay/libclang-dir.txt
echo "libclang_root=reused" > /evidence/linux/overlay/libclang-extract.log
record libclang 0
libdir=$(cat /evidence/linux/overlay/libclang-dir.txt)
multiarch=/cargo-home/a233-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a233-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a233-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a233-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  ls -l "$libdir"/libclang.so* || true
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/overlay/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang-includes 1
fi
record libclang-includes 0
real=$(find /cargo-home/a233-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a233-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ]; then
  echo "libclang-19.so.19 missing under the private clang root" >&2
  record libclang-load 1
fi
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/overlay/libclang-path.txt
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/overlay/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/overlay/libclang-ldd.txt; then
  record libclang-load 1
fi
if ! grep -q 'libLLVM' /evidence/linux/overlay/libclang-ldd.txt; then
  echo "libclang did not resolve LLVM" >&2
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/overlay/cargo-update-invoked.txt

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/overlay/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/overlay/clippy-component.log 2>&1
clippy_add=$?
set -e
record clippy-component "$clippy_add"

echo "=== cargo build oo-builder bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos \
  > /evidence/linux/overlay/build.log 2>&1
build_exit=$?
set -e
record build "$build_exit"
export OO_A233_OO_BUILDER=/target/debug/oo-builder
export OO_A233_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos
if [ ! -x "$OO_A233_OO_BUILDER" ] || [ ! -x "$OO_A233_REVIEWED_EFFECTIVE" ]; then
  echo "builder bins are missing" >&2
  record bins 2
fi
printf '%s\n' "$OO_A233_OO_BUILDER" > /evidence/linux/overlay/bin-oo-builder.txt
printf '%s\n' "$OO_A233_REVIEWED_EFFECTIVE" > /evidence/linux/overlay/bin-reviewed-effective.txt
record bins 0

echo "=== cargo test reviewed_policy_a233_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_a233_review -- --test-threads=1 --nocapture \
  > /evidence/linux/overlay/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test reviewed_policy --test reviewed_policy_a233_review -- -D warnings \
  > /evidence/linux/overlay/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/overlay/lock-after.sha256
if ! cmp -s /evidence/linux/overlay/lock-before.sha256 /evidence/linux/overlay/lock-after.sha256; then
  echo "Cargo.lock changed during the overlay" >&2
  record lock 2
fi
record lock 0
printf '%s\n' 0 > /evidence/linux/overlay/overlay.exit
echo "overlay=ok"
exit 0
