#!/bin/bash
# Overlay the frozen archive with owned files, then run the focused R1 checks.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
mkdir -p /target/phases /src
find /src -mindepth 1 -delete
cp -a /frozen/. /src/
cp -a /patch/. /src/
cd /src
test -f crates/oo-connectors/src/snapshot.rs
echo "=== identity ==="
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -m
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_component_exit=$?
set -e
echo "clippy_component_exit=$clippy_component_exit"
printf '%s\n' "$clippy_component_exit" > /target/phases/clippy-component.exit
echo "=== cargo test --locked --offline -p oo-connectors --lib --test snapshot_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib --test snapshot_live -- --test-threads=1
test_exit=$?
set -e
echo "test_exit=$test_exit"
printf '%s\n' "$test_exit" > /target/phases/test.exit
echo "=== cargo clippy --locked --offline -p oo-connectors --all-targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
echo "package_live_pg=not-run"
echo "package_lossless_live=not-run"
printf '%s\n' "not-run" > /target/phases/package-live.status
worst=0
for phase_exit in "$clippy_component_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
