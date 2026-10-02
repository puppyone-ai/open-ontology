#!/bin/bash
# Focused Linux arm64 Rust 1.94 verification of the type decoder.
# /src is a frozen snapshot. Package test exit 101 is the missing OO_PG_FIXTURE gap.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
mkdir -p /phases /tmp /cargo/probe-home
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

echo "=== cargo fetch ==="
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked
status=$?
set -e
if [ "$status" -ne 0 ]; then
  echo "locked fetch failed; resolving the new Arrow edges without upgrading pinned crates"
  set +e
  cargo fetch --manifest-path /src/Cargo.toml
  status=$?
  set -e
fi
record fetch "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi
cp /src/Cargo.lock /phases/Cargo.lock
sha256sum /src/Cargo.lock > /phases/Cargo.lock.sha256

echo "=== connectors lib ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --lib -- --test-threads=8
status=$?
set -e
record test-lib "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

echo "=== connectors doc ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --doc
status=$?
set -e
record test-doc "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

echo "=== connectors package test (OO_PG_FIXTURE absent) ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors -- --test-threads=8
status=$?
set -e
record test "$status"

echo "=== connectors clippy ==="
set +e
cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
status=$?
set -e
record clippy "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

echo "=== export arrow fixture ==="
set +e
OO_ARROW_FIXTURE_OUT=/phases/arrow-fixture.json \
  cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --lib types::tests::export_arrow_fixture -- --exact
status=$?
set -e
if [ "$status" -eq 0 ] && [ ! -s /phases/arrow-fixture.json ]; then
  echo "arrow fixture was not written" >&2
  status=2
fi
record arrow-export "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

echo "=== arrow probe ==="
rm -rf /tmp/arrow-probe /tmp/probe-target
cp -a /probe-src /tmp/arrow-probe
export CARGO_HOME=/cargo/probe-home
export CARGO_TARGET_DIR=/tmp/probe-target
set +e
cargo generate-lockfile --manifest-path /tmp/arrow-probe/Cargo.toml
status=$?
set -e
record arrow-lock "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi
cp /tmp/arrow-probe/Cargo.lock /phases/arrow-probe-Cargo.lock
set +e
cargo fetch --manifest-path /tmp/arrow-probe/Cargo.toml --locked
status=$?
set -e
record arrow-fetch "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi
set +e
OO_ARROW_FIXTURE=/phases/arrow-fixture.json \
  cargo test --manifest-path /tmp/arrow-probe/Cargo.toml --locked --offline
status=$?
set -e
record arrow-test "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi
set +e
cargo clippy --manifest-path /tmp/arrow-probe/Cargo.toml --locked --offline --all-targets -- -D warnings
status=$?
set -e
record arrow-clippy "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target

echo "=== live lossless types ==="
set +e
OO_PG_TYPES_FIXTURE=/fixture/pointer.json \
  cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --test lossless_live -- --ignored --test-threads=1 \
  > /phases/test-live.log 2>&1
status=$?
set -e
cat /phases/test-live.log
if [ "$status" -eq 0 ] && ! grep -q "test live_lossless_types ... ok" /phases/test-live.log; then
  echo "deciding live test did not run" >&2
  status=2
fi
record test-live "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

echo "=== feature graph ==="
set +e
{
  echo "=== normal ==="
  cargo tree -p oo-connectors --locked --offline --edges normal
  echo "=== features ==="
  cargo tree -p oo-connectors --locked --offline --edges features
} > /phases/feature-graph-after.txt 2> /phases/feature-graph-after.err
status=$?
set -e
record feature-graph "$status"
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /phases/final.exit
  exit "$status"
fi

printf '%s\n' 0 > /phases/final.exit
echo "phase_exit=0 phase=final"
exit 0
