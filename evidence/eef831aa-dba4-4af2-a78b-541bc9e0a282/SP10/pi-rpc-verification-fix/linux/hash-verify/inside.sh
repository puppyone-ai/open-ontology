#!/bin/bash
# Compile the a204 manifest tool and check the recorded inventories.
# Network none. No host shasum. Does not write into the a279 tree.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
export CARGO_TERM_COLOR=never
rustc -vV > /proof/toolchain.txt
echo "=== compile ==="
set +e
rustc --edition 2024 -D warnings /src/sha256_manifest.rs -o /usr/local/bin/sha256_manifest > /proof/manifest-rustc.stdout 2> /proof/manifest-rustc.stderr
manifest_rc=$?
rustc --edition 2024 -D warnings /src/manifest_eq.rs -o /usr/local/bin/manifest_eq > /proof/eq-rustc.stdout 2> /proof/eq-rustc.stderr
eq_rc=$?
set -e
printf '%s\n' "$manifest_rc" > /proof/manifest-rustc.exit
printf '%s\n' "$eq_rc" > /proof/eq-rustc.exit
echo "PHASE manifest_rustc_exit=$manifest_rc eq_rustc_exit=$eq_rc"
if [ "$manifest_rc" -ne 0 ] || [ "$eq_rc" -ne 0 ]; then
  exit 1
fi
echo "=== a279 manifest ==="
/usr/local/bin/sha256_manifest /a279 /proof/a279-rehash.sha256 > /proof/a279-rehash.stdout 2> /proof/a279-rehash.stderr
printf '%s\n' "$?" > /proof/a279-rehash.exit
set +e
/usr/local/bin/manifest_eq /a279/MANIFEST.sha256 /proof/a279-rehash.sha256 > /proof/a279-compare.stdout 2> /proof/a279-compare.stderr
a279_rc=$?
set -e
printf '%s\n' "$a279_rc" > /proof/a279-compare.exit
echo "PHASE a279_compare_exit=$a279_rc"
echo "=== fixed spike ==="
/usr/local/bin/sha256_manifest /fixed-spike /proof/fixed-spike-rehash.sha256 > /proof/fixed-spike-rehash.stdout 2> /proof/fixed-spike-rehash.stderr
printf '%s\n' "$?" > /proof/fixed-spike-rehash.exit
set +e
/usr/local/bin/manifest_eq /a279/source-spike.sha256 /proof/fixed-spike-rehash.sha256 > /proof/fixed-spike-compare.stdout 2> /proof/fixed-spike-compare.stderr
fixed_rc=$?
set -e
printf '%s\n' "$fixed_rc" > /proof/fixed-spike-compare.exit
echo "PHASE fixed_spike_compare_exit=$fixed_rc"
echo "=== current spike ==="
/usr/local/bin/sha256_manifest /current-spike /proof/current-spike-rehash.sha256 > /proof/current-spike-rehash.stdout 2> /proof/current-spike-rehash.stderr
printf '%s\n' "$?" > /proof/current-spike-rehash.exit
set +e
/usr/local/bin/manifest_eq /proof/fixed-spike-rehash.sha256 /proof/current-spike-rehash.sha256 > /proof/current-vs-fixed.stdout 2> /proof/current-vs-fixed.stderr
current_rc=$?
set -e
printf '%s\n' "$current_rc" > /proof/current-vs-fixed.exit
echo "PHASE current_vs_fixed_exit=$current_rc"
echo "=== origin tool ==="
mkdir -p /only-our
cp /src/sha256_manifest.rs /only-our/sha256_manifest.rs
/usr/local/bin/sha256_manifest /origin /proof/origin-tool-rehash.sha256 > /proof/origin-tool-rehash.stdout 2> /proof/origin-tool-rehash.stderr
printf '%s\n' "$?" > /proof/origin-tool-rehash.exit
/usr/local/bin/sha256_manifest /only-our /proof/our-tool-rehash.sha256 > /proof/our-tool-rehash.stdout 2> /proof/our-tool-rehash.stderr
printf '%s\n' "$?" > /proof/our-tool-rehash.exit
set +e
/usr/local/bin/manifest_eq /proof/origin-tool-rehash.sha256 /proof/our-tool-rehash.sha256 > /proof/tool-compare.stdout 2> /proof/tool-compare.stderr
tool_rc=$?
set -e
printf '%s\n' "$tool_rc" > /proof/tool-compare.exit
echo "PHASE tool_compare_exit=$tool_rc"
if [ "$a279_rc" -ne 0 ] || [ "$fixed_rc" -ne 0 ] || [ "$tool_rc" -ne 0 ]; then
  exit 1
fi
if ! grep -qx 'bad=1' /proof/current-vs-fixed.stdout; then
  echo "expected one current-vs-fixed mismatch" >&2
  exit 4
fi
if ! grep -q '^mismatch ./src/drive.rs ' /proof/current-vs-fixed.stderr; then
  echo "current-vs-fixed mismatch is not drive.rs" >&2
  exit 4
fi
echo "hash-verify-ok"
exit 0
