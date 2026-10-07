#!/bin/bash
# Build the package binary only. Do not rerun tests or touch Postgres.
set -eu
export PATH="/usr/local/cargo/bin:${PATH}"
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export CARGO_NET_OFFLINE=true
cd /repo/tools/release-coordinator
rustc --version
awk '
  FNR==NR { if ($0 ~ /checksum = /) root[$0]=1; next }
  $0 ~ /checksum = / && !($0 in root) { print "lock_checksum_not_in_root " $0; bad=1 }
  END { exit bad }
' /repo/Cargo.lock Cargo.lock
cargo build --locked --offline --bin release-coordinator
test -x /target/debug/release-coordinator
echo BIN_OK
