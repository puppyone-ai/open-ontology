#!/bin/bash
# Focused role-pack library test and scoped clippy. Records every phase exit.
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
echo "=== overlaid sources ==="
sha256sum crates/oo-builder/src/lib.rs crates/oo-builder/src/role_pack.rs crates/oo-builder/tests/role_pack.rs
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_component_exit=$?
set -e
echo "PHASE clippy_component_exit=$clippy_component_exit"
echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline
offline_fetch_exit=$?
set -e
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch --locked online fallback ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked
  fetch_exit=$?
  set -e
fi
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "PHASE test_exit=not_run"
  echo "PHASE clippy_lib_exit=not_run"
  echo "PHASE clippy_test_exit=not_run"
  echo "worst_phase_exit=$fetch_exit"
  exit "$fetch_exit"
fi
echo "=== cargo test -p oo-builder --test role_pack ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack
test_exit=$?
set -e
echo "PHASE test_exit=$test_exit"
echo "=== cargo clippy -p oo-builder --lib ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --lib -- -D warnings
clippy_lib_exit=$?
set -e
echo "PHASE clippy_lib_exit=$clippy_lib_exit"
echo "=== cargo clippy -p oo-builder --test role_pack ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack -- -D warnings
clippy_test_exit=$?
set -e
echo "PHASE clippy_test_exit=$clippy_test_exit"
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$clippy_lib_exit" "$clippy_test_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
exit "$worst"
