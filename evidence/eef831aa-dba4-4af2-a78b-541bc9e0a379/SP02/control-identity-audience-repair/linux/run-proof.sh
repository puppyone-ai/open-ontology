#!/bin/bash
# Inside the one owned Rust builder. Focused audience tests and Clippy. No password on argv.
set -u
export PATH=/usr/local/cargo/bin:/usr/bin:/bin
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/work/target
RAW=/evidence/linux/raw
mkdir -p "$RAW" /work
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
step cargo-fetch cargo fetch --locked --manifest-path /src/Cargo.toml
export OO_A370_LIVE=1
export OO_A370_PG_HOST=oo-a379-pg
export OO_A370_PG_PORT=5432
export OO_A370_PG_DATABASE=identity
export OO_A370_PG_PASSWORD_FILE=/secrets/login.pass
export OO_A370_PG_CA=/secrets/ca.crt
export OO_A370_PROOF=$RAW/author-proof.json
step cargo-test-focused cargo test --locked --offline --manifest-path /src/Cargo.toml --lib --test cli --test live -- --test-threads=1
step cargo-clippy cargo clippy --locked --offline --manifest-path /src/Cargo.toml --all-targets -- -D warnings
printf '%s\n' "$fail" > "$RAW/run-proof.exit"
exit "$fail"
