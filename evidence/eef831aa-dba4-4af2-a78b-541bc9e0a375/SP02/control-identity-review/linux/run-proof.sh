#!/bin/bash
# Inside the one owned Rust builder. Smoke before the independent test file is copied in.
# No password is placed on argv. Archives stay mounted read-only.
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

step rustup-clippy rustup component add clippy --toolchain 1.94.0-aarch64-unknown-linux-gnu
step cargo-fetch cargo fetch --locked --manifest-path /work/tree/Cargo.toml

export OO_A370_LIVE=1
export OO_A370_PG_HOST="${OO_A375_PG_HOST}"
export OO_A370_PG_PORT="${OO_A375_PG_PORT}"
export OO_A370_PG_DATABASE="${OO_A375_PG_DATABASE}"
export OO_A370_PG_PASSWORD_FILE=/secrets/login.pass
export OO_A370_PG_CA=/secrets/ca.crt
export OO_A370_PROOF=$RAW/author-proof.json
step cargo-test-author cargo test --locked --offline --manifest-path /work/tree/Cargo.toml --lib --test cli --test live -- --test-threads=1
step cargo-clippy-author cargo clippy --locked --offline --manifest-path /work/tree/Cargo.toml --all-targets -- -D warnings

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

(cd /archive && find . -type f -print0 | sort -z | xargs -0 sha256sum) > "$RAW/archive-after.sha256"
if cmp -s "$RAW/archive-before.sha256" "$RAW/archive-after.sha256"; then
  mark archive-immutable 0
else
  mark archive-immutable 1
fi
printf '%s\n' "$fail" > "$RAW/run-proof.exit"
exit "$fail"
