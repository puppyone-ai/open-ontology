#!/bin/bash
# Overlay proof for reviewed-policy on fixed baf70c5c plus the three owned sources.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL.
# Origin: a234 linux/smoke/inside.sh, itself from a227 smoke and retry2.
# Changes: require the CLI sources, build the producer bins and reviewed-policy, run the CLI test, then clippy -D warnings.
set -u
echo "oo_task=sp06-reviewed-policy-cli-retry1"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a234"
echo "oo_fixed=baf70c5c7b6d01183a178f53c77f65da4a7bb497"
echo "oo_platform=linux/arm64"
echo "phase=retry1"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/retry1/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/retry1/phases
  printf '%s\n' "$2" > "/evidence/linux/retry1/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/retry1/retry1.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/retry1/rustc-version.txt
rustc -Vv > /evidence/linux/retry1/rustc-verbose.txt
cargo -V | tee /evidence/linux/retry1/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/src/bin/reviewed-policy.rs ]; then
  echo "overlay archive is missing reviewed-policy.rs" >&2
  record source 2
fi
if [ ! -f crates/oo-gateway/tests/reviewed_policy_cli.rs ]; then
  echo "overlay archive is missing the CLI test" >&2
  record source 2
fi
if [ ! -f contracts/gateway/reviewed-policy-cli-v1.md ]; then
  echo "overlay archive is missing the CLI contract" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "overlay_present" > /evidence/linux/retry1/overlay-present.txt

/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/bin/reviewed-policy.rs \
  crates/oo-gateway/tests/reviewed_policy_cli.rs \
  contracts/gateway/reviewed-policy-cli-v1.md \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/Cargo.toml \
  Cargo.lock \
  > /evidence/linux/retry1/owned-sha256.txt
record owned-hash $?
/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/retry1/lock-before.sha256

if [ ! -d /cargo-home/a234-clang-root/usr/lib ]; then
  echo "private clang root missing" >&2
  record libclang 1
fi
libfound=$(find /cargo-home/a234-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
if [ -z "$libfound" ]; then
  echo "libclang shared object missing" >&2
  record libclang 1
fi
dirname "$libfound" > /evidence/linux/retry1/libclang-dir.txt
echo "libclang_root=reused" > /evidence/linux/retry1/libclang-extract.log
record libclang 0
libdir=$(cat /evidence/linux/retry1/libclang-dir.txt)
multiarch=/cargo-home/a234-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a234-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a234-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a234-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a234-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a234-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ]; then
  echo "libclang-19.so.19 missing under the private clang root" >&2
  record libclang-load 1
fi
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/retry1/libclang-path.txt
/lib/ld-linux-aarch64.so.1 --list "$real" > /evidence/linux/retry1/libclang-ldd.txt 2>&1 || true
if grep -Eq 'not found|error while loading' /evidence/linux/retry1/libclang-ldd.txt; then
  record libclang-load 1
fi
record libclang-load 0
printf '%s\n' "no" > /evidence/linux/retry1/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/retry1/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/retry1/clippy-component.log 2>&1
clippy_add=$?
set -e
record clippy-component "$clippy_add"

echo "=== cargo build producer bins and reviewed-policy ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos \
  > /evidence/linux/retry1/build.log 2>&1
producer_exit=$?
cargo build --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin reviewed-policy \
  >> /evidence/linux/retry1/build.log 2>&1
policy_exit=$?
set -e
if [ "$producer_exit" -ne 0 ]; then
  record build "$producer_exit"
fi
record build "$policy_exit"
export OO_A234_OO_BUILDER=/target/debug/oo-builder
export OO_A234_REVIEWED_EFFECTIVE=/target/debug/reviewed-effective-oos
export OO_A234_REVIEWED_POLICY=/target/debug/reviewed-policy
if [ ! -x "$OO_A234_OO_BUILDER" ] || [ ! -x "$OO_A234_REVIEWED_EFFECTIVE" ] || [ ! -x "$OO_A234_REVIEWED_POLICY" ]; then
  echo "bins are missing" >&2
  record bins 2
fi
printf '%s\n' "$OO_A234_OO_BUILDER" > /evidence/linux/retry1/bin-oo-builder.txt
printf '%s\n' "$OO_A234_REVIEWED_EFFECTIVE" > /evidence/linux/retry1/bin-reviewed-effective.txt
printf '%s\n' "$OO_A234_REVIEWED_POLICY" > /evidence/linux/retry1/bin-reviewed-policy.txt
record bins 0

echo "=== cargo test -p oo-gateway --test reviewed_policy_cli ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1 \
  > /evidence/linux/retry1/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 6 passed' /evidence/linux/retry1/test.log; then
  echo "expected 6 reviewed-policy CLI tests" >&2
  record test-count 1
fi
record test-count 0

echo "=== cargo clippy reviewed-policy ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin reviewed-policy --test reviewed_policy_cli -- -D warnings \
  > /evidence/linux/retry1/clippy.stdout 2> /evidence/linux/retry1/clippy.stderr
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/retry1/lock-after.sha256
before=$(awk '{print $1}' /evidence/linux/retry1/lock-before.sha256)
after=$(awk '{print $1}' /evidence/linux/retry1/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/retry1/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/retry1/lock-hash.txt
record lock-hash 0

if grep -n 'unsafe' crates/oo-gateway/src/bin/reviewed-policy.rs > /evidence/linux/retry1/unsafe-lines.txt; then
  echo "reviewed-policy.rs contains an unsafe token" >&2
  record unsafe 2
fi
printf '%s\n' 0 > /evidence/linux/retry1/phases/unsafe.exit

printf '%s\n' 0 > /evidence/linux/retry1/retry1.exit
echo "retry1=ok"
exit 0
