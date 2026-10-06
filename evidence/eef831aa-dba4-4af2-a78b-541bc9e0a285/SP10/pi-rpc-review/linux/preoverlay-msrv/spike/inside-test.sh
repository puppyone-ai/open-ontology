#!/bin/bash
# Runs inside the already started Linux arm64 Rust container. Offline. One job.
set -u
cd /work
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_NET_OFFLINE=true
export CARGO_BUILD_JOBS=1
{
  echo "rustc=$(rustc -vV | head -n 1)"
  cargo -V
  cargo clippy -V
} > /proof/toolchain.txt 2>&1
cargo generate-lockfile --offline > /proof/lock.stdout 2> /proof/lock.stderr
printf '%s\n' "$?" > /proof/lock.exit
if [ "$(cat /proof/lock.exit)" -ne 0 ]; then
  echo "lock failed"
  exit 1
fi
set +e
cargo test --offline --locked -- --test-threads=1 > /proof/test.stdout 2> /proof/test.stderr
printf '%s\n' "$?" > /proof/test.exit
cargo clippy --offline --locked --all-targets -- -D warnings > /proof/clippy.stdout 2> /proof/clippy.stderr
printf '%s\n' "$?" > /proof/clippy.exit
mkdir -p /proof/live /var/pi-home/work
cargo run --offline --locked --bin pi-rpc-live -- \
  /opt/pi-prefix/install/node_modules/.bin/pi \
  /opt/pi-prefix/node/bin \
  /opt/probe/mock_openai \
  /proof/live \
  > /proof/live.stdout 2> /proof/live.stderr
printf '%s\n' "$?" > /proof/live.exit
echo "test=$(cat /proof/test.exit) clippy=$(cat /proof/clippy.exit) live=$(cat /proof/live.exit)"
rc=0
[ "$(cat /proof/test.exit)" -eq 0 ] || rc=1
[ "$(cat /proof/clippy.exit)" -eq 0 ] || rc=1
[ "$(cat /proof/live.exit)" -eq 0 ] || rc=1
exit "$rc"
