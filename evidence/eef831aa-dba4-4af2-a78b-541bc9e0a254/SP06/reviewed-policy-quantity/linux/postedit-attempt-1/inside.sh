#!/bin/bash
# Focused post-edit proof. Work tree is the fixed archive plus the quantity arm and the new test.
set -u
echo "oo_task=sp06-reviewed-policy-quantity-postedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a254"
echo "oo_fixed=c828e2781fc37eb62249fab54fa0b7fff86f0249"
echo "oo_platform=linux/arm64"
echo "phase=postedit"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/postedit/phases
cd /work

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

sha=$(cat /work/.fixed-base-sha)
if [ "$sha" != "c828e2781fc37eb62249fab54fa0b7fff86f0249" ]; then
  echo "work tree sha mismatch" >&2
  record source 2
fi
if [ -f /work/rust-toolchain.toml ]; then
  echo "toolchain file was not moved aside" >&2
  record source 2
fi
for path in \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/bin/reviewed-policy.rs \
  crates/oo-gateway/tests/reviewed_policy.rs \
  crates/oo-gateway/tests/reviewed_policy_cli.rs \
  Cargo.toml \
  Cargo.lock
do
  if ! cmp -s "/work/$path" "/src/$path"; then
    echo "overlay changed $path" >&2
    record source 2
  fi
done
if cmp -s /work/crates/oo-gateway/src/reviewed_policy.rs /src/crates/oo-gateway/src/reviewed_policy.rs; then
  echo "quantity arm was not applied" >&2
  record source 2
fi
awk '
  $0 ~ /"int2" \| "int4" \| "int8" => matches!/ {on=1}
  on {print}
  on && $0 ~ /"text" =>/ {exit}
' /work/crates/oo-gateway/src/reviewed_policy.rs > /evidence/linux/postedit/integer-arm.txt
if ! grep -q 'SemanticType::Quantity' /evidence/linux/postedit/integer-arm.txt; then
  echo "work integer arm does not accept quantity" >&2
  record source 2
fi
if ! grep -q 'SemanticType::StatusCode' /evidence/linux/postedit/integer-arm.txt; then
  echo "work integer arm dropped the historical cell" >&2
  record source 2
fi
test -f /work/crates/oo-gateway/tests/reviewed_policy_quantity.rs
record source 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a254-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a254-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$(cat /evidence/linux/smoke/libclang-path.txt)"
export LD_LIBRARY_PATH="${LIBCLANG_PATH}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a254-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a254-clang-root/usr/share/pkgconfig"
printf '%s\n' "no" > /evidence/linux/postedit/cargo-update-invoked.txt

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/postedit/lock-before.sha256

echo "=== rebuild reviewed-policy from the quantity overlay ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-gateway --bin reviewed-policy \
  > /evidence/linux/postedit/build-reviewed-policy.log 2>&1
build_exit=$?
set -e
record build-reviewed-policy "$build_exit"
cp /target/debug/oo-builder /target/debug/reviewed-effective-oos /target/debug/reviewed-policy \
  /target/debug/oo-pipeline /target/debug/init-pg /target/debug/analyze-pg \
  /target/debug/query-pg /target/debug/install-column-acl \
  /cargo-home/a254-bins/
record bin-copy $?
/cargo-home/bin/sha256 file \
  /cargo-home/a254-bins/oo-builder \
  /cargo-home/a254-bins/reviewed-effective-oos \
  /cargo-home/a254-bins/reviewed-policy \
  /cargo-home/a254-bins/oo-pipeline \
  /cargo-home/a254-bins/init-pg \
  /cargo-home/a254-bins/analyze-pg \
  /cargo-home/a254-bins/query-pg \
  /cargo-home/a254-bins/install-column-acl \
  > /evidence/linux/postedit/bin-sha256.txt
record bin-hash $?

export OO_A227_OO_BUILDER=/cargo-home/a254-bins/oo-builder
export OO_A227_REVIEWED_EFFECTIVE=/cargo-home/a254-bins/reviewed-effective-oos
export OO_A234_OO_BUILDER=/cargo-home/a254-bins/oo-builder
export OO_A234_REVIEWED_EFFECTIVE=/cargo-home/a254-bins/reviewed-effective-oos
export OO_A234_REVIEWED_POLICY=/cargo-home/a254-bins/reviewed-policy
export OO_A254_OO_BUILDER=/cargo-home/a254-bins/oo-builder
export OO_A254_REVIEWED_EFFECTIVE=/cargo-home/a254-bins/reviewed-effective-oos
export OO_A254_REVIEWED_POLICY=/cargo-home/a254-bins/reviewed-policy

echo "=== boundary reviewed_policy ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy -- --test-threads=1 \
  > /evidence/linux/postedit/test-reviewed-policy.log 2>&1
test_exit=$?
set -e
record test-reviewed-policy "$test_exit"
if ! grep -q 'test result: ok. 21 passed' /evidence/linux/postedit/test-reviewed-policy.log; then
  echo "expected 21 boundary adapter tests" >&2
  record test-count 1
fi
record test-count 0

echo "=== boundary reviewed_policy_cli ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_cli -- --test-threads=1 \
  > /evidence/linux/postedit/test-reviewed-policy-cli.log 2>&1
cli_exit=$?
set -e
record test-reviewed-policy-cli "$cli_exit"
if ! grep -q 'test result: ok. 6 passed' /evidence/linux/postedit/test-reviewed-policy-cli.log; then
  echo "expected 6 boundary CLI tests" >&2
  record cli-count 1
fi
record cli-count 0

echo "=== reviewed_policy_quantity ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test reviewed_policy_quantity -- --test-threads=1 \
  > /evidence/linux/postedit/test-reviewed-policy-quantity.log 2>&1
qty_exit=$?
set -e
record test-reviewed-policy-quantity "$qty_exit"
if ! grep -q 'test result: ok. 6 passed' /evidence/linux/postedit/test-reviewed-policy-quantity.log; then
  echo "expected 6 quantity tests" >&2
  record quantity-count 1
fi
record quantity-count 0

echo "=== clippy gateway lib, reviewed-policy, quantity test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --lib --bin reviewed-policy --test reviewed_policy_quantity -- -D warnings \
  > /evidence/linux/postedit/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/postedit/lock-after.sha256
before=$(cat /evidence/linux/postedit/lock-before.sha256)
after=$(cat /evidence/linux/postedit/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/postedit/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/postedit/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/postedit/postedit.exit
echo "postedit=ok"
exit 0
