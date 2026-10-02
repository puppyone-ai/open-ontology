#!/bin/bash
# New retention attempt. Does not modify /src. Writes stdout and stderr to /phases.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_PG_FIXTURE=/secrets/pointer.json
mkdir -p /phases /tmp

record() {
  printf '%s\n' "$2" > "/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

echo "=== fetch ==="
set +e
cargo fetch --manifest-path /src/Cargo.toml --locked > /phases/fetch-stdout.txt 2> /phases/fetch-stderr.txt
status=$?
set -e
record fetch "$status"
if [ "$status" -ne 0 ]; then
  record rerun "$status"
  exit "$status"
fi

rm -rf /tmp/work
# cap-drop ALL cannot preserve ownership. Copy bytes only.
cp -a --no-preserve=ownership /src /tmp/work
cp /opt/a097/a097_retention_rerun.rs /tmp/work/crates/oo-connectors/tests/a097_retention_rerun.rs
lock_src=$(sha256sum /src/Cargo.lock | awk '{print $1}')
lock_copy=$(sha256sum /tmp/work/Cargo.lock | awk '{print $1}')
printf '%s\n' "$lock_src" > /phases/lock-src.sha
printf '%s\n' "$lock_copy" > /phases/lock-copy-before.sha
if [ "$lock_src" != "$lock_copy" ]; then
  record lock 96
  record rerun 96
  exit 96
fi

echo "=== retention oracle ==="
set +e
cargo test --manifest-path /tmp/work/Cargo.toml --locked --offline -p oo-connectors --test a097_retention_rerun -- --test-threads=1 --nocapture > /phases/stdout.txt 2> /phases/stderr.txt
status=$?
set -e
record rerun "$status"
lock_after=$(sha256sum /tmp/work/Cargo.lock | awk '{print $1}')
printf '%s\n' "$lock_after" > /phases/lock-copy-after.sha
if [ "$lock_after" != "$lock_src" ]; then
  record lock 96
  exit 96
fi
record lock 0
exit "$status"
