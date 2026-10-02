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
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -m
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
worst=0
for phase_exit in "$fetch_exit" "$lib_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
