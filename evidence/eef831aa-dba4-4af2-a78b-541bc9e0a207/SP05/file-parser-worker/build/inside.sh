#!/bin/bash
# Update only the Arrow IPC 60 closure, then test and clippy the worker offline.
set -eu
cd /src
{
  echo "started=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  rustc --version
  cargo --version
  echo "RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-}"
  rustup show
} > /evidence/toolchain.txt

installed=$(rustup component list --toolchain 1.94.0 --installed)
printf '%s\n' "$installed" > /evidence/components-before.txt
if ! printf '%s\n' "$installed" | grep -q '^clippy'; then
  rustup component add --toolchain 1.94.0 clippy
fi
if ! printf '%s\n' "$installed" | grep -q '^rustfmt'; then
  rustup component add --toolchain 1.94.0 rustfmt
fi
rustup component list --toolchain 1.94.0 --installed > /evidence/components.txt

cp -p Cargo.lock /evidence/Cargo.lock.before
set +e
cargo update -p arrow-ipc --precise 60.0.0
update_exit=$?
set -e
printf '%s\n' "$update_exit" > /evidence/cargo-update.exit
if [ "$update_exit" -ne 0 ]; then
  exit "$update_exit"
fi
diff -u /evidence/Cargo.lock.before Cargo.lock > /evidence/lock.diff || true

set +e
cargo fetch --locked
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/cargo-fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

rustfmt --edition 2024 \
  crates/oo-connectors/src/bin/oo-file-parser-worker.rs \
  crates/oo-connectors/tests/file_parser_worker.rs

set +e
cargo test --locked --offline -p oo-connectors --test file_parser_worker -- --test-threads=8
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

set +e
cargo clippy --locked --offline -p oo-connectors --bin oo-file-parser-worker --test file_parser_worker -- -D warnings
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

rustfmt --edition 2024 --check \
  crates/oo-connectors/src/bin/oo-file-parser-worker.rs \
  crates/oo-connectors/tests/file_parser_worker.rs
printf '%s\n' "$?" > /evidence/rustfmt-check.exit
echo "inside_ok=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
