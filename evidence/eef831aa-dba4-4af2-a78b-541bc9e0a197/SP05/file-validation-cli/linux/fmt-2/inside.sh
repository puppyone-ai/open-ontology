#!/bin/bash
# Format only the two new Rust files. No cargo build. No PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
cd /work
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
rustup show
echo "=== rustfmt component ==="
set +e
rustup component add rustfmt --toolchain 1.94.0 > /evidence/rustfmt-add.stdout 2> /evidence/rustfmt-add.stderr
add_exit=$?
set -e
echo "$add_exit" > /evidence/rustfmt-add.exit
echo "PHASE rustfmt_add_exit=$add_exit"
if [ "$add_exit" -ne 0 ]; then
  exit "$add_exit"
fi
echo "=== rustfmt ==="
set +e
rustfmt --edition 2024 --emit files validate-file.rs validate_file_cli.rs > /evidence/rustfmt.stdout 2> /evidence/rustfmt.stderr
fmt_exit=$?
set -e
echo "$fmt_exit" > /evidence/rustfmt.exit
echo "PHASE rustfmt_exit=$fmt_exit"
rustfmt --version > /evidence/rustfmt-version.txt
exit "$fmt_exit"
