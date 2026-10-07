#!/bin/bash
# Hash the review tree after the host has written this run's inspect files.
# The only path skipped is the output SHA256SUMS.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
ready=/evidence/linux/inventory/host-meta-ready
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if [ -f "$ready" ]; then
    break
  fi
  sleep 1
done
if [ ! -f "$ready" ]; then
  echo "host meta was not ready" >&2
  exit 2
fi
if [ -f /evidence/SHA256SUMS ]; then
  echo "refusing to overwrite SHA256SUMS" >&2
  exit 2
fi
rustc --edition 2021 -D warnings -o /usr/local/bin/sha256_manifest /tools/sha256_manifest.rs
printf '0\n' > /evidence/linux/inventory/docker-run.exit
printf '%s\n' "oo-a319-inventory" > /evidence/linux/inventory/container-removed.txt
/usr/local/bin/sha256_manifest /evidence /evidence/SHA256SUMS > /tmp/sha256-inventory.log
status=$?
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$status" > /evidence/linux/inventory/docker-run.exit
  exit "$status"
fi
exit 0
