#!/bin/bash
# Focused review overlay. Same lock rules. Does not rerun the author suite.
set -eu
echo "PHASE toolchain"
rustc --version
cargo --version
rustc --version | grep -q '1.94.0'
echo "PHASE tree"
test -f /src/Cargo.lock
test -f /src/tools/gateway-token-store/src/a314_review.rs
grep -q 'mod a314_review;' /src/tools/gateway-token-store/src/lib.rs
echo "PHASE overlay_present"
echo "PHASE root_lock"
sha256sum /src/Cargo.lock | awk '{print $1}' > /evidence/linux/overlay-2/root-lock.sha256
cmp /evidence/linux/overlay-2/root-lock.sha256 /evidence/tools/expected-root-lock.sha256
echo "PHASE subset"
rustc --edition 2024 -D warnings /evidence/tools/lock_subset.rs -o /target/lock_subset
set +e
/target/lock_subset /src/Cargo.lock /src/tools/gateway-token-store/Cargo.lock | tee /evidence/linux/overlay-2/subset.txt
subset_exit=${PIPESTATUS[0]}
set -eu
if [ "$subset_exit" -ne 0 ]; then
  echo "PHASE subset_failed"
  exit 4
fi
grep -q 'subset_ok registry=125 path=gateway-token-store' /evidence/linux/overlay-2/subset.txt
echo "PHASE fetch"
cd /src/tools/gateway-token-store
cargo fetch --locked --offline
echo "PHASE clippy_component"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0
fi
echo "PHASE test"
export CARGO_NET_OFFLINE=true
cargo test --locked --offline --lib a314_review -- --test-threads=1
echo "PHASE clippy"
cargo clippy --locked --offline --all-targets -- -D warnings
echo "PHASE crate_manifest"
rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /target/sha256_manifest
/target/sha256_manifest /src/tools/gateway-token-store /evidence/linux/overlay-2/crate-manifest.sha256
echo "PHASE overlay_ok"
