#!/bin/bash
# Offline Clippy of the changed acceptance package, then the new tests only.
# Network is none. Do not rustup component add. Do not run the unchanged protocol tests.
set -u
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/target/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/rustup
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
if ! cargo clippy --version > "$EV/clippy-version.txt" 2>"$EV/phases/clippy-version.stderr"; then
  echo "clippy component is absent from the rustup volume" >&2
  printf '%s\n' 2 > "$EV/phases/clippy.exit"
  printf '%s\n' 2 > "$EV/focused.exit"
  exit 2
fi
sha256sum src/*.rs tests/protocol.rs CONTRACT.md README.md BULK-QUESTIONS.md linux/a415/config/fresh-campaign.json > "$EV/postchange.sha256"
cargo clippy --locked --offline --all-targets --no-deps -- -D warnings > "$EV/phases/clippy.stdout" 2>"$EV/phases/clippy.stderr"
clippy_exit=$?
printf '%s\n' "$clippy_exit" > "$EV/phases/clippy.exit"
echo "PHASE clippy=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' 2 > "$EV/focused.exit"
  exit 2
fi
run_filter() {
  name=$1
  cargo test --locked --offline "$name" -- --test-threads=1 > "$EV/phases/test-${name}.stdout" 2>"$EV/phases/test-${name}.stderr"
  status=$?
  printf '%s\n' "$status" > "$EV/phases/test-${name}.exit"
  echo "PHASE test-$name=$status"
  if [ "$status" -ne 0 ]; then
    printf '%s\n' 1 > "$EV/focused.exit"
    exit 1
  fi
}
run_filter campaign_evidence
run_filter user_cli_phases_perform
run_filter replay_text_does_not
printf '%s\n' 0 > "$EV/focused.exit"
exit 0
