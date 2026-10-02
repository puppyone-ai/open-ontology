#!/bin/bash
# Tiny pre-edit smoke of the archived baseline. linux/arm64, Rust 1.94, oo-core check only.
# Not a matrix, not clippy, not an all-module test, and not an air-gap claim.
set -u
echo "oo_task=isc-attribution-pre-smoke"
echo "oo_source=6fa6481267e5a4efac20831fa0b407a8cf028905"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "package=oo-core"
echo "package_reason=documentation-only correction; tiny check of the foundation crate"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "check_argv=cargo check --manifest-path Cargo.toml --locked --offline -p oo-core"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
echo "CARGO_HOME=$CARGO_HOME"
echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
rustc -Vv
cargo -V
rustup show
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
cd /src
if [ ! -f Cargo.lock ] || [ ! -f Cargo.toml ] || [ ! -f crates/oo-core/Cargo.toml ]; then
  echo "snapshot missing cargo inputs" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/snapshot.exit
echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== cargo check --manifest-path Cargo.toml --locked --offline -p oo-core ==="
set +e
cargo check --manifest-path Cargo.toml --locked --offline -p oo-core
check_exit=$?
set -e
echo "phase_exit=$check_exit phase=cargo_check_oo_core"
printf '%s\n' "$check_exit" > /evidence/check.exit
echo "elapsed_seconds=$SECONDS"
exit "$check_exit"
