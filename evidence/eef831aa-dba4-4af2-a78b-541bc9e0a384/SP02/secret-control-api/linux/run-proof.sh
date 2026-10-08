#!/bin/bash
# Inside the one owned Rust builder. Focused tests and Clippy. No password on argv.
set -u
export PATH=/usr/local/cargo/bin:/usr/bin:/bin
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
RAW=/evidence/linux/raw
mkdir -p "$RAW" /work/identity /work/secret /state
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
command -v openssl >/dev/null
printf '%s\n' "$(openssl version)" > "$RAW/openssl-version.txt"
phase=${OO_A384_PHASE:?phase}
export OO_A384_LIVE=1
export OO_A384_PG_HOST=oo-a384-pg
export OO_A384_PG_PORT=5432
export OO_A384_PG_PASSWORD_FILE=/secrets/login.pass
export OO_A384_PG_CA=/secrets/ca.crt
export OO_A384_STATE=/state
export OO_A384_MASTER_KEY=/state/master.key
export OO_A384_IDENTITY_BIN=/work/identity/debug/control-identity
export OO_A384_PROOF=/evidence/linux/raw/author-proof.json

if [ "$phase" = "write" ]; then
  step rustc-version rustc --version --verbose
  step cargo-version cargo --version
  step rustup-clippy rustup component add clippy --toolchain 1.94.0-aarch64-unknown-linux-gnu
  step cargo-fetch-identity cargo fetch --locked --manifest-path /identity/Cargo.toml
  step cargo-fetch-secret cargo fetch --locked --manifest-path /src/Cargo.toml
  printf '%s\n' 'cargo-update-invoked=no' > "$RAW/cargo-update.txt"
  step cargo-build-identity env CARGO_TARGET_DIR=/work/identity cargo build --locked --offline --manifest-path /identity/Cargo.toml --bin control-identity
  step cargo-clippy env CARGO_TARGET_DIR=/work/secret cargo clippy --locked --offline --manifest-path /src/Cargo.toml --all-targets -- -D warnings
  if [ "$(cat "$RAW/cargo-clippy.exit")" -eq 0 ]; then
    step cargo-test-focused env CARGO_TARGET_DIR=/work/secret cargo test --locked --offline --manifest-path /src/Cargo.toml -- --test-threads=1 --nocapture
  else
    printf '%s\n' 'skipped because clippy failed' > "$RAW/cargo-test-focused.log"
    mark cargo-test-focused 1
  fi
  if [ -f /state/phase.txt ] && grep -q '^live_secret=write_done$' /state/phase.txt; then
    mark phase-marker 0
  else
    printf '%s\n' 'write marker missing' > "$RAW/phase-marker.log"
    mark phase-marker 1
  fi
elif [ "$phase" = "restart" ]; then
  step cargo-test-restart env CARGO_TARGET_DIR=/work/secret cargo test --locked --offline --manifest-path /src/Cargo.toml --test live -- --test-threads=1 --nocapture
  if [ -f /state/phase.txt ] && grep -q '^live_secret=done$' /state/phase.txt; then
    mark phase-marker 0
  else
    printf '%s\n' 'restart marker missing' > "$RAW/phase-marker.log"
    mark phase-marker 1
  fi
else
  printf '%s\n' 'phase must be write or restart' > "$RAW/phase.log"
  mark phase 1
fi

printf '%s\n' "$fail" > "$RAW/run-proof.exit"
exit "$fail"
