#!/bin/bash
# Rust SHA-256 inventory. No cargo. No network.
set -u
echo "oo_task=sp11-pg-onboarding-outcome-repair-manifest"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a268"
echo "phase=manifest"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
rustc --version | tee /out/rustc-version.txt
case "$(rustc --version)" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain" >&2; exit 2 ;;
esac
rustc --edition 2024 /tool/sha256_manifest.rs -o /target/sha256_manifest_a268 > /out/rustc.log 2>&1
mkdir -p /evidence/source
/target/sha256_manifest_a268 /onboarding /out/tool-sha256.txt | tee /out/tool-walk.txt
cp /out/tool-sha256.txt /evidence/source/tool-sha256.txt
/target/sha256_manifest_a268 /evidence /out/SHA256SUMS | tee /out/evidence-walk.txt
echo manifest=ok
