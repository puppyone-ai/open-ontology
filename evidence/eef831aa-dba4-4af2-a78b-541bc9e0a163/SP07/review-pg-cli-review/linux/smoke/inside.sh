#!/bin/bash
# Linux/arm64 Rust 1.94 lib smoke, scoped Clippy, legacy CLI, independent oracle.
set -u
cd /src
mkdir -p /evidence/oracle-out
{
  echo "rustc=$(rustc --version)"
  rustc -vV
  echo "cargo=$(cargo --version)"
} > /evidence/rustc-version.txt
if ! rustc --version | grep -q 'rustc 1.94.0 '; then
  echo "wrong rustc" >> /evidence/rustc-version.txt
  printf '%s\n' 4 > /evidence/rustc.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/rustc.exit
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
printf '%s\n' "$?" > /evidence/rustup-clippy.exit
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
printf '%s\n' "$?" > /evidence/fetch.exit
fetch=$(cat /evidence/fetch.exit)
if [ "$fetch" -ne 0 ]; then
  printf '%s\n' 1 > /evidence/lib.exit
  printf '%s\n' 1 > /evidence/oracle.exit
  printf '%s\n' 1 > /evidence/structure_draft_cli.exit
  printf '%s\n' 1 > /evidence/captured_pg_cli.exit
  printf '%s\n' 1 > /evidence/review_pg_cli.exit
  printf '%s\n' 1 > /evidence/clippy.exit
  exit 1
fi
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib -- --test-threads=8 > /evidence/lib.stdout 2> /evidence/lib.stderr
printf '%s\n' "$?" > /evidence/lib.exit
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test a163_oracle -- --test-threads=1 > /evidence/oracle.stdout 2> /evidence/oracle.stderr
printf '%s\n' "$?" > /evidence/oracle.exit
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test structure_draft_cli -- --test-threads=4 > /evidence/structure_draft_cli.stdout 2> /evidence/structure_draft_cli.stderr
printf '%s\n' "$?" > /evidence/structure_draft_cli.exit
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test captured_pg_cli -- --test-threads=4 > /evidence/captured_pg_cli.stdout 2> /evidence/captured_pg_cli.stderr
printf '%s\n' "$?" > /evidence/captured_pg_cli.exit
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test review_pg_cli -- --test-threads=4 > /evidence/review_pg_cli.stdout 2> /evidence/review_pg_cli.stderr
printf '%s\n' "$?" > /evidence/review_pg_cli.exit
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --lib --bin oo-builder --test a163_oracle --test review_pg_cli --test captured_pg_cli --test structure_draft_cli -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
printf '%s\n' "$?" > /evidence/clippy.exit
set -e
lib=$(cat /evidence/lib.exit)
oracle=$(cat /evidence/oracle.exit)
legacy=$(cat /evidence/structure_draft_cli.exit)
captured=$(cat /evidence/captured_pg_cli.exit)
author=$(cat /evidence/review_pg_cli.exit)
clip=$(cat /evidence/clippy.exit)
if [ "$lib" -ne 0 ] || [ "$oracle" -ne 0 ] || [ "$legacy" -ne 0 ] || [ "$captured" -ne 0 ] || [ "$author" -ne 0 ] || [ "$clip" -ne 0 ]; then
  exit 1
fi
exit 0
