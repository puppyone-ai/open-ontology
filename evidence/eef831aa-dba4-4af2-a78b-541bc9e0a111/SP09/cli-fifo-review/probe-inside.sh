#!/bin/bash
# Offline independent probe. No fetch and no Clippy. Not an air-gap claim.
set -u
export HOME=/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export A111_REPORT=/out/probe-report.json
export A111_CASES=/out/cases
mkdir -p /out/phases /out/cases
cd /src
{
  echo "argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli --test a111_probe -- --test-threads=1 --nocapture"
  echo "network=none"
  echo "offline=yes"
  echo "fetch=not_run"
  echo "clippy=not_run"
  echo "airgap_claim=no"
} > /out/phases/argv.txt

rustc -vV > /out/phases/rustc.txt 2>&1
printf '%s\n' "$?" > /out/phases/rustc.exit
cargo -V > /out/phases/cargo.txt 2>&1
printf '%s\n' "$?" > /out/phases/cargo.exit
if [ -f /target/debug/oo ]; then
  sha256sum /target/debug/oo > /out/phases/oo-before.sha256
fi

cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli --test a111_probe -- --test-threads=1 --nocapture > /out/phases/probe.txt 2>&1
probe_ec=$?
printf '%s\n' "$probe_ec" > /out/phases/probe.exit
if [ -f /target/debug/oo ]; then
  sha256sum /target/debug/oo > /out/phases/oo-after.sha256
fi
sha256sum \
  crates/oo-cli/src/load.rs \
  crates/oo-cli/tests/common/mod.rs \
  crates/oo-cli/tests/query.rs \
  contracts/cli/query/README.md \
  Cargo.lock \
  crates/oo-cli/Cargo.toml \
  > /out/phases/product-sha256.txt
printf 'elapsed_seconds=%s\n' "$SECONDS" >> /out/phases/argv.txt
exit "$probe_ec"
