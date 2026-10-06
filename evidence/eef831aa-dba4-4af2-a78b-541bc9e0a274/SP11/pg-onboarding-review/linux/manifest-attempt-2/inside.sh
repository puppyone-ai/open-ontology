#!/bin/bash
# Inventory only. No network. No PostgreSQL. No cargo.
# The binary is written on the /out bind mount. The tmpfs at /tmp is not executable.
set -euo pipefail
echo "oo_task=sp11-pg-onboarding-review-manifest-2"
echo "phase=manifest"
export HOME=/tmp
export RUSTUP_TOOLCHAIN=1.94.0
rustc --edition 2024 /tools/sha256_manifest.rs -o /out/sha256_manifest
/out/sha256_manifest /evidence /out/SHA256SUMS
printf '%s\n' 0 > /evidence/linux/manifest-attempt-2/manifest.exit
echo "manifest=ok"
