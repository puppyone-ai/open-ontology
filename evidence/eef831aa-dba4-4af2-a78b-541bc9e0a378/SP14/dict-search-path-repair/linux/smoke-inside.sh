#!/bin/bash
# Pre-edit smoke. Version and lock shape only. No cargo update, no tests.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a361/SP14/pg-backup-restore-review/linux/smoke-inside.sh
# commit c870a52817d172d13b7f890548bf620d55a623e7
# blob d8d536d3c12da8b1387287ec7226067c7187f0a5
# sha256 3b5e988f96687c888ab848bffeb9e0eb6b13284dda24afc9dfc00d0b4ca5bf34
# Change: both child locks. Each is a registry subset, package_names=211 source_lines=208.
set -euo pipefail
mkdir -p /evidence/linux/pre-edit-smoke
rustc --version > /evidence/linux/pre-edit-smoke/rustc-version.txt
cargo --version > /evidence/linux/pre-edit-smoke/cargo-version.txt
rustc --version --verbose > /evidence/linux/pre-edit-smoke/rustc-verbose.txt
rustup component list --toolchain 1.94.0 --installed > /evidence/linux/pre-edit-smoke/components.txt
printf '%s\n' no > /evidence/linux/pre-edit-smoke/cargo-update-invoked.txt
cp /calendar/Cargo.lock /evidence/linux/pre-edit-smoke/calendar-Cargo.lock.copy
cp /enum/Cargo.lock /evidence/linux/pre-edit-smoke/enum-Cargo.lock.copy
awk '
  /^name = / { names++ }
  /^source = / { sources++ }
  END { printf "package_names=%d source_lines=%d\n", names+0, sources+0 }
' /calendar/Cargo.lock > /evidence/linux/pre-edit-smoke/calendar-lock-shape.txt
awk '
  /^name = / { names++ }
  /^source = / { sources++ }
  END { printf "package_names=%d source_lines=%d\n", names+0, sources+0 }
' /enum/Cargo.lock > /evidence/linux/pre-edit-smoke/enum-lock-shape.txt
grep -q 'rustc 1.94.0' /evidence/linux/pre-edit-smoke/rustc-version.txt
grep -q 'package_names=211' /evidence/linux/pre-edit-smoke/calendar-lock-shape.txt
grep -q 'source_lines=208' /evidence/linux/pre-edit-smoke/calendar-lock-shape.txt
grep -q 'package_names=211' /evidence/linux/pre-edit-smoke/enum-lock-shape.txt
grep -q 'source_lines=208' /evidence/linux/pre-edit-smoke/enum-lock-shape.txt
printf '%s\n' 0 > /evidence/linux/pre-edit-smoke/inside.exit
