#!/bin/bash
# Focused Linux arm64 Rust 1.94 smoke of the frozen archive. No fixture. No probe.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /target/phases
echo "=== identity ==="
uname -m
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
echo "=== clippy component ==="
set +e
rustup component list --toolchain 1.94.0 --installed
installed_exit=$?
set -e
echo "component_list_exit=$installed_exit"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0
  add_exit=$?
  set -e
  echo "clippy_add_exit=$add_exit"
  printf '%s\n' "$add_exit" > /target/phases/clippy-add.exit
else
  echo "clippy_add_exit=already_installed"
  printf '%s\n' "0" > /target/phases/clippy-add.exit
  add_exit=0
fi
echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "fetch_exit=$fetch_exit"
printf '%s\n' "$fetch_exit" > /target/phases/fetch.exit
echo "=== cargo test --locked --offline -p oo-connectors --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
lib_exit=$?
set -e
echo "lib_exit=$lib_exit"
printf '%s\n' "$lib_exit" > /target/phases/lib.exit
echo "package_live_pg=not-run"
echo "package_lossless_live=not-run"
echo "snapshot_live=not-run"
echo "live_fixture_reason=OO_PG_FIXTURE and OO_PG_SNAPSHOT_FIXTURE are absent; missing fixtures are not green"
printf '%s\n' "not-run" > /target/phases/live-fixture.status
echo "=== cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
worst=0
for phase_exit in "$add_exit" "$fetch_exit" "$lib_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
