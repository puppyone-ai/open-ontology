#!/bin/bash
# Overlay proof. Fixed archive a48db85 plus analyze-pg. One private PostgreSQL.
# Linux arm64 Rust 1.94. No cargo update. Jobs = 1.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src
mkdir -p /evidence/live /cargo-home/bin
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
rustc +1.94.0 -vV | tee /evidence/rustc-version.txt
echo "rustc_vv_exit=${PIPESTATUS[0]}"
cargo +1.94.0 -V | tee /evidence/cargo-version.txt
uname -a | tee /evidence/uname.txt
printf '%s\n' no > /evidence/cargo-update-invoked.txt

if [ ! -f crates/oo-pipeline/src/bin/analyze-pg.rs ] \
  || [ ! -f crates/oo-pipeline/tests/analyze_pg_cli.rs ] \
  || [ ! -f contracts/pipeline/analyze-pg-cli-v1.md ] \
  || [ ! -f crates/oo-gateway/tests/analyze_pg_catalog_proof.rs ]
then
  echo "overlay missing" >&2
  phase overlay-present 2
  exit 2
fi
phase overlay-present 0

echo "=== base hashes ==="
set +e
sha256sum -c /smoke-base-sha256.txt > /evidence/base-sha256.txt 2>&1
base_exit=$?
set -e
phase base-match "$base_exit"
sha256sum Cargo.lock > /evidence/lock-before.txt
sha256sum \
  crates/oo-pipeline/src/bin/analyze-pg.rs \
  crates/oo-pipeline/tests/analyze_pg_cli.rs \
  contracts/pipeline/analyze-pg-cli-v1.md \
  crates/oo-gateway/tests/analyze_pg_catalog_proof.rs \
  > /evidence/overlay-sha256.txt

echo "=== hash tools ==="
rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
phase sha256-tool $?
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
phase select-debs-tool $?
rustc --edition 2021 /tools/sha256_manifest.rs -o /cargo-home/bin/sha256_manifest
phase manifest-tool $?
export PATH="/cargo-home/bin:${PATH}"

echo "=== cargo fetch locked offline ==="
set +e
cargo +1.94.0 fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
phase fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== cargo test analyze_pg_cli ==="
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test analyze_pg_cli \
  -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
phase test "$test_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
phase clippy-component "$component_exit"

echo "=== cargo clippy analyze-pg ==="
set +e
cargo +1.94.0 clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline \
  --bin analyze-pg --test analyze_pg_cli -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
phase clippy "$clippy_exit"

if [ "$test_exit" -ne 0 ]; then
  phase live not_run
  phase catalog not_run
  sha256sum Cargo.lock > /evidence/lock-after.txt
  if cmp -s /evidence/lock-before.txt /evidence/lock-after.txt; then
    phase lock-unchanged 0
  else
    phase lock-unchanged 2
  fi
  exit "$test_exit"
fi

echo "=== cargo test analyze_pg live ==="
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test analyze_pg_cli \
  -- --ignored --test-threads=1 \
  > /evidence/live.stdout 2> /evidence/live.stderr
live_exit=$?
set -e
phase live "$live_exit"

if [ "$live_exit" -ne 0 ]; then
  phase libclang not_run
  phase catalog not_run
else
  echo "=== libclang ==="
  set +e
  bash /extract-libclang.sh > /evidence/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  phase libclang "$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    phase catalog not_run
  else
    libdir=$(cat /evidence/libclang/libclang-dir.txt)
    multiarch=/cargo-home/a236-clang-root/usr/lib/aarch64-linux-gnu
    llvm_inc=/cargo-home/a236-clang-root/usr/lib/llvm-19/lib/clang/19/include
    export LIBCLANG_PATH="$libdir"
    export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
    {
      echo "LIBCLANG_PATH=$LIBCLANG_PATH"
      test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
    } > /evidence/libclang/libclang.txt
    echo "=== policy catalog proof ==="
    set +e
    cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test analyze_pg_catalog_proof \
      -- --ignored --test-threads=1 \
      > /evidence/catalog.stdout 2> /evidence/catalog.stderr
    catalog_exit=$?
    set -e
    phase catalog "$catalog_exit"
  fi
fi

echo "=== secret scan ==="
leak=0
for facts in /evidence/live/facts.txt /evidence/live/catalog-facts.txt; do
  if [ ! -f "$facts" ]; then
    echo "missing $facts" >&2
    leak=1
    continue
  fi
  for secret_file in /run/analyze-secrets/*.pass; do
    secret=$(tr -d '[:space:]' < "$secret_file")
    if grep -q -F "$secret" "$facts"; then
      echo "password text appeared in evidence" >&2
      leak=1
    fi
  done
done
phase secret-scan "$leak"

sha256sum Cargo.lock > /evidence/lock-after.txt
if cmp -s /evidence/lock-before.txt /evidence/lock-after.txt; then
  phase lock-unchanged 0
else
  echo "Cargo.lock changed" >&2
  phase lock-unchanged 2
fi

if [ -f /evidence/live/facts.txt ]; then
  /cargo-home/bin/sha256_manifest /evidence/live /evidence/live-sha256.txt || phase live-hash 1
fi

if [ "$final" -ne 0 ]; then
  exit "$final"
fi
exit 0
