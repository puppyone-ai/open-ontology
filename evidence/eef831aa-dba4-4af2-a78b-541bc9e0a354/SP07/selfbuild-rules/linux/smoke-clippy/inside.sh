#!/bin/bash
# Install the 1.94 clippy component before product tests. Preserves the first smoke.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/smoke-clippy/inside.sh
# Change: evidence path only. The first smoke stays at linux/smoke with clippy absent and exit 1.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export HOME=/tmp
export CARGO_TERM_COLOR=never
mkdir -p /evidence/linux/smoke-clippy/phases /cargo-home
rustc --version | tee /evidence/linux/smoke-clippy/rustc-version.txt
if rustup component list --installed | grep -q '^clippy-'; then
  printf '%s\n' already > /evidence/linux/smoke-clippy/clippy-component.txt
  printf '%s\n' 0 > /evidence/linux/smoke-clippy/phases/clippy-component.exit
  exit 0
fi
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/smoke-clippy/clippy-component.log 2>&1
status=$?
printf '%s\n' "$status" > /evidence/linux/smoke-clippy/phases/clippy-component.exit
if [ "$status" -ne 0 ]; then
  exit "$status"
fi
rustup component list --installed | grep '^clippy-' > /evidence/linux/smoke-clippy/clippy-component.txt
printf '%s\n' no > /evidence/linux/smoke-clippy/cargo-update-invoked.txt
exit 0
