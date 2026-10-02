#!/bin/bash
# Recompile only the independent oracle. The author live_pg result is already recorded.
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

rm -rf /tmp/work
cp -a /src /tmp/work
cp /opt/a097/a097_role_guc.rs /tmp/work/crates/oo-connectors/tests/a097_role_guc.rs
lock_src=$(sha256sum /src/Cargo.lock | awk '{print $1}')
lock_copy=$(sha256sum /tmp/work/Cargo.lock | awk '{print $1}')
if [ "$lock_src" != "$lock_copy" ]; then
  record oracle 96
  exit 96
fi
rm -f /phases/oracle-notes.txt /phases/probe-stderr.txt

set +e
cargo test --manifest-path /tmp/work/Cargo.toml --locked --offline -p oo-connectors --test a097_role_guc -- --test-threads=1
status=$?
set -e
record oracle "$status"
lock_after=$(sha256sum /tmp/work/Cargo.lock | awk '{print $1}')
printf '%s\n' "$lock_after" > /phases/lock-copy-after.sha
if [ "$lock_after" != "$lock_src" ]; then
  record lock 96
  exit 96
fi
record lock 0
exit "$status"
