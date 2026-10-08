#!/bin/bash
# Version smoke only. No compile, no cargo fetch, no source edit.
set -u
export PATH=/usr/local/cargo/bin:/usr/bin:/bin
export RUSTUP_TOOLCHAIN=1.94.0
RAW=/evidence/linux/raw
mkdir -p "$RAW"
{
  echo "uname=$(uname -m)"
  rustc --version
  rustc --version --verbose
  cargo --version
  openssl version
} > "$RAW/smoke.log" 2>&1
smoke=0
grep -q 'aarch64' "$RAW/smoke.log" || smoke=1
grep -q 'rustc 1.94.0' "$RAW/smoke.log" || smoke=1
printf '%s\n' "$smoke" > "$RAW/smoke.exit"
exit "$smoke"
