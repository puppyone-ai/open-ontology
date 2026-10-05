#!/bin/bash
# Full clippy after the focused-7 clean removed oo-connectors artifacts.
# Clippy is installed in this container. It is not kept in the image.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
echo "=== identity ==="
rustc -vV
cargo -V
test -f crates/oo-connectors/src/jsonl.rs
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
echo "=== cargo clippy all targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
