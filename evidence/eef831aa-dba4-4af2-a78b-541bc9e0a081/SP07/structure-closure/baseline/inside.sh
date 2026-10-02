#!/bin/bash
# Pre-edit focused oo-builder check. linux/arm64, Rust 1.94. Not a matrix.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
cd /src
echo "oo_task=structure-closure-baseline"
echo "oo_source=mounted-git-archive"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "=== rustc -vV ==="
rustc -vV
echo "rustc_vv_exit=$?"
echo "=== cargo -V ==="
cargo -V
echo "cargo_v_exit=$?"
echo "=== rustup show ==="
rustup show
echo "=== uname ==="
uname -a
cat /etc/os-release
echo "=== id ==="
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_component_exit=$?
set -e
echo "PHASE clippy_component_exit=$clippy_component_exit"
printf '%s\n' "$clippy_component_exit" > /evidence/clippy-component.exit
echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "PHASE fetch_exit=$fetch_exit"
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  echo "PHASE test_exit=not_run"
  echo "PHASE clippy_exit=not_run"
  exit "$fetch_exit"
fi
echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder
test_exit=$?
set -e
echo "PHASE test_exit=$test_exit"
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "PHASE clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
exit "$worst"
