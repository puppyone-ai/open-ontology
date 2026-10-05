#!/bin/bash
# Focused Linux arm64 Rust 1.94 run: JSONL reader tests, connectors --lib, clippy.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export OO_JSONL_ORACLE_DIR=/evidence/oracle
mkdir -p "$OO_JSONL_ORACLE_DIR"
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
echo "=== owned sources ==="
test -f crates/oo-connectors/src/jsonl.rs
test -f crates/oo-connectors/tests/jsonl_reader.rs
test -f contracts/source/jsonl-reader-v1.md
grep -q 'mod jsonl;' crates/oo-connectors/src/lib.rs
sha256sum \
  crates/oo-connectors/src/jsonl.rs \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/tests/jsonl_reader.rs \
  contracts/source/jsonl-reader-v1.md \
  > /evidence/source-sha256.txt
echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
echo "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "not_run" > /evidence/test.exit
  echo "not_run" > /evidence/lib.exit
  echo "not_run" > /evidence/clippy.exit
  exit "$fetch_exit"
fi
echo "=== cargo test jsonl_reader ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test jsonl_reader -- --nocapture \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  echo "not_run" > /evidence/lib.exit
  echo "not_run" > /evidence/clippy.exit
  exit "$test_exit"
fi
echo "=== cargo test oo-connectors lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib \
  > /evidence/lib.stdout 2> /evidence/lib.stderr
lib_exit=$?
set -e
echo "$lib_exit" > /evidence/lib.exit
echo "PHASE lib_exit=$lib_exit"
if [ "$lib_exit" -ne 0 ]; then
  echo "not_run" > /evidence/clippy.exit
  exit "$lib_exit"
fi
echo "=== rustup component add clippy ==="
set +e
rustup component add clippy > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
echo "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  echo "not_run" > /evidence/clippy.exit
  exit "$component_exit"
fi
echo "=== cargo clippy oo-connectors all targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
