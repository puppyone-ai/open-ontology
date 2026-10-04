#!/bin/bash
# Retry after the probe-2 UTF-8 snippet panic. Installs clippy in this container. Installs clippy in this container. Does not overwrite probe/.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export OO_A168_OUT=/evidence/out
cd /src
rustc -vV > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
cat /evidence/rustc-version.txt
cat /evidence/cargo-version.txt
sha256sum crates/oo-builder/tests/role_pack_a168_review.rs > /evidence/overlay-got.sha256
if ! cmp -s /evidence/overlay.sha256 /evidence/overlay-got.sha256; then
  echo "overlay bytes differ from the review source" >&2
  echo 2 > /evidence/overlay-hash.exit
  exit 2
fi
echo 0 > /evidence/overlay-hash.exit
sha256sum crates/oo-builder/src/role_pack.rs crates/oo-builder/src/lib.rs > /evidence/product-sha256.txt
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
clippy_component_exit=$?
set -e
echo "$clippy_component_exit" > /evidence/rustup-clippy.exit
echo "PHASE clippy_component_exit=$clippy_component_exit"
mkdir -p /evidence/out
echo "=== cargo test review overlay ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack --test role_pack_a168_review > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
echo "=== python oracle ==="
set +e
python3 /oracle.py /evidence/out > /evidence/oracle.stdout 2> /evidence/oracle.stderr
oracle_exit=$?
set -e
echo "$oracle_exit" > /evidence/oracle.exit
echo "PHASE oracle_exit=$oracle_exit"
echo "=== cargo clippy review test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack_a168_review -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
worst=0
for phase_exit in "$clippy_component_exit" "$test_exit" "$oracle_exit" "$clippy_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "$worst" > /evidence/worst.exit
echo "worst_phase_exit=$worst"
exit "$worst"
