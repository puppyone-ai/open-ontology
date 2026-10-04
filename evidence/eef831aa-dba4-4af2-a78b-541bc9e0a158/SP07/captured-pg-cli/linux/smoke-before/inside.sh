#!/bin/bash
# Pre-edit Linux/arm64 Rust 1.94 library and scoped Clippy smoke.
# Immutable archive. No rustfmt. No product writes.
set -u
cd /src
{
  echo "rustc=$(rustc --version)"
  rustc -vV
} > /evidence/rustc-version.txt
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
printf '%s\n' "$?" > /evidence/rustup-clippy.exit
set +e
cargo test -p oo-builder --lib -- --test-threads=8 > /evidence/lib.stdout 2> /evidence/lib.stderr
printf '%s\n' "$?" > /evidence/lib.exit
cargo clippy -p oo-builder --lib --test captured_pg -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
printf '%s\n' "$?" > /evidence/clippy.exit
set -e
lib=$(cat /evidence/lib.exit)
clip=$(cat /evidence/clippy.exit)
if [ "$lib" -ne 0 ] || [ "$clip" -ne 0 ]; then
  exit 1
fi
exit 0
