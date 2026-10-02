#!/bin/bash
# RED live test. Production session code is the frozen baseline.
# The overlaid live_pg.rs is the new GUC assertion. It must fail first.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_PG_FIXTURE=/secrets/pointer.json
mkdir -p /phases /tmp
cd /src
echo "=== red live_pg ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --test live_pg -- --nocapture
status=$?
set -e
printf '%s\n' "$status" > /phases/red-live.exit
echo "phase_exit=$status phase=red-live"
exit "$status"
