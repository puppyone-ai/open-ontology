#!/bin/bash
# Second owned builder pass. Author tests already passed. This copies the fixed oracle only.
# The cargo volume is reused. PostgreSQL is the one already running. No second database.
set -u
export PATH=/usr/local/cargo/bin:/usr/bin:/bin
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/work/target
RAW=/evidence/linux/raw
mkdir -p "$RAW" /evidence/responses/actual
fail=0

mark() {
  printf '%s\n' "$2" > "$RAW/$1.exit"
  if [ "$2" -ne 0 ]; then
    fail=1
  fi
}

step() {
  name=$1
  shift
  set +e
  "$@" > "$RAW/$name.log" 2>&1
  code=$?
  set -e
  mark "$name" "$code"
  return 0
}

set -e
step rustup-clippy rustup component add clippy --toolchain 1.94.0-aarch64-unknown-linux-gnu
cp /evidence/oracle/independent.rs /work/tree/tests/independent.rs
export OO_A375_ORACLE=1
export OO_A375_PG_PASSWORD_FILE=/secrets/login.pass
export OO_A375_PG_CA=/secrets/ca.crt
export OO_A375_PROOF=$RAW/independent-proof.json
export OO_A375_RESPONSES=/evidence/responses/actual
export OO_A375_TRUNCATE=$RAW/truncate-note.txt
export OO_A375_CHILD_LOCK=/archive/tools/control-identity/Cargo.lock
export OO_A375_ROOT_LOCK=/archive/Cargo.lock
export OO_A375_SCAN_DIR=/evidence
step cargo-test-independent cargo test --locked --offline --manifest-path /work/tree/Cargo.toml --test independent -- --test-threads=1
step cargo-clippy-independent cargo clippy --locked --offline --manifest-path /work/tree/Cargo.toml --test independent -- -D warnings
(cd /archive && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$RAW/archive-after-rerun.sha256"
if cmp -s "$RAW/archive-before.sha256" "$RAW/archive-after-rerun.sha256"; then
  mark archive-immutable-rerun 0
else
  mark archive-immutable-rerun 1
fi
printf '%s\n' "$fail" > "$RAW/rerun-independent.exit"
exit "$fail"
