#!/bin/bash
# Inside the one owned Rust builder. Smoke before the audience oracle is copied in.
# No password is placed on argv. The archive stays mounted read-only.
set -u
export PATH=/usr/local/cargo/bin:/usr/bin:/bin
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/work/target
RAW=/evidence/linux/raw
mkdir -p "$RAW" /evidence/responses/schema /evidence/responses/actual /work
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
  printf '%s\n' "$name" > "$RAW/status.txt"
  set +e
  "$@" > "$RAW/$name.log" 2>&1
  code=$?
  set -e
  mark "$name" "$code"
  return 0
}

set -e
{
  echo "uname=$(uname -m)"
  rustc --version
  rustc --version --verbose
  cargo --version
  openssl version
} > "$RAW/smoke.log" 2>&1
smoke=0
grep -q 'aarch64' "$RAW/smoke.log" || smoke=1
grep -q 'rustc 1.94.0' "$RAW/smoke.log" || smoke=1
mark smoke "$smoke"
if [ "$smoke" -ne 0 ]; then
  printf '%s\n' "$fail" > "$RAW/run-proof.exit"
  exit "$fail"
fi

(cd /archive && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$RAW/archive-before.sha256"
cp -a /archive/tools/control-identity/schema/. /evidence/responses/schema/
cp -a /archive/tools/control-identity/openapi/control-identity.openapi.yaml /evidence/responses/schema/control-identity.openapi.yaml

rm -rf /work/tree
mkdir -p /work/tree
cp -a /archive/tools/control-identity/. /work/tree/
test ! -e /work/tree/tests/audience.rs

step rustup-clippy rustup component add clippy --toolchain 1.94.0-aarch64-unknown-linux-gnu
step cargo-fetch cargo fetch --locked --manifest-path /work/tree/Cargo.toml
step cargo-test-lib cargo test --locked --offline --manifest-path /work/tree/Cargo.toml --lib -- --test-threads=1
step cargo-clippy cargo clippy --locked --offline --manifest-path /work/tree/Cargo.toml --all-targets -- -D warnings

cp /evidence/oracle/audience.rs /work/tree/tests/audience.rs
export OO_A375C_ORACLE=1
export OO_A375C_PG_PASSWORD_FILE=/secrets/login.pass
export OO_A375C_PG_CA=/secrets/ca.crt
export OO_A375C_PROOF=$RAW/audience-proof.json
export OO_A375C_RESPONSES=/evidence/responses/actual
step cargo-test-audience cargo test --locked --offline --manifest-path /work/tree/Cargo.toml --test audience -- --test-threads=1
step cargo-clippy-audience cargo clippy --locked --offline --manifest-path /work/tree/Cargo.toml --test audience -- -D warnings

(cd /archive && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$RAW/archive-after.sha256"
if cmp -s "$RAW/archive-before.sha256" "$RAW/archive-after.sha256"; then
  mark archive-immutable 0
else
  mark archive-immutable 1
fi
printf 'done\n' > "$RAW/status.txt"
printf '%s\n' "$fail" > "$RAW/run-proof.exit"
exit "$fail"
