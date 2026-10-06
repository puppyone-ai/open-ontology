#!/bin/bash
# Fresh pi-rpc-live against the pinned Pi binary. Network none. No clippy stand-in.
# The review oracle then checks the raw transcript. It does not rewrite historical evidence.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_NET_OFFLINE=true
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
cd /work
{
  rustc -vV
  cargo -V
} > /proof/toolchain.txt
mkdir -p /proof/live /var/pi-home/work
/opt/pi-prefix/install/node_modules/.bin/pi --version > /proof/live/pi-version-pre.stdout 2> /proof/live/pi-version-pre.stderr
printf '%s\n' "$?" > /proof/live/pi-version-pre.exit
echo "PHASE pi_version_pre_exit=$(cat /proof/live/pi-version-pre.exit)"
set +e
cargo run --offline --locked --bin pi-rpc-live -- \
  /opt/pi-prefix/install/node_modules/.bin/pi \
  /opt/pi-prefix/node/bin \
  /opt/probe/mock_openai \
  /proof/live \
  > /proof/live.stdout 2> /proof/live.stderr
live_exit=$?
set -e
printf '%s\n' "$live_exit" > /proof/live.exit
echo "PHASE live_exit=$live_exit"
set +e
rustc --edition 2024 -D warnings /oracle/review_oracle.rs -o /target/review_oracle > /proof/oracle-rustc.stdout 2> /proof/oracle-rustc.stderr
oracle_build=$?
printf '%s\n' "$oracle_build" > /proof/oracle-rustc.exit
oracle_exit=2
if [ "$oracle_build" -eq 0 ]; then
  /target/review_oracle live /proof /work > /proof/live-oracle.stdout 2> /proof/live-oracle.stderr
  oracle_exit=$?
fi
set -e
printf '%s\n' "$oracle_exit" > /proof/live-oracle.exit
echo "PHASE oracle_build=$oracle_build live_oracle=$oracle_exit"
if [ "$live_exit" -ne 0 ]; then
  exit "$live_exit"
fi
exit "$oracle_exit"
