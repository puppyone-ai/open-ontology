#!/bin/bash
# Clippy was absent in overlay-2. Install it here, then repeat the overlay test and clippy.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_JSONL_REVIEW_ORACLE_DIR=/evidence/oracle
mkdir -p "$OO_JSONL_REVIEW_ORACLE_DIR"
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -a
id

echo "=== pins ==="
sha256sum \
  crates/oo-connectors/src/jsonl.rs \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/tests/jsonl_reader.rs \
  crates/oo-connectors/tests/jsonl_reader_a185_review.rs \
  contracts/source/jsonl-reader-v1.md \
  > /evidence/source-sha256.txt
cmp -s crates/oo-connectors/src/jsonl.rs /evidence/expect-jsonl.rs
cmp_exit=$?
printf '%s\n' "$cmp_exit" > /evidence/blob-cmp.exit
echo "PHASE blob_cmp_exit=$cmp_exit"
if [ "$cmp_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/fetch.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$cmp_exit"
fi

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo test independent overlay ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test jsonl_reader_a185_review -- --nocapture \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$test_exit"
fi

echo "=== cargo clippy overlay test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --test jsonl_reader_a185_review -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
