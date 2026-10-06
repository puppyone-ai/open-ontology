#!/bin/bash
# Independent overlay-2 on the sealed 0a30fcda archive.
# Runs only parse_file_a251_review. Does not re-run parse_file_cli.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_A251_ORACLE_DIR=/evidence/oracle
mkdir -p "$OO_A251_ORACLE_DIR"
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
  crates/oo-connectors/src/bin/parse-file.rs \
  crates/oo-connectors/tests/parse_file_cli.rs \
  crates/oo-connectors/tests/parse_file_a251_review.rs \
  contracts/connectors/parse-file-cli-v1.md \
  crates/oo-connectors/src/bin/oo-file-parser-worker.rs \
  crates/oo-connectors/src/file_worker_parent.rs \
  crates/oo-connectors/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/source-sha256.txt
if ! cmp -s crates/oo-connectors/src/bin/parse-file.rs /blobs/parse-file.rs \
  || ! cmp -s crates/oo-connectors/tests/parse_file_cli.rs /blobs/parse_file_cli.rs \
  || ! cmp -s contracts/connectors/parse-file-cli-v1.md /blobs/parse-file-cli-v1.md \
  || ! cmp -s crates/oo-connectors/tests/parse_file_a251_review.rs /evidence/expect-review.rs
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  printf '%s\n' not_run > /evidence/fetch-offline.exit
  printf '%s\n' not_run > /evidence/fetch.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/hash-compare.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
echo "PHASE blob_cmp_exit=0"

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
timeout --signal=KILL 3600 \
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test parse_file_a251_review -- --test-threads=1 --nocapture \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

if [ "$test_exit" -eq 0 ]; then
  echo "=== sha256sum agrees with the rust oracle ==="
  {
    for name in csv tsv jsonl empty; do
      got=$(sha256sum "/evidence/oracle/${name}.bin" | awk '{print $1}')
      want=$(tr -d '[:space:]' < "/evidence/oracle/${name}.sha256")
      echo "${name}_sha256sum=$got"
      echo "${name}_rust=$want"
      pgot=$(sha256sum "/evidence/oracle/${name}.preimage.txt" | awk '{print $1}')
      pwant=$(tr -d '[:space:]' < "/evidence/oracle/${name}.lineage.sha256")
      echo "${name}_preimage_sha256sum=$pgot"
      echo "${name}_lineage_rust=$pwant"
    done
  } > /evidence/hash-compare.txt
  hash_exit=0
  got=
  while IFS= read -r line; do
    case "$line" in
      *_sha256sum=*|*_preimage_sha256sum=*) got=${line#*=} ;;
      *_rust=*|*_lineage_rust=*)
        want=${line#*=}
        if [ "$got" != "$want" ]; then
          hash_exit=1
        fi
        ;;
    esac
  done < /evidence/hash-compare.txt
  printf '%s\n' "$hash_exit" > /evidence/hash-compare.exit
  echo "PHASE hash_compare_exit=$hash_exit"
else
  printf '%s\n' not_run > /evidence/hash-compare.exit
  hash_exit=0
fi

echo "=== cargo clippy review harness ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --test parse_file_a251_review -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$hash_exit" -ne 0 ]; then worst=$hash_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
