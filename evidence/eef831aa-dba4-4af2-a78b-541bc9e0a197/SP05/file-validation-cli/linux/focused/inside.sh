#!/bin/bash
# Focused Linux arm64 Rust 1.94 run of validate-file.
# Immutable git archive plus the three new files. No PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export OO_A197_ORACLE_DIR=/evidence/oracle
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
echo "=== focused presence ==="
if [ ! -f crates/oo-pipeline/src/bin/validate-file.rs ] \
  || [ ! -f crates/oo-pipeline/tests/validate_file_cli.rs ] \
  || [ ! -f contracts/pipeline/validate-file-cli-v1.md ]; then
  echo "validate-file sources are missing from the focused archive" >&2
  echo 2 > /evidence/presence.exit
  exit 2
fi
if [ ! -f crates/oo-connectors/src/delimited.rs ] || [ ! -f crates/oo-connectors/src/jsonl.rs ]; then
  echo "fixed-base file readers are missing from the focused archive" >&2
  echo 2 > /evidence/presence.exit
  exit 2
fi
echo 0 > /evidence/presence.exit
echo "=== archive hashes ==="
sha256sum \
  Cargo.toml \
  Cargo.lock \
  rust-toolchain.toml \
  crates/oo-pipeline/Cargo.toml \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/src/bin/validate-file.rs \
  crates/oo-pipeline/tests/validate_file_cli.rs \
  contracts/pipeline/validate-file-cli-v1.md \
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
mkdir -p /evidence/oracle
echo "=== cargo test validate_file_cli ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test validate_file_cli -- --nocapture > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  echo "worst_phase_exit=$test_exit"
  exit "$test_exit"
fi
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
set -e
echo "$clippy_add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$clippy_add_exit"
if [ "$clippy_add_exit" -ne 0 ]; then
  echo "worst_phase_exit=$clippy_add_exit"
  exit "$clippy_add_exit"
fi
echo "=== cargo clippy validate-file ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --bin validate-file --test validate_file_cli -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  echo "worst_phase_exit=$clippy_exit"
  exit "$clippy_exit"
fi
echo "=== rustfmt check ==="
set +e
rustup component add rustfmt --toolchain 1.94.0 > /evidence/rustfmt-add.stdout 2> /evidence/rustfmt-add.stderr
rustfmt_add_exit=$?
set -e
echo "$rustfmt_add_exit" > /evidence/rustfmt-add.exit
if [ "$rustfmt_add_exit" -ne 0 ]; then
  echo "worst_phase_exit=$rustfmt_add_exit"
  exit "$rustfmt_add_exit"
fi
set +e
rustfmt --edition 2024 --check \
  crates/oo-pipeline/src/bin/validate-file.rs \
  crates/oo-pipeline/tests/validate_file_cli.rs \
  > /evidence/rustfmt.stdout 2> /evidence/rustfmt.stderr
rustfmt_exit=$?
set -e
echo "$rustfmt_exit" > /evidence/rustfmt.exit
echo "PHASE rustfmt_exit=$rustfmt_exit"
echo "worst_phase_exit=$rustfmt_exit"
exit "$rustfmt_exit"
