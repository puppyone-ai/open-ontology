#!/bin/bash
# Focused Linux arm64 Rust 1.94 smoke of the frozen exporter archive.
# No PostgreSQL. No cargo update. No producer.
set -u
echo "oo_task=sp05-query-result-export-decimal-actual-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a333"
echo "oo_exporter=62b4afecdd377fa94310f46467c48a01e61d14db"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/export
export CARGO_TERM_COLOR=never
export CARGO_NET_OFFLINE=true
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:${PATH}"
mkdir -p /evidence/actual-pg-chain/linux/smoke/phases /cargo-home/bin
printf '%s\n' "not-invoked" > /evidence/actual-pg-chain/linux/smoke/cargo-update-invoked.txt

record() {
  mkdir -p /evidence/actual-pg-chain/linux/smoke/phases
  printf '%s\n' "$2" > "/evidence/actual-pg-chain/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/actual-pg-chain/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/actual-pg-chain/linux/smoke/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  printf '%s\n' 0 > /evidence/actual-pg-chain/linux/smoke/phases/sha256-tool.exit
fi
/cargo-home/bin/sha256 file /src/Cargo.lock /src/Cargo.toml > /evidence/actual-pg-chain/linux/smoke/exporter-lock.txt
record exporter-hash $?
if ! grep -q 'cc614d3de29ee73fb29ed7bfa9761102f9a3fe26d27244e8e76000016a791d22' /evidence/actual-pg-chain/linux/smoke/exporter-lock.txt; then
  echo "exporter Cargo.lock is not the frozen commit" >&2
  record exporter-stable 1
fi
if ! grep -q 'b17cb555e0f20e5fcd7853c584e2dd0a1c0414140e5cad533a3e6f885e7cd1f4' /evidence/actual-pg-chain/linux/smoke/exporter-lock.txt; then
  echo "exporter Cargo.toml is not the frozen commit" >&2
  record exporter-stable 1
fi
record exporter-stable 0

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/actual-pg-chain/linux/smoke/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/actual-pg-chain/linux/smoke/clippy-component.log
  record clippy-component 0
fi

cd /src
cargo test --locked --offline -- --test-threads=1 > /evidence/actual-pg-chain/linux/smoke/test.log 2>&1
record test $?
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/actual-pg-chain/linux/smoke/clippy.log 2>&1
record clippy $?
printf '%s\n' 0 > /evidence/actual-pg-chain/linux/smoke/smoke.exit
echo "smoke_done"
