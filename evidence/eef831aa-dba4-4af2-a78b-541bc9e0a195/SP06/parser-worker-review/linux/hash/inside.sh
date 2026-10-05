#!/bin/bash
# Refresh SHA256SUMS from the Rust tool. Self-test runs inside the binary.
set -u
export PATH="/cargo-home/bin:${PATH}"
if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
fi
exec /cargo-home/bin/sha256 dir /evidence/SHA256SUMS /evidence
