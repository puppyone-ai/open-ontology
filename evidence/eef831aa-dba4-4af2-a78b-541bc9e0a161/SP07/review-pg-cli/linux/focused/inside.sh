#!/bin/bash
# Focused review-pg-draft CLI tests, legacy captured and structure CLI once, scoped Clippy.
set -u
cd /src
{
  echo "rustc=$(rustc --version)"
  rustc -vV
} > /evidence/rustc-version.txt
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
printf '%s\n' "$?" > /evidence/rustup-clippy.exit
set +e
cargo test -p oo-builder --test review_pg_cli -- --test-threads=4 > /evidence/review_pg_cli.stdout 2> /evidence/review_pg_cli.stderr
printf '%s\n' "$?" > /evidence/review_pg_cli.exit
cargo test -p oo-builder --test captured_pg_cli -- --test-threads=4 > /evidence/captured_pg_cli.stdout 2> /evidence/captured_pg_cli.stderr
printf '%s\n' "$?" > /evidence/captured_pg_cli.exit
cargo test -p oo-builder --test structure_draft_cli -- --test-threads=4 > /evidence/structure_draft_cli.stdout 2> /evidence/structure_draft_cli.stderr
printf '%s\n' "$?" > /evidence/structure_draft_cli.exit
cargo clippy -p oo-builder --bin oo-builder --test review_pg_cli --test captured_pg_cli -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
printf '%s\n' "$?" > /evidence/clippy.exit
set -e
review=$(cat /evidence/review_pg_cli.exit)
captured=$(cat /evidence/captured_pg_cli.exit)
legacy=$(cat /evidence/structure_draft_cli.exit)
clip=$(cat /evidence/clippy.exit)
if [ "$review" -ne 0 ] || [ "$captured" -ne 0 ] || [ "$legacy" -ne 0 ] || [ "$clip" -ne 0 ]; then
  exit 1
fi
exit 0
