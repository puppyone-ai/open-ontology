#!/bin/bash
# Fixed live run. Snapshot is the frozen baseline. Overlays are session.rs,
# sql.rs, model.rs, and live_pg.rs. Locked and offline. No fetch.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_PG_FIXTURE=/secrets/pointer.json
mkdir -p /phases /tmp
cd /src

record() {
  printf '%s\n' "$2" > "/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0
status=$?
set -e
record clippy-component "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

echo "=== green live_pg ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --test live_pg -- --nocapture
status=$?
set -e
record live "$status"

echo "=== connectors lib ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --lib
status=$?
set -e
record test-lib "$status"

echo "=== connectors doc ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --doc
status=$?
set -e
record test-doc "$status"

echo "=== connectors clippy ==="
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
status=$?
set -e
record clippy "$status"

worst=0
for name in live test-lib test-doc clippy; do
  value=$(cat "/phases/$name.exit")
  if [ "$value" -ne 0 ]; then
    worst=$value
  fi
done
printf '%s\n' "$worst" > /phases/final.exit
echo "phase_exit=$worst phase=final"
exit "$worst"
