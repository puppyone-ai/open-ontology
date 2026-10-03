#!/bin/bash
# Non-login shell so the image PATH keeps cargo and rustup.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
echo "PATH=$PATH"
rustup component add clippy --toolchain 1.94.0
add_exit=$?
echo "clippy_add_exit=$add_exit"
printf '%s\n' "$add_exit" > /evidence/clippy-add-rerun.exit
if [ "$add_exit" -ne 0 ]; then
  printf '%s\n' "$add_exit" > /evidence/clippy-rerun.exit
  exit "$add_exit"
fi
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set +e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /evidence/clippy-rerun.exit
exit "$clippy_exit"
