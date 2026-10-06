#!/bin/bash
# Retest after the live bin failed to compile. Offline. One CPU.
set -u
cd /work
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_NET_OFFLINE=true
export CARGO_BUILD_JOBS=1
{
  rustc -vV
  cargo -V
  rustup component list --installed || true
  ls "$(rustc --print sysroot)/bin"
} > /proof/toolchain.txt 2>&1
set +e
cargo test --offline --locked -- --test-threads=1 > /proof/test.stdout 2> /proof/test.stderr
printf '%s\n' "$?" > /proof/test.exit
if rustup component list --installed 2>/dev/null | grep -q '^clippy'; then
  cargo clippy --offline --locked --all-targets -- -D warnings > /proof/clippy.stdout 2> /proof/clippy.stderr
  printf '%s\n' "$?" > /proof/clippy.exit
else
  echo "clippy is not installed for toolchain 1.94.0; network none forbids rustup component add" > /proof/clippy.stderr
  printf '%s\n' 127 > /proof/clippy.exit
fi
mkdir -p /proof/live /var/pi-home/work
if [ "$(cat /proof/test.exit)" -eq 0 ]; then
  cargo run --offline --locked --bin pi-rpc-live -- \
    /opt/pi-prefix/install/node_modules/.bin/pi \
    /opt/pi-prefix/node/bin \
    /opt/probe/mock_openai \
    /proof/live \
    > /proof/live.stdout 2> /proof/live.stderr
  printf '%s\n' "$?" > /proof/live.exit
else
  echo "live skipped because tests did not compile or pass" > /proof/live.stderr
  printf '%s\n' 125 > /proof/live.exit
fi
echo "test=$(cat /proof/test.exit) clippy=$(cat /proof/clippy.exit) live=$(cat /proof/live.exit)"
rc=0
[ "$(cat /proof/test.exit)" -eq 0 ] || rc=1
[ "$(cat /proof/live.exit)" -eq 0 ] || rc=1
exit "$rc"
