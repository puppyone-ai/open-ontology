#!/bin/bash
# Live author test and independent oracle against the immutable /src archive.
# The oracle is compiled from a writable copy. /src and its lock stay unchanged.
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

sha256() {
  sha256sum "$1" | awk '{print $1}'
}

echo "=== copy archive for the oracle ==="
rm -rf /tmp/work
cp -a /src /tmp/work
cp /opt/a097/a097_role_guc.rs /tmp/work/crates/oo-connectors/tests/a097_role_guc.rs
lock_src=$(sha256 /src/Cargo.lock)
lock_copy=$(sha256 /tmp/work/Cargo.lock)
printf '%s\n' "$lock_src" > /phases/lock-src.sha
printf '%s\n' "$lock_copy" > /phases/lock-copy-before.sha
if [ "$lock_src" != "$lock_copy" ]; then
  record lock 96
  record final 96
  exit 96
fi

echo "=== author live_pg ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline -p oo-connectors --test live_pg -- --test-threads=1
status=$?
set -e
record live "$status"

echo "=== independent oracle ==="
set +e
cargo test --manifest-path /tmp/work/Cargo.toml --locked --offline -p oo-connectors --test a097_role_guc -- --test-threads=1
status=$?
set -e
record oracle "$status"

lock_after=$(sha256 /tmp/work/Cargo.lock)
printf '%s\n' "$lock_after" > /phases/lock-copy-after.sha
if [ "$lock_after" != "$lock_src" ]; then
  record lock 96
else
  record lock 0
fi

echo "=== sqlx-core feature tree ==="
set +e
cargo tree --manifest-path /src/Cargo.toml -e features -i sqlx-core -p oo-connectors --locked --offline > /phases/cargo-tree.txt
status=$?
set -e
record tree "$status"

worst=0
for name in live oracle lock tree; do
  value=$(cat "/phases/$name.exit")
  if [ "$value" -ne 0 ]; then
    worst=$value
  fi
done
record final "$worst"
exit "$worst"
