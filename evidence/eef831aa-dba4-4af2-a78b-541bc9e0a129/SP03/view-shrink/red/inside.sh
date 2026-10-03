#!/bin/bash
# Unfixed driver. init_mut unit test is expected to panic. TCP peer is recorded as it exits.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /evidence
echo "=== identity ==="
uname -m
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
echo "=== cargo test sqlx-core init_mut view shrink ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p sqlx-core --lib init_mut_small_frames_shrink_the_receive_view -- --test-threads=1 --nocapture
unit_exit=$?
set -e
echo "unit_exit=$unit_exit"
printf '%s\n' "$unit_exit" > /evidence/unit.exit
echo "=== cargo test pg_wire_view_shrink ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test pg_wire_view_shrink -- --test-threads=1 --nocapture
tcp_exit=$?
set -e
echo "tcp_exit=$tcp_exit"
printf '%s\n' "$tcp_exit" > /evidence/tcp.exit
printf '%s\n' "$unit_exit" > /evidence/worst.exit
exit "$unit_exit"
