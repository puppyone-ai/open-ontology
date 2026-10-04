#!/bin/bash
# Composition probe. The archive is the fixed commit plus one ignored test file.
set -u
cd /src
mkdir -p /evidence/live /tmp/cargo-out
{
  echo "oo_task=full-import-review-probe"
  echo "oo_toolchain_request=1.94.0"
  uname -a
  cat /etc/os-release
  rustc --version --verbose
  cargo --version
  rustc --version
} > /evidence/toolchain.txt 2>&1
printf '%s\n' $? > /evidence/toolchain.exit
if ! grep -q '^rustc 1.94.0 ' /evidence/toolchain.txt; then
  echo "toolchain is not rustc 1.94.0" | tee -a /evidence/toolchain.txt
  exit 2
fi
if ! grep -q 'host: aarch64-unknown-linux-gnu' /evidence/toolchain.txt; then
  echo "host is not aarch64-unknown-linux-gnu" | tee -a /evidence/toolchain.txt
  exit 2
fi
set -e
test -f crates/oo-pipeline/tests/full_import_a159_review.rs
sha256sum \
  crates/oo-pipeline/Cargo.toml \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/full_import.rs \
  crates/oo-pipeline/src/copy_sink.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/src/target_installer.rs \
  crates/oo-pipeline/migrations/0000_bootstrap_roles.sql \
  crates/oo-pipeline/migrations/0001_target_fence_expand.sql \
  crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql \
  crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql \
  crates/oo-pipeline/migrations/0004_physical_mapping.sql \
  crates/oo-pipeline/migrations/0005_full_import_expand.sql \
  contracts/pipeline/full-import-v1.json \
  contracts/pipeline/full-import-v1.md \
  crates/oo-pipeline/tests/full_import_a159_review.rs \
  > /evidence/probe-sha256.txt
echo "=== cargo test composition oracle ==="
set +e
cargo test --locked --offline -p oo-pipeline --test full_import_a159_review -- --ignored --nocapture --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
printf '%s\n' $? > /evidence/test.exit
set -e
echo "phase_exit=$(cat /evidence/test.exit) phase=composition"
exit "$(cat /evidence/test.exit)"
