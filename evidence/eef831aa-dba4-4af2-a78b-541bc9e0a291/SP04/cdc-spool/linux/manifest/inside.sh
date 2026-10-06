#!/bin/bash
# Std-only inventory. Compiles the byte-copied a199 manifest tool and hashes
# the product tree, then this evidence directory. Network is none.
set -u
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
mkdir -p /tmp
echo "=== identity ==="
rustc -vV
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  exit 2
fi
rustc --edition 2024 -D warnings /manifest-src/sha256_manifest.rs -o /usr/local/bin/sha256_manifest
sha256sum /manifest-src/sha256_manifest.rs > /evidence/tools/sha256_manifest.sha256
/usr/local/bin/sha256_manifest /product /evidence/product.sha256
/usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256
echo MANIFEST_OK
