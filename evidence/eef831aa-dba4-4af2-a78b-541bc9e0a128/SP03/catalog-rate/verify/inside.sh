#!/bin/bash
# Post-fix lib, catalog_rate_live, unmodified snapshot_live, and clippy.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /target/phases /evidence
echo "=== identity ==="
uname -m
rustc -Vv
cargo -V
echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
lib_exit=$?
set -e
echo "lib_exit=$lib_exit"
printf '%s\n' "$lib_exit" > /target/phases/lib.exit
echo "=== cargo test catalog_rate_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test catalog_rate_live -- --test-threads=1 --nocapture
rate_exit=$?
set -e
echo "catalog_rate_exit=$rate_exit"
printf '%s\n' "$rate_exit" > /target/phases/catalog_rate.exit
echo "=== cargo test snapshot_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test snapshot_live -- --test-threads=1 --nocapture
snap_exit=$?
set -e
echo "snapshot_live_exit=$snap_exit"
printf '%s\n' "$snap_exit" > /target/phases/snapshot_live.exit
echo "=== clippy component ==="
set +e
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0
  add_exit=$?
else
  add_exit=0
fi
set -e
echo "clippy_add_exit=$add_exit"
printf '%s\n' "$add_exit" > /target/phases/clippy-add.exit
echo "=== cargo clippy --all-targets -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
worst=0
for phase_exit in "$lib_exit" "$rate_exit" "$snap_exit" "$add_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
