#!/bin/bash
# Focused smoke of the immutable archive at /src. No fixture is attached.
# Writes go to /phases, /cargo, /target, and /tmp. Host HOME is not mounted.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
mkdir -p /phases /tmp
cd /src

record() {
  printf '%s\n' "$2" > "/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

{
  echo "uname=$(uname -a)"
  rustc -Vv
  cargo -V
  rustup show
  cat /etc/os-release
} > /phases/rustc.txt 2>&1
cat /phases/rustc.txt

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

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked
status=$?
set -e
record fetch "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

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

echo "=== connectors package test ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors
status=$?
set -e
record test "$status"

echo "=== connectors clippy ==="
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
status=$?
set -e
record clippy "$status"

worst=0
for name in test-lib test-doc test clippy; do
  value=$(cat "/phases/$name.exit")
  if [ "$value" -ne 0 ]; then
    worst=$value
  fi
done
printf '%s\n' "$worst" > /phases/final.exit
echo "phase_exit=$worst phase=final"
exit "$worst"
