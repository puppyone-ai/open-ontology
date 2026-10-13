#!/bin/bash
# Focused tests and clippy for this crate only. No cargo update. No network fetch.
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
cargo test --locked --offline --all-targets > "$EV/phases/test.stdout" 2> "$EV/phases/test.stderr"
test_exit=$?
printf '%s\n' "$test_exit" > "$EV/phases/test.exit"
echo "PHASE test=$test_exit"
if ! cargo clippy --version > "$EV/clippy-version.txt" 2>"$EV/phases/clippy-version.stderr"; then
  rustup component add clippy --toolchain 1.94.0 > "$EV/phases/clippy-component.stdout" 2>"$EV/phases/clippy-component.stderr"
  component_exit=$?
  printf '%s\n' "$component_exit" > "$EV/phases/clippy-component.exit"
  echo "PHASE clippy-component=$component_exit"
  cargo clippy --version >> "$EV/clippy-version.txt" 2>>"$EV/phases/clippy-version.stderr" || true
fi
cargo clippy --locked --offline --all-targets --no-deps -- -D warnings > "$EV/phases/clippy.stdout" 2>"$EV/phases/clippy.stderr"
clippy_exit=$?
printf '%s\n' "$clippy_exit" > "$EV/phases/clippy.exit"
echo "PHASE clippy=$clippy_exit"
if [ "$test_exit" -ne 0 ] || [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
BIN=/target/debug/minimal-loop-acceptance
if [ ! -x "$BIN" ]; then
  echo "PHASE bin=missing"
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
"$BIN" fixture --out "$EV/fixture" > "$EV/phases/fixture.stdout" 2> "$EV/phases/fixture.stderr"
fixture_exit=$?
printf '%s\n' "$fixture_exit" > "$EV/phases/fixture.exit"
echo "PHASE fixture=$fixture_exit"
kit_exit=0
if [ "${KIT_REQUIRED:-0}" = "1" ]; then
  "$BIN" kit --root /opt/pi-runtime-kit --out "$EV/kit" > "$EV/phases/kit.stdout" 2> "$EV/phases/kit.stderr"
  kit_exit=$?
  printf '%s\n' "$kit_exit" > "$EV/phases/kit.exit"
  echo "PHASE kit=$kit_exit"
fi
if [ "$fixture_exit" -ne 0 ] || [ "$kit_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$EV/focused.exit"
  exit 1
fi
printf '%s\n' 0 > "$EV/focused.exit"
exit 0
