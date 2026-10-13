#!/bin/bash
# Pre-change Linux arm64 Rust 1.94 smoke of the runner that a415 is about to edit.
# No cargo update, no network fetch, no PostgreSQL, no secret mounts.
set -u
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/target/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
EV=/evidence
mkdir -p "$EV/phases" "$TMPDIR"
cd /src/testing/minimal-loop-acceptance
rustc --version > "$EV/rustc-version.txt"
uname -m > "$EV/uname.txt"
got=$(cat "$EV/rustc-version.txt")
case "$got" in
  "rustc 1.94.0 "*) printf '%s\n' 0 > "$EV/phases/toolchain.exit" ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > "$EV/phases/toolchain.exit"; printf '%s\n' 2 > "$EV/smoke.exit"; exit 2 ;;
esac
if ! grep -q aarch64 "$EV/uname.txt"; then
  echo "not arm64" >&2
  printf '%s\n' 2 > "$EV/phases/arch.exit"
  printf '%s\n' 2 > "$EV/smoke.exit"
  exit 2
fi
printf '%s\n' 0 > "$EV/phases/arch.exit"
sha256sum src/*.rs tests/protocol.rs CONTRACT.md README.md BULK-QUESTIONS.md > "$EV/prechange.sha256"
cargo test --locked --offline --all-targets > "$EV/phases/test.stdout" 2> "$EV/phases/test.stderr"
test_exit=$?
printf '%s\n' "$test_exit" > "$EV/phases/test.exit"
echo "PHASE test=$test_exit"
printf '%s\n' "$test_exit" > "$EV/smoke.exit"
exit "$test_exit"
