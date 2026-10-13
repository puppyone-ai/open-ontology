#!/bin/bash
# First compile on the untouched base archive. This script does not read the driver tree.
set -u
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/pre-edit /cargo-home /target
if [ ! -d /cargo-home/registry/src ]; then
  mkdir -p /cargo-home/registry /cargo-home/git
  cp -a /host-registry/. /cargo-home/registry/
  if [ -d /host-git ] && [ -n "$(ls -A /host-git 2>/dev/null || true)" ]; then
    cp -a /host-git/. /cargo-home/git/
  fi
fi
rustc --version | tee /evidence/pre-edit/rustc-version.txt
cargo --version | tee /evidence/pre-edit/cargo-version.txt
rustc -vV > /evidence/pre-edit/rustc-vv.txt
{
  echo "archive_mount=$(sha256sum /archive/Cargo.lock /archive/Cargo.toml /archive/crates/oo-sdk/Cargo.toml /archive/crates/oo-connectors/Cargo.toml)"
} > /evidence/pre-edit/archive-identity.sha256
cd /archive
set +e
cargo check -p oo-sdk -p oo-connectors --locked --offline --jobs 1 \
  > /evidence/pre-edit/cargo-check.stdout \
  2> /evidence/pre-edit/cargo-check.stderr
offline=$?
printf '%s\n' "$offline" > /evidence/pre-edit/cargo-check.exit
if [ "$offline" != "0" ]; then
  cargo check -p oo-sdk -p oo-connectors --locked --jobs 1 \
    > /evidence/pre-edit/cargo-check-online.stdout \
    2> /evidence/pre-edit/cargo-check-online.stderr
  online=$?
  printf '%s\n' "$online" > /evidence/pre-edit/cargo-check-online.exit
  exit "$online"
fi
exit 0
