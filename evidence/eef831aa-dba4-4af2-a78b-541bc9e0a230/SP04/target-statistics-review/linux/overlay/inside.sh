#!/bin/bash
# Review proof. Fixed product archive plus the two review tests. One private PostgreSQL.
# Linux arm64 Rust 1.94. No cargo update. No apt. Jobs = 1.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src
mkdir -p /evidence/live /evidence/libclang /cargo-home/bin
final=0

phase() {
  printf '%s\n' "$2" > "/evidence/$1.exit"
  echo "PHASE $1=$2"
  case "$2" in
    ''|*[!0-9]*) ;;
    *)
      if [ "$2" -ne 0 ] && [ "$final" -eq 0 ]; then
        final=$2
      fi
      ;;
  esac
}

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
uname -a
cat /etc/os-release
id

if [ ! -f crates/oo-pipeline/src/target_statistics.rs ] || [ ! -f crates/oo-pipeline/tests/target_statistics_a230_review.rs ] || [ ! -f crates/oo-gateway/tests/target_statistics_a230_catalog.rs ]; then
  echo "overlay missing the product or the review harness" >&2
  phase overlay-present 2
  exit 2
fi
phase overlay-present 0

echo "=== hash tools ==="
rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
phase sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
phase select-debs-tool $?
rustc --edition 2021 /tools/sha256_manifest.rs -o /cargo-home/bin/sha256_manifest
phase manifest-tool $?
if [ "$final" -ne 0 ]; then
  exit "$final"
fi
export PATH="/cargo-home/bin:${PATH}"
/cargo-home/bin/sha256 file \
  crates/oo-pipeline/src/target_statistics.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/tests/target_statistics_pg_live.rs \
  contracts/pipeline/target-statistics-v1.md \
  crates/oo-pipeline/tests/target_statistics_a230_review.rs \
  crates/oo-gateway/tests/target_statistics_a230_catalog.rs \
  Cargo.lock \
  > /evidence/owned-sha256.txt
phase owned-hash $?
cmp -s crates/oo-pipeline/src/target_statistics.rs /blobs/target_statistics.rs \
  && cmp -s crates/oo-pipeline/src/lib.rs /blobs/lib.rs \
  && cmp -s crates/oo-pipeline/tests/target_statistics_pg_live.rs /blobs/target_statistics_pg_live.rs \
  && cmp -s contracts/pipeline/target-statistics-v1.md /blobs/target-statistics-v1.md
phase blob-cmp $?
if [ "$final" -ne 0 ]; then
  exit "$final"
fi

echo "=== cargo test target_statistics lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib target_statistics -- --test-threads=1 \
  > /evidence/lib.stdout 2> /evidence/lib.stderr
lib_exit=$?
set -e
phase lib "$lib_exit"

echo "=== cargo test review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test target_statistics_a230_review -- --ignored --test-threads=1 \
  > /evidence/live.stdout 2> /evidence/live.stderr
live_exit=$?
set -e
phase live "$live_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
phase clippy-component "$component_exit"

echo "=== cargo clippy pipeline ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib --test target_statistics_a230_review -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
phase clippy "$clippy_exit"

if [ ! -f /evidence/live/facts.txt ]; then
  phase libclang not_run
  phase catalog not_run
  phase clippy-gateway not_run
  exit "$final"
fi

echo "=== libclang ==="
set +e
bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
extract_exit=$?
set -e
phase libclang "$extract_exit"
if [ "$extract_exit" -ne 0 ]; then
  phase catalog not_run
  phase clippy-gateway not_run
  exit "$final"
fi
libdir=$(cat /evidence/libclang/libclang-dir.txt)
multiarch=/cargo-home/a230-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a230-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/libclang/libclang.txt

echo "=== policy catalog review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test target_statistics_a230_catalog -- --ignored --test-threads=1 \
  > /evidence/catalog.stdout 2> /evidence/catalog.stderr
catalog_exit=$?
set -e
phase catalog "$catalog_exit"

echo "=== cargo clippy gateway review ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --test target_statistics_a230_catalog -- -D warnings \
  > /evidence/clippy-gateway.stdout 2> /evidence/clippy-gateway.stderr
gateway_clippy=$?
set -e
phase clippy-gateway "$gateway_clippy"

echo "=== secret scan ==="
leak=0
for facts in /evidence/live/facts.txt /evidence/live/catalog-facts.txt; do
  if [ ! -f "$facts" ]; then
    echo "missing $facts" >&2
    leak=1
    continue
  fi
  for secret_file in /secrets/*.pass; do
    secret=$(tr -d '[:space:]' < "$secret_file")
    if grep -q -F -- "$secret" "$facts"; then
      echo "password text appeared in evidence" >&2
      leak=1
    fi
  done
done
phase secret-scan "$leak"

if [ -f /evidence/live/facts.txt ]; then
  /cargo-home/bin/sha256_manifest /evidence/live /evidence/live-sha256.txt
  phase live-hash $?
fi

if [ "$final" -ne 0 ]; then
  exit "$final"
fi
exit 0
