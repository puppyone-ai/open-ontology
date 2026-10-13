#!/bin/bash
# Focused tests and clippy after the a415 gate edit. No cargo update. No network fetch.
# Clippy is already in the pinned image. Do not rustup component add.
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
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > "$EV/phases/toolchain.exit"; printf '%s\n' 2 > "$EV/focused.exit"; exit 2 ;;
esac
if ! grep -q aarch64 "$EV/uname.txt"; then
  echo "not arm64" >&2
  printf '%s\n' 2 > "$EV/phases/arch.exit"
  printf '%s\n' 2 > "$EV/focused.exit"
  exit 2
fi
printf '%s\n' 0 > "$EV/phases/arch.exit"
sha256sum src/*.rs tests/protocol.rs CONTRACT.md README.md BULK-QUESTIONS.md linux/a415/config/fresh-campaign.json > "$EV/postchange.sha256"
cargo test --locked --offline --all-targets > "$EV/phases/test.stdout" 2>"$EV/phases/test.stderr"
test_exit=$?
printf '%s\n' "$test_exit" > "$EV/phases/test.exit"
echo "PHASE test=$test_exit"
if cargo clippy --version > "$EV/clippy-version.txt" 2>"$EV/phases/clippy-version.stderr"; then
  cargo clippy --locked --offline --all-targets --no-deps -- -D warnings > "$EV/phases/clippy.stdout" 2>"$EV/phases/clippy.stderr"
  clippy_exit=$?
  printf '%s\n' "$clippy_exit" > "$EV/phases/clippy.exit"
  echo "PHASE clippy=$clippy_exit"
else
  echo "clippy component is absent from the pinned image; not installing" > "$EV/clippy-absent.txt"
  echo "clippy component is absent from the pinned image; not installing" >&2
  printf '%s\n' 2 > "$EV/phases/clippy.exit"
  clippy_exit=2
  echo "PHASE clippy=absent"
fi
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
BIN=/target/debug/minimal-loop-acceptance
if [ ! -x "$BIN" ]; then
  echo "PHASE bin=missing"
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
"$BIN" fixture --out "$EV/fixture" > "$EV/phases/fixture.stdout" 2>"$EV/phases/fixture.stderr"
fixture_exit=$?
printf '%s\n' "$fixture_exit" > "$EV/phases/fixture.exit"
echo "PHASE fixture=$fixture_exit"
if [ "$fixture_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' 2 > "$EV/focused.exit"
  exit 2
fi
printf '%s\n' 0 > "$EV/focused.exit"
exit 0
