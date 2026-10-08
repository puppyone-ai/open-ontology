#!/bin/bash
# Clippy only. The green test campaign is not repeated.
set -u
EV=/evidence
mkdir -p "$EV" /target/tmp /src
export TMPDIR=/target/tmp
rm -rf /src/tools
cp -a /archive/tools /src/tools
cp -a /archive/Cargo.lock /root.lock
cp /oracle/a394_oracle.rs /src/tools/session-control-api/tests/a394_oracle.rs
cd /src/tools/session-control-api
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
  printf '%s\n' 1 > "$EV/lock.exit"
  exit 1
fi
printf '%s\n' 0 > "$EV/lock.exit"
cargo clippy --locked --offline --all-targets --keep-going -- -D warnings > "$EV/clippy.stdout" 2> "$EV/clippy.stderr"
printf '%s\n' $? > "$EV/clippy.exit"
exit "$(cat "$EV/clippy.exit")"
