#!/bin/bash
# Pre-edit focused smoke. Existing oo-builder library tests only. No product edits.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
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
echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "PHASE test_exit=not_run"
  echo "PHASE clippy_exit=not_run"
  echo "worst_phase_exit=$fetch_exit"
  exit "$fetch_exit"
fi
echo "=== cargo test -p oo-builder --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib
test_exit=$?
set -e
echo "PHASE test_exit=$test_exit"
echo "=== cargo clippy -p oo-builder --lib ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --lib -- -D warnings
clippy_exit=$?
set -e
echo "PHASE clippy_exit=$clippy_exit"
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
exit "$worst"
