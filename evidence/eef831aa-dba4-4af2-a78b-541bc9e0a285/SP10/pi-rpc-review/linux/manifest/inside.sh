#!/bin/bash
# Compile the a204 manifest tool and the a282 compare tool, then inventory this review.
# Network none. No host shasum. Does not modify a279 or a282.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
rustc -vV
rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest
rustc --edition 2024 -D warnings /evidence/tools/manifest_eq.rs -o /usr/local/bin/manifest_eq
/usr/local/bin/sha256_manifest /origin-manifest /evidence/linux/manifest/origin-manifest.sha256
grep -F 'df49151419241e9cfe9e2ef2405e6a59a302652905aae454617246006e44aa88  ./launch.sh' /evidence/linux/manifest/origin-manifest.sha256
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
