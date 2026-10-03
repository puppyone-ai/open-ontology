#!/bin/bash
# Working-tree snapshot_live. The mount is the repo, not the frozen archive.
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
echo "tree=working"
echo "=== cargo test --locked --offline -p oo-connectors --test snapshot_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test snapshot_live -- --test-threads=1 --nocapture
live_exit=$?
set -e
echo "live_exit=$live_exit"
printf '%s\n' "$live_exit" > /target/phases/live.exit
exit "$live_exit"
