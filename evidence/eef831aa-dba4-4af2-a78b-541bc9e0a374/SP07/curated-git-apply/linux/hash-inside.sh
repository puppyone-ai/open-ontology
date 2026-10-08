#!/bin/bash
# Run inside the pinned Rust image. Argument: source or final.
set -euo pipefail
mode=${1:?mode}
cd /evidence
if [ -e /src/.git ] || [ -e /src/tools/curated-git-apply/.git ]; then
  echo "source extract contains git metadata" >&2
  exit 1
fi
rustc --edition 2021 -C opt-level=2 -o /tmp/hash-tree /evidence/linux/hash-oracle/main.rs
if [ "$mode" = source ]; then
  rustc --version > linux/hash/rustc-version.txt
  git --version > linux/hash/git-version.txt
  uname -m > linux/hash/uname.txt
  printf '%s\n' 0 > linux/hash/rustc.exit
  /tmp/hash-tree /src /src/SHA256SUMS > /tmp/source-manifest.stdout
  printf '%s\n' 0 > linux/hash/source-manifest.exit
  cp /tmp/source-manifest.stdout linux/hash/source-manifest.stdout
  cp /src/SHA256SUMS source/SHA256SUMS
  cp /src/SHA256SUMS.sha256 source/SHA256SUMS.sha256
  /tmp/hash-tree /evidence /evidence/SHA256SUMS > /tmp/evidence-manifest.stdout
  cp /tmp/evidence-manifest.stdout linux/hash/evidence-manifest.stdout
  exit 0
fi
if [ "$mode" != final ]; then
  echo "unknown mode" >&2
  exit 2
fi
/tmp/hash-tree /evidence /evidence/SHA256SUMS > /tmp/evidence-final.stdout
cp /evidence/SHA256SUMS /tmp/evidence-final.sums
cp /evidence/SHA256SUMS.sha256 /tmp/evidence-final.side
/tmp/hash-tree /evidence /tmp/evidence-check.sums > /tmp/evidence-check.stdout
cmp /tmp/evidence-final.sums /tmp/evidence-check.sums
cmp /tmp/evidence-final.side /evidence/SHA256SUMS.sha256
cmp /tmp/evidence-final.sums /evidence/SHA256SUMS
