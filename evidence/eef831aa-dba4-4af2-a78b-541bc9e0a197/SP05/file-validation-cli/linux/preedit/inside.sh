#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of oo-pipeline.
# Immutable git archive. The validate-file command is not in this tree.
# No PostgreSQL. No product edit.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
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
echo "=== preedit absence ==="
if [ -f crates/oo-pipeline/src/bin/validate-file.rs ] \
  || [ -f crates/oo-pipeline/tests/validate_file_cli.rs ] \
  || [ -f contracts/pipeline/validate-file-cli-v1.md ]; then
  echo "validate-file sources are in the preedit archive" >&2
  echo 2 > /evidence/absence.exit
  exit 2
fi
if [ ! -f crates/oo-connectors/src/delimited.rs ] || [ ! -f crates/oo-connectors/src/jsonl.rs ]; then
  echo "fixed-base file readers are missing from the preedit archive" >&2
  echo 2 > /evidence/absence.exit
  exit 2
fi
echo 0 > /evidence/absence.exit
echo "=== archive hashes ==="
sha256sum \
  Cargo.toml \
  Cargo.lock \
  rust-toolchain.toml \
  crates/oo-pipeline/Cargo.toml \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/jsonl.rs \
  > /evidence/archive-sha256.txt
if sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo 0 > /evidence/hash-compare.exit
else
  echo "archive hash mismatch" >&2
  echo 2 > /evidence/hash-compare.exit
  exit 2
fi
echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
echo "$offline_fetch_exit" > /evidence/fetch-offline.exit
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
echo "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "not_run" > /evidence/test.exit
  echo "worst_phase_exit=$fetch_exit"
  exit "$fetch_exit"
fi
echo "=== cargo test oo-pipeline lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  echo "worst_phase_exit=$test_exit"
  exit "$test_exit"
fi
echo "=== cargo test oo-pipeline bins ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --bins > /evidence/bins.stdout 2> /evidence/bins.stderr
bins_exit=$?
set -e
echo "$bins_exit" > /evidence/bins.exit
echo "PHASE bins_exit=$bins_exit"
echo "worst_phase_exit=$bins_exit"
exit "$bins_exit"
