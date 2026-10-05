#!/bin/bash
# Fourth overlay on the sealed 6bd2455849da2527973c136968d4549ecd98d770 archive.
# Runs only file_parser_worker_a224_review. Offline fetch. No parent export.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export OO_A224_ORACLE_DIR=/evidence/oracle
mkdir -p "$OO_A224_ORACLE_DIR" /target/review-bin
cd /src

mark_missing() {
  for name in "$@"; do
    if [ ! -f "/evidence/${name}" ]; then
      printf '%s\n' not_run > "/evidence/${name}"
    fi
  done
}

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
release=$(rustc -vV | awk '/^release:/{print $2}')
printf '%s\n' "$release" > /evidence/rustc-release.txt
if [ "$release" != "1.94.0" ]; then
  echo "toolchain is ${release}" >&2
  printf '%s\n' 2 > /evidence/toolchain.exit
  mark_missing blob-cmp.exit parent-absent.exit fetch.exit clippy-component.exit test.exit \
    hash-compare.exit clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/toolchain.exit
echo "PHASE toolchain=1.94.0"

echo "=== pins ==="
sha256sum \
  crates/oo-connectors/src/bin/oo-file-parser-worker.rs \
  crates/oo-connectors/tests/file_parser_worker.rs \
  crates/oo-connectors/tests/file_parser_worker_a224_review.rs \
  contracts/connectors/file-parser-worker-v1.md \
  crates/oo-connectors/Cargo.toml \
  Cargo.lock \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/jsonl.rs \
  crates/oo-connectors/src/lib.rs \
  > /evidence/source-sha256.txt
if ! cmp -s crates/oo-connectors/src/bin/oo-file-parser-worker.rs /blobs/oo-file-parser-worker.rs \
  || ! cmp -s crates/oo-connectors/tests/file_parser_worker.rs /blobs/file_parser_worker.rs \
  || ! cmp -s contracts/connectors/file-parser-worker-v1.md /blobs/file-parser-worker-v1.md \
  || ! cmp -s crates/oo-connectors/Cargo.toml /blobs/oo-connectors-Cargo.toml \
  || ! cmp -s Cargo.lock /blobs/Cargo.lock \
  || ! cmp -s crates/oo-connectors/src/delimited.rs /blobs/delimited.rs \
  || ! cmp -s crates/oo-connectors/src/jsonl.rs /blobs/jsonl.rs \
  || ! cmp -s crates/oo-connectors/src/lib.rs /blobs/lib.rs \
  || ! cmp -s crates/oo-connectors/tests/file_parser_worker_a224_review.rs /evidence/expect-review.rs
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  mark_missing parent-absent.exit fetch.exit clippy-component.exit test.exit hash-compare.exit clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -e crates/oo-connectors/src/file_worker_parent.rs ] || grep -q file_worker_parent crates/oo-connectors/src/lib.rs; then
  echo "parent export is in the archive" >&2
  printf '%s\n' 2 > /evidence/parent-absent.exit
  mark_missing fetch.exit clippy-component.exit test.exit hash-compare.exit clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/parent-absent.exit
echo "PHASE blob_cmp_exit=0"

echo "=== cargo fetch locked offline ==="
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  mark_missing clippy-component.exit test.exit hash-compare.exit clippy.exit
  exit "$fetch_exit"
fi

echo "=== rustup component add clippy ==="
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  mark_missing test.exit hash-compare.exit clippy.exit
  exit "$component_exit"
fi

echo "=== cargo test independent overlay ==="
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test file_parser_worker_a224_review -- --nocapture --test-threads=8 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

hash_exit=0
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/hash-compare.exit
else
  echo "=== rust oracle and sha256sum ==="
  rustc --edition 2024 -D warnings /tools/sha256_files.rs -o /target/review-bin/sha256_files \
    > /evidence/hash-rustc.stdout 2> /evidence/hash-rustc.stderr
  hash_rustc=$?
  printf '%s\n' "$hash_rustc" > /evidence/hash-rustc.exit
  if [ "$hash_rustc" -ne 0 ]; then
    hash_exit=$hash_rustc
    printf '%s\n' "$hash_exit" > /evidence/hash-compare.exit
  else
    /target/review-bin/sha256_files check-oracle /evidence/oracle \
      shelf.csv tab.tsv notes.jsonl \
      shelf-profile.txt shelf-schema.txt \
      notes-profile.txt notes-schema.txt \
      empty.bin > /evidence/hash-compare.txt 2> /evidence/hash-compare.stderr
    hash_exit=$?
    {
      echo "rust_exit=$hash_exit"
      for name in shelf.csv tab.tsv notes.jsonl shelf-profile.txt shelf-schema.txt notes-profile.txt notes-schema.txt empty.bin; do
        got=$(/usr/bin/sha256sum "/evidence/oracle/${name}" | awk '{print $1}')
        want=$(tr -d '[:space:]' < "/evidence/oracle/${name}.sha256")
        echo "${name} sha256sum=${got} rust=${want}"
        if [ "$got" != "$want" ]; then
          hash_exit=1
        fi
      done
    } >> /evidence/hash-compare.txt
    printf '%s\n' "$hash_exit" > /evidence/hash-compare.exit
  fi
fi
echo "PHASE hash_compare_exit=$hash_exit"

echo "=== cargo clippy overlay test ==="
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors \
  --test file_parser_worker_a224_review -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$hash_exit" -ne 0 ]; then worst=$hash_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
