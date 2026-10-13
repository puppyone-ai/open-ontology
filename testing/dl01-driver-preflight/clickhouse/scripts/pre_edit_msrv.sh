#!/bin/bash
# MSRV proof for the untouched base archive. rust-toolchain.toml in that
# archive names 1.98.1; this script forces the image toolchain 1.94.0.
set -u
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/msrv194
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
mkdir -p /evidence/pre-edit /cargo-home /target/msrv194
cd /archive
rustup run 1.94.0 rustc -vV > /evidence/pre-edit/msrv-rustc-vv.txt
rustup show > /evidence/pre-edit/msrv-rustup-show.txt 2>&1 || true
if ! grep -q '^release: 1.94.0$' /evidence/pre-edit/msrv-rustc-vv.txt; then
  printf '91\n' > /evidence/pre-edit/msrv-cargo-check.exit
  exit 91
fi
set +e
rustup run 1.94.0 cargo check -p oo-sdk -p oo-connectors --locked --offline --jobs 1 \
  > /evidence/pre-edit/msrv-cargo-check.stdout \
  2> /evidence/pre-edit/msrv-cargo-check.stderr
offline=$?
printf '%s\n' "$offline" > /evidence/pre-edit/msrv-cargo-check.exit
if [ "$offline" != "0" ]; then
  rustup run 1.94.0 cargo check -p oo-sdk -p oo-connectors --locked --jobs 1 \
    > /evidence/pre-edit/msrv-cargo-check-online.stdout \
    2> /evidence/pre-edit/msrv-cargo-check-online.stderr
  online=$?
  printf '%s\n' "$online" > /evidence/pre-edit/msrv-cargo-check-online.exit
  exit "$online"
fi
exit 0
