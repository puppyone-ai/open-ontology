#!/bin/bash
# Live inspect and apply-reviewed against the private PostgreSQL alias.
# Product bins come from the immutable archive. The onboarding crate is the worktree.
# No cargo update. No apt. No shell children inside the CLI.
set -u
echo "oo_task=sp11-pg-onboarding-cli-chain"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a261"
echo "oo_fixed=1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368"
echo "phase=chain"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/chain/phases /evidence/live/work /cargo-home/a261-bins
cd /src

record() {
  mkdir -p /evidence/linux/chain/phases
  printf '%s\n' "$2" > "/evidence/linux/chain/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/chain/chain.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/chain/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/chain/fixed-base.txt
if [ "$sha" != "1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368" ]; then
  record source 2
fi
record source 0
printf '%s\n' "no" > /evidence/linux/chain/cargo-update-invoked.txt

libdir=$(find /cargo-home/a261-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
if [ -z "$libdir" ]; then
  echo "private clang root is absent" >&2
  record libclang 1
fi
libdir=$(dirname "$libdir")
multiarch=/cargo-home/a261-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a261-clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a261-clang-root/usr/bin:${PATH}"
record libclang 0

build_bin() {
  pkg=$1
  bin=$2
  set +e
  CARGO_TARGET_DIR=/target cargo build --manifest-path /src/Cargo.toml --locked --offline -p "$pkg" --bin "$bin" \
    > "/evidence/linux/chain/bins-build-${bin}.log" 2>&1
  bins_exit=$?
  set -e
  record "bins-${bin}" "$bins_exit"
  cp "/target/debug/${bin}" "/cargo-home/a261-bins/${bin}"
}

build_bin oo-pipeline init-pg
build_bin oo-pipeline oo-pipeline
build_bin oo-pipeline analyze-pg
build_bin oo-builder oo-builder
build_bin oo-builder reviewed-effective-oos
build_bin oo-builder role-pack-bundle
build_bin oo-gateway reviewed-policy
build_bin oo-gateway install-column-acl
build_bin oo-gateway provision-mapping-reader
build_bin oo-gateway query-pg

echo "=== pg-onboarding ==="
set +e
CARGO_TARGET_DIR=/target/onboarding cargo build --manifest-path /onboarding/Cargo.toml --locked --offline --bin pg-onboarding \
  > /evidence/linux/chain/build-onboarding.log 2>&1
onboard_exit=$?
set -e
record build-onboarding "$onboard_exit"
cp /target/onboarding/debug/pg-onboarding /cargo-home/a261-bins/pg-onboarding

export OO_A261_ONBOARDING=/cargo-home/a261-bins/pg-onboarding
export OO_A261_BINS=/cargo-home/a261-bins
export OO_A261_SECRETS=/secrets
export OO_A261_RUN=/evidence/live/run
export OO_A261_WORK=/evidence/live/work
export OO_A261_READY=/ready/runtime.ready
if [ -e "$OO_A261_RUN" ]; then
  echo "run directory already exists" >&2
  record run-dir 2
fi
record run-dir 0

echo "=== live_chain ==="
set +e
CARGO_TARGET_DIR=/target/onboarding cargo test --manifest-path /onboarding/Cargo.toml --locked --offline --test live_chain -- --ignored --test-threads=1 \
  > /evidence/linux/chain/live.log 2>&1
live_exit=$?
set -e
record live "$live_exit"
if [ ! -f /evidence/live/work/oracle-ok.txt ]; then
  echo "oracle did not write its marker" >&2
  record oracle 1
fi
record oracle 0
printf '%s\n' 0 > /evidence/linux/chain/chain.exit
echo "chain=ok"
exit 0
