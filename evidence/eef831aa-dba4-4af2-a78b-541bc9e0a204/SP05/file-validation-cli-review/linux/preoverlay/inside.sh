#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 focused smoke of the fixed validate-file commit.
# The archive is 59be14ba. This script must not see the review overlay.
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

echo "=== source pins ==="
sha256sum \
  crates/oo-pipeline/src/bin/validate-file.rs \
  crates/oo-pipeline/tests/validate_file_cli.rs \
  contracts/pipeline/validate-file-cli-v1.md \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/jsonl.rs \
  crates/oo-connectors/src/lib.rs \
  > /evidence/archive-sha256.txt
if ! cmp -s crates/oo-pipeline/src/bin/validate-file.rs /blobs/validate-file.rs \
  || ! cmp -s crates/oo-pipeline/tests/validate_file_cli.rs /blobs/validate_file_cli.rs \
  || ! cmp -s contracts/pipeline/validate-file-cli-v1.md /blobs/validate-file-cli-v1.md \
  || ! cmp -s crates/oo-connectors/src/delimited.rs /blobs/delimited.rs \
  || ! cmp -s crates/oo-connectors/src/jsonl.rs /blobs/jsonl.rs \
  || ! cmp -s crates/oo-connectors/src/lib.rs /blobs/lib.rs
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f crates/oo-pipeline/tests/validate_file_a204_review.rs ]; then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
if ! sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo "archive hash mismatch" >&2
  printf '%s\n' 2 > /evidence/hash-compare.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/hash-compare.exit

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
  printf '%s\n' not_run > /evidence/lib.exit
  printf '%s\n' not_run > /evidence/cli.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test oo-pipeline lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib \
  > /evidence/lib.stdout 2> /evidence/lib.stderr
lib_exit=$?
set -e
printf '%s\n' "$lib_exit" > /evidence/lib.exit
echo "PHASE lib_exit=$lib_exit"

echo "=== cargo test validate_file_cli one pass ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test validate_file_cli \
  > /evidence/cli.stdout 2> /evidence/cli.stderr
cli_exit=$?
set -e
printf '%s\n' "$cli_exit" > /evidence/cli.exit
echo "PHASE cli_exit=$cli_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  worst=$component_exit
  if [ "$lib_exit" -ne 0 ]; then worst=$lib_exit; fi
  if [ "$cli_exit" -ne 0 ]; then worst=$cli_exit; fi
  echo "worst_phase_exit=$worst"
  exit "$worst"
fi

echo "=== cargo clippy validate-file ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline \
  --bin validate-file --test validate_file_cli -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$lib_exit" -ne 0 ]; then worst=$lib_exit; fi
if [ "$cli_exit" -ne 0 ]; then worst=$cli_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
