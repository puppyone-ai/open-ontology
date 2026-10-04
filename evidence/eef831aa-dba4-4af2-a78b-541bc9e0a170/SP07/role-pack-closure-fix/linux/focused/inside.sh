#!/bin/bash
# Post-edit Linux arm64 Rust 1.94 focused role_pack test and clippy.
# Snapshot is mounted read-only. No Python. No PostgreSQL. No host cargo.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true
echo "=== snapshot hashes ==="
sha256sum \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-builder/Cargo.toml crates/oo-core/Cargo.toml crates/oo-jobs/Cargo.toml \
  crates/oo-builder/src/lib.rs crates/oo-builder/src/role_pack.rs \
  crates/oo-builder/tests/role_pack.rs contracts/builder/role-pack-v1.md \
  > /evidence/snapshot-sha256.txt
echo "=== unsafe scan ==="
grep -n "unsafe" crates/oo-builder/src/role_pack.rs crates/oo-builder/src/lib.rs > /evidence/unsafe-lines.txt || true
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
clippy_component_exit=$?
set -e
echo "$clippy_component_exit" > /evidence/rustup-clippy.exit
echo "PHASE clippy_component_exit=$clippy_component_exit"
echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
echo "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
echo "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "not_run" > /evidence/test.exit
  echo "not_run" > /evidence/clippy.exit
  echo "worst_phase_exit=$fetch_exit"
  exit "$fetch_exit"
fi
echo "=== cargo test role_pack ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
echo "=== cargo clippy oo-builder all targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
{
  echo "rustc_vv_exit=0"
  rustc -vV
  echo "cargo_v_exit=0"
  cargo -V
} > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "$worst" > /evidence/worst.exit
echo "worst_phase_exit=$worst"
exit "$worst"
