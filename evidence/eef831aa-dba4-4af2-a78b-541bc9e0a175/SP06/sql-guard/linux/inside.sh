#!/bin/bash
# Pre-edit oo-gateway lib smoke on Linux arm64 Rust 1.94.
# No source edits. No cargo update. The lock stays --locked.
set -u
echo "oo_task=sp06-sql-guard-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a175"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "phase=preedit"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- -D warnings"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
echo "CARGO_HOME=$CARGO_HOME"
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-gateway/Cargo.toml ]; then
  echo "snapshot missing cargo inputs" >&2
  exit 2
fi
if [ -f crates/oo-gateway/src/sql_guard.rs ]; then
  echo "preedit saw sql_guard.rs; refusing" >&2
  exit 2
fi
mkdir -p /evidence/linux/phases
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set -e
  echo "phase_exit=$clippy_add phase=rustup_component_add_clippy"
  printf '%s\n' "$clippy_add" > /evidence/linux/phases/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /evidence/linux/phases/clippy-component.exit
fi
echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /evidence/linux/phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== cargo test -p oo-gateway --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_oo_gateway_lib"
printf '%s\n' "$test_exit" > /evidence/linux/phases/test.exit
echo "=== cargo clippy -p oo-gateway --lib ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_gateway_lib"
printf '%s\n' "$clippy_exit" > /evidence/linux/phases/clippy.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
