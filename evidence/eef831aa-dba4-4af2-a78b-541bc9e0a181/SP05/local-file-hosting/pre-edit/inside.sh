#!/bin/bash
# Pre-edit focused smoke of oo-control at the fixed base. No product edits.
set -u
echo "oo_task=sp05-local-file-hosting-pre-edit"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-control"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-control/src/lib.rs ] || [ -f crates/oo-control/src/file_hosting.rs ]; then
  echo "archive is not the pre-edit control crate" >&2
  exit 2
fi
mkdir -p /evidence/phases
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set -e
  echo "phase_exit=$clippy_add phase=rustup_component_add_clippy"
  printf '%s\n' "$clippy_add" > /evidence/phases/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /evidence/phases/clippy-component.exit
fi
echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /evidence/phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-control ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-control
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_oo_control"
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_control"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
echo "elapsed_smoke_seconds=$SECONDS"
worst=0
for phase_exit in "$fetch_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "smoke_worst_phase_exit=$worst"
printf '%s\n' "$worst" > /evidence/phases/worst.exit
exit "$worst"
