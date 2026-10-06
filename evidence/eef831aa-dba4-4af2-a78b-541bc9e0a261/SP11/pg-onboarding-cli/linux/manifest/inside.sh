#!/bin/bash
# Std-only inventory. No cargo. No network use inside the compile.
set -u
echo "oo_task=sp11-pg-onboarding-cli-manifest"
rustc --edition 2021 -o /usr/local/bin/sha256_manifest /tool/sha256_manifest.rs
sha256_manifest /onboarding /out/tool.txt tools/pg-onboarding
sha256_manifest /evidence /out/evidence.txt evidence/eef831aa-dba4-4af2-a78b-541bc9e0a261/SP11/pg-onboarding-cli
sort -k2 /out/tool.txt /out/evidence.txt > /out/SHA256SUMS
wc -l /out/SHA256SUMS
echo manifest=ok
