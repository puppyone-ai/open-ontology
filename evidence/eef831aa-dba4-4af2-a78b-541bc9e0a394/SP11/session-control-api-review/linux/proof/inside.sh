#!/bin/bash
# Compile the immutable archive plus the a394 oracle. Locked. No cargo update.
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
{
  echo "archive_extra"
  diff -rq /archive/tools/session-control-api /src/tools/session-control-api || true
} > "$EV/archive-diff.txt"
if ! rustup component list --installed | grep -q '^clippy-'; then
  rustup component add clippy > "$EV/clippy-component.stdout" 2> "$EV/clippy-component.stderr"
  printf '%s\n' $? > "$EV/clippy-component.exit"
else
  printf '%s\n' present > "$EV/clippy-component.stdout"
  printf '%s\n' 0 > "$EV/clippy-component.exit"
fi
if [ -f Cargo.lock ]; then
  printf '%s\n' lock_present > "$EV/lock.stdout"
  printf '%s\n' 0 > "$EV/lock.exit"
else
  printf '%s\n' lock_missing > "$EV/lock.stdout"
  printf '%s\n' 1 > "$EV/lock.exit"
  exit 1
fi
cargo fetch --locked > "$EV/fetch.stdout" 2> "$EV/fetch.stderr"
printf '%s\n' $? > "$EV/fetch.exit"
if [ "$(cat "$EV/fetch.exit")" != 0 ]; then
  exit 1
fi
set +e
cargo test --locked --offline --lib --bins -- --test-threads=1 > "$EV/test-lib.stdout" 2> "$EV/test-lib.stderr"
printf '%s\n' $? > "$EV/test-lib.exit"
cargo test --locked --offline --test contract -- --test-threads=1 > "$EV/test-contract.stdout" 2> "$EV/test-contract.stderr"
printf '%s\n' $? > "$EV/test-contract.exit"
cargo test --locked --offline --test lock_subset -- --test-threads=1 > "$EV/test-lock.stdout" 2> "$EV/test-lock.stderr"
printf '%s\n' $? > "$EV/test-lock.exit"
cargo test --locked --offline --test live -- --test-threads=1 > "$EV/test-live.stdout" 2> "$EV/test-live.stderr"
printf '%s\n' $? > "$EV/test-live.exit"
cargo test --locked --offline --test a394_oracle -- --test-threads=1 > "$EV/test-oracle.stdout" 2> "$EV/test-oracle.stderr"
printf '%s\n' $? > "$EV/test-oracle.exit"
cargo clippy --locked --offline --all-targets --keep-going -- -D warnings > "$EV/clippy.stdout" 2> "$EV/clippy.stderr"
printf '%s\n' $? > "$EV/clippy.exit"
set -e
fail=0
for name in test-lib test-contract test-lock test-live test-oracle clippy; do
  if [ "$(cat "$EV/${name}.exit")" != 0 ]; then
    fail=1
  fi
done
exit "$fail"
