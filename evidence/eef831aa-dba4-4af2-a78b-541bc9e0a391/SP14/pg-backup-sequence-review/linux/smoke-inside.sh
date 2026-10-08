#!/bin/bash
# Pre-review smoke of the immutable product tree. Version and lock shape only.
# No cargo update, no tests, no product writes.
set -euo pipefail
mkdir -p /evidence/linux/pre-edit-smoke
rustc --version > /evidence/linux/pre-edit-smoke/rustc-version.txt
cargo --version > /evidence/linux/pre-edit-smoke/cargo-version.txt
rustc --version --verbose > /evidence/linux/pre-edit-smoke/rustc-verbose.txt
rustup component list --toolchain 1.94.0 --installed > /evidence/linux/pre-edit-smoke/components.txt
printf '%s\n' no > /evidence/linux/pre-edit-smoke/cargo-update-invoked.txt
cp /src/Cargo.lock /evidence/linux/pre-edit-smoke/Cargo.lock.copy
awk '
  /^name = / { names++ }
  /^source = / { sources++ }
  END { printf "package_names=%d source_lines=%d\n", names+0, sources+0 }
' /src/Cargo.lock > /evidence/linux/pre-edit-smoke/lock-shape.txt
grep -q 'rustc 1.94.0' /evidence/linux/pre-edit-smoke/rustc-version.txt
grep -q 'package_names=1' /evidence/linux/pre-edit-smoke/lock-shape.txt
grep -q 'source_lines=0' /evidence/linux/pre-edit-smoke/lock-shape.txt
printf '%s\n' 0 > /evidence/linux/pre-edit-smoke/inside.exit
