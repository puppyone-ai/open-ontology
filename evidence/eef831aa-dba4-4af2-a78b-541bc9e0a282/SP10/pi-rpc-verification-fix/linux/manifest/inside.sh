#!/bin/bash
# Compile the a204 manifest tool and write this evidence inventory.
# The origin-launch hash is finished before the evidence walk.
# Network none. No host shasum. Does not modify a279.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
rustc -vV
rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest
rustc --edition 2024 -D warnings /evidence/tools/manifest_eq.rs -o /usr/local/bin/manifest_eq
/usr/local/bin/sha256_manifest /origin-manifest /evidence/linux/manifest/origin-manifest.sha256
grep -F '9b28b43d9a9afe9cad69a5f086d7d96af4758f58c7e053a25efa52567a69cdd8  ./launch.sh' /evidence/linux/manifest/origin-manifest.sha256
/usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256
/usr/local/bin/sha256_manifest /evidence /tmp/evidence-rehash.sha256
/usr/local/bin/manifest_eq /evidence/MANIFEST.sha256 /tmp/evidence-rehash.sha256 > /tmp/eq.stdout
cat /tmp/eq.stdout
expected=$(awk -F= '$1=="expected"{print $2}' /tmp/eq.stdout)
fresh=$(awk -F= '$1=="fresh"{print $2}' /tmp/eq.stdout)
bad=$(awk -F= '$1=="bad"{print $2}' /tmp/eq.stdout)
want=$((expected + 1))
if [ "$bad" != 0 ] || [ "$fresh" != "$want" ]; then
  echo "self-check failed expected=$expected fresh=$fresh bad=$bad" >&2
  exit 1
fi
echo "self-check-ok expected=$expected fresh=$fresh"
