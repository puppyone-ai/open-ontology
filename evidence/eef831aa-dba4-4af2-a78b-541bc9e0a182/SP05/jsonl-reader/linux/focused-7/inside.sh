#!/bin/bash
# Clean clippy of every oo-connectors target after cargo clean -p oo-connectors.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
echo "=== identity ==="
rustc -vV
cargo -V
test -f crates/oo-connectors/src/jsonl.rs
test -f crates/oo-connectors/tests/jsonl_reader.rs
echo "=== cargo clean oo-connectors ==="
cargo clean --manifest-path Cargo.toml -p oo-connectors > /evidence/clean.stdout 2> /evidence/clean.stderr
echo $? > /evidence/clean.exit
echo "=== cargo clippy all targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
