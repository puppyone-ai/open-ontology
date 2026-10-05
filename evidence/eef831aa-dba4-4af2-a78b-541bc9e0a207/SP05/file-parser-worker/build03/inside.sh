#!/bin/bash
# Add the Arrow IPC 60 closure by resolving oo-connectors, then test offline.
# build01 failed: cargo update -p arrow-ipc cannot see a crate that is not in the lock yet.
# cargo fetch has no -p in 1.94. A workspace fetch keeps locked versions and adds the missing edge.
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
cargo fetch --manifest-path Cargo.toml
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/cargo-fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
diff -u /evidence/Cargo.lock.before Cargo.lock > /evidence/lock.diff || true
set +e
bash /evidence/lock-gate.sh /evidence/Cargo.lock.before Cargo.lock
gate_exit=$?
set -e
printf '%s\n' "$gate_exit" > /evidence/lock-gate.exit
if [ "$gate_exit" -ne 0 ]; then
  exit "$gate_exit"
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
echo "inside_ok=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
