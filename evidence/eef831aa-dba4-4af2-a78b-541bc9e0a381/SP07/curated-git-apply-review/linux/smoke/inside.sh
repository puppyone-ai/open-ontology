#!/bin/bash
# Toolchain identity only. No cargo fetch, no compile, no product mount.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
{
  echo "=== identity ==="
  rustc -vV
  cargo -V
  git --version
  uname -a
  id
} > /evidence/identity.txt 2>&1
rustc -vV > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
git --version > /evidence/git-version.txt
uname -m > /evidence/uname.txt
if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 4 > /evidence/rustc.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/rustc.exit
grep -q '1.94.0' /evidence/cargo-version.txt
grep -q 'aarch64' /evidence/uname.txt
echo "PHASE rustc_exit=0"
