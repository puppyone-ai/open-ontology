#!/bin/bash
# Post-edit focused tests on e6ebf93 plus the a327 numeric overlay.
# Linux arm64 Rust 1.94. No cargo update. No apt.
set -u
echo "oo_task=sp06-numeric-query-postedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a327"
echo "oo_fixed=e6ebf93b5ac261c097a24d145611e0300e4f88b5"
echo "oo_platform=linux/arm64"
echo "phase=postedit"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a327
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/postedit/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/postedit/phases
  printf '%s\n' "$2" > "/evidence/linux/postedit/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/postedit/postedit.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/postedit/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
export PATH="/cargo-home/bin:${PATH}"

for path in \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/tests/a327_numeric.rs
do
  if [ ! -f "$path" ]; then
    echo "overlay missing $path" >&2
    record overlay 2
  fi
done
/cargo-home/bin/sha256 file \
  crates/oo-gateway/src/reviewed_policy.rs \
  crates/oo-gateway/src/query_executor.rs \
  crates/oo-gateway/src/bin/query-pg.rs \
  crates/oo-gateway/tests/a327_numeric.rs \
  Cargo.lock \
  > /evidence/linux/postedit/overlay-sha256.txt
record overlay 0

if [ ! -s /evidence/linux/smoke/libclang-dir.txt ] || [ ! -d /cargo-home/a327-clang-root/usr/lib ]; then
  echo "libclang root from the pre-edit smoke is missing" >&2
  record libclang 1
fi
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a327-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a327-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a327-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a327-clang-root/usr/share/pkgconfig"
real=$(find /cargo-home/a327-clang-root/usr/lib -name 'libclang-19.so.19' -print -quit)
devso=$(find /cargo-home/a327-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  export LIBCLANG_PATH="$(dirname "$devso")"
fi
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -z "$real" ] || [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang 1
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/postedit/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/postedit/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/postedit/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=1 \
  > /evidence/linux/postedit/test-lib.log 2>&1
test_exit=$?
set -e
record test-lib "$test_exit"

echo "=== cargo test -p oo-gateway --test a327_numeric ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test a327_numeric -- --test-threads=1 \
  > /evidence/linux/postedit/test-a327.log 2>&1
a327_exit=$?
set -e
record test-a327 "$a327_exit"

echo "=== cargo test -p oo-gateway --bin query-pg ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin query-pg -- --test-threads=1 \
  > /evidence/linux/postedit/test-query-pg.log 2>&1
bin_exit=$?
set -e
record test-query-pg "$bin_exit"

echo "=== cargo clippy -p oo-gateway --lib --bins ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --bins -- -D warnings \
  > /evidence/linux/postedit/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

echo "=== unowned query_executor_pg_live --no-run ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_pg_live --no-run \
  > /evidence/linux/postedit/seam-query-executor-pg-live.log 2>&1
seam_exit=$?
set -e
printf '%s\n' "$seam_exit" > /evidence/linux/postedit/phases/seam-query-executor-pg-live.exit
echo "phase_exit=$seam_exit phase=seam-query-executor-pg-live"
if [ "$seam_exit" -eq 0 ]; then
  echo "unowned live test compiled; Cell match seam did not fire" >&2
  record seam-query-executor-pg-live 2
fi

echo "=== build reviewed bins for the quantity seam ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder --bin reviewed-effective-oos \
  > /evidence/linux/postedit/build-builder.log 2>&1
builder_exit=$?
set -e
record build-builder "$builder_exit"
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin reviewed-policy \
  > /evidence/linux/postedit/build-reviewed-policy.log 2>&1
policy_exit=$?
set -e
record build-reviewed-policy "$policy_exit"

export OO_A254_OO_BUILDER=/target/a327/debug/oo-builder
export OO_A254_REVIEWED_EFFECTIVE=/target/a327/debug/reviewed-effective-oos
export OO_A254_REVIEWED_POLICY=/target/a327/debug/reviewed-policy
echo "=== unowned reviewed_policy_quantity numeric case ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_quantity -- --exact wrong_physical_quantity_is_semantic --test-threads=1 \
  > /evidence/linux/postedit/seam-reviewed-policy-quantity.log 2>&1
qty_exit=$?
set -e
printf '%s\n' "$qty_exit" > /evidence/linux/postedit/phases/seam-reviewed-policy-quantity.exit
echo "phase_exit=$qty_exit phase=seam-reviewed-policy-quantity"
if [ "$qty_exit" -eq 0 ]; then
  echo "unowned quantity test passed; numeric refusal seam did not fire" >&2
  record seam-reviewed-policy-quantity 2
fi

before=16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a
after=$(/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}')
printf 'before=%s\nafter=%s\n' "$before" "$after" > /evidence/linux/postedit/lock-hash.txt
if [ "$before" != "$after" ]; then
  record lock-hash 1
fi
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/postedit/postedit.exit
echo "postedit=ok"
exit 0
