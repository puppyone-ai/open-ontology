#!/bin/bash
# Focused Linux arm64 Rust 1.94 smoke of the captured-pg compiler.
# Archive is parent 1a1425c9. The artifact-diff command is not in this tree.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true

echo "=== overlay absent ==="
if [ -f crates/oo-builder/src/bin/ontology-artifact-diff.rs ] \
  || [ -f crates/oo-builder/tests/ontology_artifact_diff.rs ] \
  || [ -f crates/oo-builder/tests/ontology_artifact_diff_a243_review.rs ]
then
  echo "artifact-diff files are already in the parent archive" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
if [ ! -f crates/oo-builder/src/captured_pg.rs ] \
  || [ ! -f crates/oo-builder/src/bin/oo-builder.rs ] \
  || [ ! -f contracts/builder/artifact-diff-v1.md ] \
  || [ ! -f examples/ontology-draft/failures/adapter.code ]
then
  echo "parent archive is missing the compiler, contract, or examples" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
sha256sum \
  crates/oo-builder/src/captured_pg.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  contracts/builder/artifact-diff-v1.md \
  > /evidence/source-sha256.txt

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/bin.exit
  exit "$fetch_exit"
fi

echo "=== cargo test captured_pg ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test captured_pg \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

echo "=== cargo build oo-builder ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-builder --bin oo-builder \
  > /evidence/bin.stdout 2> /evidence/bin.stderr
bin_exit=$?
set -e
printf '%s\n' "$bin_exit" > /evidence/bin.exit
echo "PHASE bin_exit=$bin_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$bin_exit" -ne 0 ]; then worst=$bin_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
