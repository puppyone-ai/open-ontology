#!/bin/bash
# Independent overlay on the sealed 59be14ba archive.
# Runs only validate_file_a204_review. Does not re-run validate_file_cli.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_A204_ORACLE_DIR=/evidence/oracle
mkdir -p "$OO_A204_ORACLE_DIR"
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

echo "=== pins ==="
sha256sum \
  crates/oo-pipeline/src/bin/validate-file.rs \
  crates/oo-pipeline/tests/validate_file_cli.rs \
  crates/oo-pipeline/tests/validate_file_a204_review.rs \
  contracts/pipeline/validate-file-cli-v1.md \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/jsonl.rs \
  crates/oo-connectors/src/lib.rs \
  > /evidence/source-sha256.txt
if ! cmp -s crates/oo-pipeline/src/bin/validate-file.rs /blobs/validate-file.rs \
  || ! cmp -s crates/oo-pipeline/tests/validate_file_cli.rs /blobs/validate_file_cli.rs \
  || ! cmp -s contracts/pipeline/validate-file-cli-v1.md /blobs/validate-file-cli-v1.md \
  || ! cmp -s crates/oo-connectors/src/delimited.rs /blobs/delimited.rs \
  || ! cmp -s crates/oo-connectors/src/jsonl.rs /blobs/jsonl.rs \
  || ! cmp -s crates/oo-connectors/src/lib.rs /blobs/lib.rs \
  || ! cmp -s crates/oo-pipeline/tests/validate_file_a204_review.rs /evidence/expect-review.rs
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
  printf '%s\n' not_run > /evidence/hash-compare.exit
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
  printf '%s\n' not_run > /evidence/hash-compare.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo test independent overlay ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test validate_file_a204_review -- --nocapture \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/hash-compare.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$test_exit"
fi

echo "=== sha256sum agrees with the rust oracle ==="
{
  for name in positive-csv positive-tsv positive-jsonl; do
    got=$(sha256sum "/evidence/oracle/${name}.bin" | awk '{print $1}')
    want=$(tr -d '[:space:]' < "/evidence/oracle/${name}.sha256")
    echo "${name}_sha256sum=$got"
    echo "${name}_rust=$want"
  done
} > /evidence/hash-compare.txt
hash_exit=0
while IFS= read -r line; do
  case "$line" in
    *_sha256sum=*) got=${line#*=} ;;
    *_rust=*)
      want=${line#*=}
      if [ "$got" != "$want" ]; then
        hash_exit=1
      fi
      ;;
  esac
done < /evidence/hash-compare.txt
printf '%s\n' "$hash_exit" > /evidence/hash-compare.exit
echo "PHASE hash_compare_exit=$hash_exit"
if [ "$hash_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$hash_exit"
fi

echo "=== cargo clippy overlay test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test validate_file_a204_review -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
