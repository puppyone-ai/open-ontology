#!/bin/bash
# Compile and test session-control-api only. One new-workspace lock, then --locked --offline.
# Does not update the root lock and does not run cargo update.
set -u
cd /src/tools/session-control-api
EV=/evidence
mkdir -p "$EV" /target/tmp
export TMPDIR=/target/tmp
rustc --version > "$EV/version.txt" 2>&1
printf '%s\n' $? > "$EV/version.exit"
if ! rustup component list --installed | grep -q '^clippy-'; then
  rustup component add clippy > "$EV/clippy-component.stdout" 2> "$EV/clippy-component.stderr"
  printf '%s\n' $? > "$EV/clippy-component.exit"
else
  printf '%s\n' present > "$EV/clippy-component.stdout"
  printf '%s\n' 0 > "$EV/clippy-component.exit"
fi
if [ ! -f Cargo.lock ]; then
  cargo generate-lockfile --offline > "$EV/lock-offline.stdout" 2> "$EV/lock-offline.stderr"
  printf '%s\n' $? > "$EV/lock-offline.exit"
  if [ "$(cat "$EV/lock-offline.exit")" != 0 ]; then
    cargo generate-lockfile > "$EV/lock.stdout" 2> "$EV/lock.stderr"
    printf '%s\n' $? > "$EV/lock.exit"
  else
    printf '%s\n' "offline" > "$EV/lock.stdout"
    printf '%s\n' 0 > "$EV/lock.exit"
  fi
else
  printf '%s\n' "lock_present" > "$EV/lock.stdout"
  printf '%s\n' 0 > "$EV/lock.exit"
fi
if [ "$(cat "$EV/lock.exit")" != 0 ]; then
  exit 1
fi
cargo fetch --locked > "$EV/fetch.stdout" 2> "$EV/fetch.stderr"
printf '%s\n' $? > "$EV/fetch.exit"
if [ "$(cat "$EV/fetch.exit")" != 0 ]; then
  exit 1
fi
cargo test --locked --offline --all-targets -- --test-threads=1 > "$EV/test.stdout" 2> "$EV/test.stderr"
printf '%s\n' $? > "$EV/test.exit"
cargo clippy --locked --offline --all-targets --keep-going -- -D warnings > "$EV/clippy.stdout" 2> "$EV/clippy.stderr"
printf '%s\n' $? > "$EV/clippy.exit"
test_exit=$(cat "$EV/test.exit")
clippy_exit=$(cat "$EV/clippy.exit")
if [ "$test_exit" != 0 ] || [ "$clippy_exit" != 0 ]; then
  exit 1
fi
exit 0
