#!/bin/bash
# Build, test, and clippy release-control-api. No login shell.
set -eu
export PATH="/usr/local/cargo/bin:${PATH}"
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
cd /repo/tools/release-control-api
rustc --version
cargo --version
rustup show active-toolchain
if [ ! -f Cargo.lock ]; then
  cargo generate-lockfile
fi
awk '
  FNR==NR { if ($0 ~ /checksum = /) root[$0]=1; next }
  $0 ~ /checksum = / && !($0 in root) { print "lock_checksum_not_in_root " $0; bad=1 }
  END { exit bad }
' /repo/Cargo.lock Cargo.lock
cargo fetch --locked
mkdir -p /evidence/linux/build-1 /evidence/linux/proof
rustup component list --toolchain 1.94.0 > /evidence/linux/build-1/components-before.txt
rustup component add clippy --toolchain 1.94.0
export CARGO_NET_OFFLINE=true
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline -- --test-threads=1
echo BUILD_OK
