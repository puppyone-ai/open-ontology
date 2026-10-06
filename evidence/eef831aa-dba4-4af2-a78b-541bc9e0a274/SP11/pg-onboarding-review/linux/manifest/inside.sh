#!/bin/bash
# Inventory only. No network. No PostgreSQL. No cargo.
set -u
echo "oo_task=sp11-pg-onboarding-review-manifest"
echo "phase=manifest"
export HOME=/tmp
rustc --edition 2024 /tools/sha256_manifest.rs -o /tmp/sha256_manifest
/tmp/sha256_manifest /evidence /out/SHA256SUMS
printf '%s\n' 0 > /evidence/linux/manifest/manifest.exit
echo "manifest=ok"
exit 0
