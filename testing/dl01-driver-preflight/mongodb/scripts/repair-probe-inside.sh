#!/bin/bash
# Probe-only Linux Rust 1.94 test, clippy, and build. No lockfile rewrite.
# Does not compile mongodb-preview, pg-builder, or oo-pipeline.
set -euo pipefail

cd /src
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.txt 2>&1
fi

echo "probe-fetch" | tee /evidence/phase.txt
if ! cargo metadata --locked --offline --format-version 1 >/evidence/probe-metadata.txt 2>/evidence/probe-metadata.err; then
  cargo fetch --locked > /evidence/probe-fetch.txt 2>&1
  printf 'fetched\n' > /evidence/probe-fetch.result
else
  printf 'cached\n' > /evidence/probe-fetch.result
fi
echo "probe-test" | tee -a /evidence/phase.txt
cargo test --locked --offline --jobs 1 --workspace -- --test-threads=1 > /evidence/probe-test.txt 2>&1
echo "probe-clippy" | tee -a /evidence/phase.txt
cargo clippy --locked --offline --jobs 1 --all-targets --workspace -- -D warnings > /evidence/probe-clippy.txt 2>&1
echo "probe-build" | tee -a /evidence/phase.txt
cargo build --locked --offline --jobs 1 --bins > /evidence/probe-build.txt 2>&1

mkdir -p /evidence/bin
cp /target/debug/oo-mongodb-driver-probe /evidence/bin/oo-mongodb-driver-probe
cp /target/debug/oo-mongodb-arrow-decode /evidence/bin/oo-mongodb-arrow-decode
chmod 755 /evidence/bin/oo-mongodb-driver-probe /evidence/bin/oo-mongodb-arrow-decode
if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence || true
fi
printf 'probe-build-ok\n' | tee /evidence/build-result.txt
