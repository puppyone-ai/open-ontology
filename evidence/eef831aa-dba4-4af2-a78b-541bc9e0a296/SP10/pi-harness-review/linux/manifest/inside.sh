#!/bin/bash
# Hash the review directory. The manifest file itself is excluded by the tool.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
echo "=== identity ==="
rustc -vV
set +e
rustc --edition 2021 -O -o /usr/local/bin/sha256_manifest /oracle/sha256_manifest.rs > /tmp/rustc.stdout 2> /tmp/rustc.stderr
rustc_exit=$?
set -e
echo "PHASE rustc_exit=$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then
  cat /tmp/rustc.stderr >&2
  exit "$rustc_exit"
fi
/usr/local/bin/sha256_manifest /review /review/MANIFEST.sha256
echo "PHASE manifest_exit=0"
