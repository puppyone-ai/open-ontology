#!/bin/bash
# Focused pre-overlay smoke of the three frozen benchmark crates. No review harness.
set -u
phase() {
  printf '%s\n' "$2" > "/evidence/${1}.exit"
}
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/bench
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
export PATH="/usr/local/cargo/bin:${PATH}"

rustc --version > /evidence/rustc.txt 2>&1
phase rustc $?
cargo --version > /evidence/cargo.txt 2>&1
phase cargo $?
rustc -vV > /evidence/rustc-vv.txt 2>&1
phase rustc-vv $?

if [ -f /src/rust-toolchain.toml ]; then
  echo "toolchain file still selects the repo channel" >&2
  phase toolchain 2
  exit 2
fi
phase toolchain 0
test -f /src/benchmarks/result-comparator/src/lib.rs
test -f /src/benchmarks/query-result-adapter/src/lib.rs
test -f /src/benchmarks/trial-statistics/src/lib.rs
test ! -e /src/benchmarks/sql-runner
test ! -e /src/harness
phase archive-shape 0

(
  cd /src
  sha256sum \
    benchmarks/result-comparator/src/lib.rs \
    benchmarks/result-comparator/src/compare.rs \
    benchmarks/result-comparator/src/parse.rs \
    benchmarks/result-comparator/src/main.rs \
    benchmarks/result-comparator/CONTRACT.md \
    benchmarks/query-result-adapter/src/lib.rs \
    benchmarks/query-result-adapter/src/parse.rs \
    benchmarks/query-result-adapter/src/main.rs \
    benchmarks/query-result-adapter/CONTRACT.md \
    benchmarks/trial-statistics/src/lib.rs \
    benchmarks/trial-statistics/src/stats.rs \
    benchmarks/trial-statistics/src/parse.rs \
    benchmarks/trial-statistics/src/main.rs \
    benchmarks/trial-statistics/CONTRACT.md \
    Cargo.toml \
    Cargo.lock
) > /evidence/product-sha256.txt
phase product-sha256 0

fetch_one() {
  local name="$1"
  local manifest="$2"
  set +e
  cargo fetch --locked --offline --manifest-path "$manifest" \
    > "/evidence/fetch-${name}-offline.stdout" 2> "/evidence/fetch-${name}-offline.stderr"
  local off=$?
  set -e
  printf '%s\n' "$off" > "/evidence/fetch-${name}-offline.exit"
  if [ "$off" -ne 0 ]; then
    set +e
    cargo fetch --locked --manifest-path "$manifest" \
      > "/evidence/fetch-${name}.stdout" 2> "/evidence/fetch-${name}.stderr"
    local on=$?
    set -e
    printf '%s\n' "$on" > "/evidence/fetch-${name}.exit"
    if [ "$on" -ne 0 ]; then
      return "$on"
    fi
  else
    printf '%s\n' 0 > "/evidence/fetch-${name}.exit"
  fi
}

test_one() {
  local name="$1"
  local manifest="$2"
  set +e
  cargo test --manifest-path "$manifest" --locked --offline -- --test-threads=1 \
    > "/evidence/test-${name}.stdout" 2> "/evidence/test-${name}.stderr"
  local code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/test-${name}.exit"
  return "$code"
}

fetch_one comparator /src/benchmarks/result-comparator/Cargo.toml || exit $?
fetch_one adapter /src/benchmarks/query-result-adapter/Cargo.toml || exit $?
fetch_one trials /src/benchmarks/trial-statistics/Cargo.toml || exit $?
test_one comparator /src/benchmarks/result-comparator/Cargo.toml || exit $?
test_one adapter /src/benchmarks/query-result-adapter/Cargo.toml || exit $?
test_one trials /src/benchmarks/trial-statistics/Cargo.toml || exit $?
echo smoke_ok
