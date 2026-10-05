#!/bin/bash
# Independent overlay on the fixed commit. Install clippy in this container, then test it.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_A199_ORACLE_DIR=/evidence/oracle
mkdir -p "$OO_A199_ORACLE_DIR"
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
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/tests/delimited.rs \
  crates/oo-connectors/tests/delimited_reader_a199_review.rs \
  contracts/connectors/delimited-reader-v1.md \
  > /evidence/source-sha256.txt
if ! cmp -s crates/oo-connectors/src/delimited.rs /blobs/delimited.rs \
  || ! cmp -s crates/oo-connectors/src/lib.rs /blobs/lib.rs \
  || ! cmp -s crates/oo-connectors/tests/delimited.rs /blobs/tests-delimited.rs \
  || ! cmp -s contracts/connectors/delimited-reader-v1.md /blobs/delimited-reader-v1.md \
  || ! cmp -s crates/oo-connectors/tests/delimited_reader_a199_review.rs /evidence/expect-review.rs
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  printf '%s\n' not_run > /evidence/fetch.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
echo "PHASE blob_cmp_exit=0"

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
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test delimited_reader_a199_review -- --nocapture \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  printf '%s\n' not_run > /evidence/hash-compare.exit
  exit "$test_exit"
fi

echo "=== sha256sum agrees with the rust oracle ==="
csv_got=$(sha256sum /evidence/oracle/positive.csv | awk '{print $1}')
csv_want=$(tr -d '[:space:]' < /evidence/oracle/positive.csv.sha256)
tsv_got=$(sha256sum /evidence/oracle/positive.tsv | awk '{print $1}')
tsv_want=$(tr -d '[:space:]' < /evidence/oracle/positive.tsv.sha256)
{
  echo "csv_sha256sum=$csv_got"
  echo "csv_rust=$csv_want"
  echo "tsv_sha256sum=$tsv_got"
  echo "tsv_rust=$tsv_want"
} > /evidence/hash-compare.txt
if [ "$csv_got" = "$csv_want" ] && [ "$tsv_got" = "$tsv_want" ]; then
  printf '%s\n' 0 > /evidence/hash-compare.exit
  echo "PHASE hash_compare_exit=0"
else
  printf '%s\n' 1 > /evidence/hash-compare.exit
  echo "PHASE hash_compare_exit=1"
  printf '%s\n' not_run > /evidence/clippy.exit
  exit 1
fi

echo "=== cargo clippy overlay test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --test delimited_reader_a199_review -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
