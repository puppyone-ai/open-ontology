#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 smoke of the fixed delimited reader.
# The archive is commit 671507b2. This script must not see the review overlay.
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

echo "=== source pins ==="
sha256sum \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/tests/delimited.rs \
  contracts/connectors/delimited-reader-v1.md \
  > /evidence/source-sha256.txt
if ! cmp -s crates/oo-connectors/src/delimited.rs /blobs/delimited.rs \
  || ! cmp -s crates/oo-connectors/src/lib.rs /blobs/lib.rs \
  || ! cmp -s crates/oo-connectors/tests/delimited.rs /blobs/tests-delimited.rs \
  || ! cmp -s contracts/connectors/delimited-reader-v1.md /blobs/delimited-reader-v1.md
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f crates/oo-connectors/tests/delimited_reader_a199_review.rs ]; then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
test -f contracts/source/pgsql/empty-catalog.json
test -f crates/oo-connectors/src/delimited.rs

echo "=== cargo fetch locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/lib.exit
  printf '%s\n' not_run > /evidence/test-delimited.exit
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
  printf '%s\n' not_run > /evidence/lib.exit
  printf '%s\n' not_run > /evidence/test-delimited.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo test oo-connectors lib offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib \
  > /evidence/lib.stdout 2> /evidence/lib.stderr
lib_exit=$?
set -e
printf '%s\n' "$lib_exit" > /evidence/lib.exit
echo "PHASE lib_exit=$lib_exit"
if [ "$lib_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test-delimited.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$lib_exit"
fi

echo "=== cargo test delimited offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test delimited \
  > /evidence/test-delimited.stdout 2> /evidence/test-delimited.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test-delimited.exit
echo "PHASE test_delimited_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$test_exit"
fi

echo "=== cargo clippy oo-connectors all targets offline ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
