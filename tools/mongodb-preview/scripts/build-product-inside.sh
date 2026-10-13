#!/bin/bash
# Linux Rust 1.94 test and clippy. A drifted lock is preserved and not used.
set -euo pipefail
cd /work/tools/mongodb-preview
mkdir -p /evidence
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt
rustup component list --installed | tee /evidence/rustup-components-before.txt
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.txt 2>&1
fi
rustup component list --installed | tee /evidence/rustup-components.txt
cp Cargo.lock /evidence/Cargo.lock.before
set +e
cargo generate-lockfile > /evidence/cargo-generate-lockfile.txt 2>&1
gen_rc=$?
set -e
printf '%s\n' "$gen_rc" > /evidence/generate-lockfile.exit
if [[ "$gen_rc" -ne 0 ]]; then
  exit "$gen_rc"
fi
cp Cargo.lock /evidence/Cargo.lock.generated
set +e
bash /work/tools/mongodb-preview/scripts/check-lock.sh Cargo.lock /evidence/Cargo.lock.before > /evidence/lock-resolution.txt
lock_rc=$?
set -e
if [[ "$lock_rc" -ne 0 ]]; then
  cp Cargo.lock /evidence/Cargo.lock.rejected
  cp /evidence/Cargo.lock.before Cargo.lock
  printf 'generate-lockfile-drifted\n' | tee -a /evidence/lock-resolution.txt
  cargo build --jobs 1 --bins --lib --tests > /evidence/cargo-build-lockfix.txt 2>&1
  bash /work/tools/mongodb-preview/scripts/check-lock.sh Cargo.lock /evidence/Cargo.lock.before > /evidence/lock-resolution-after-minimal.txt
  printf 'minimal-lock-update\n' | tee -a /evidence/lock-resolution.txt
else
  printf 'generate-lockfile\n' | tee -a /evidence/lock-resolution.txt
fi
cp Cargo.lock /evidence/Cargo.lock
cargo fetch --locked > /evidence/cargo-fetch.txt 2>&1
set +e
cargo test --jobs 1 --locked --offline -- --test-threads=1 > /evidence/cargo-test.txt 2>&1
test_rc=$?
set -e
printf '%s\n' "$test_rc" > /evidence/cargo-test.exit
if [[ "$test_rc" -ne 0 ]]; then
  exit "$test_rc"
fi
set +e
cargo clippy --jobs 1 --locked --offline --all-targets -- -D warnings > /evidence/cargo-clippy.txt 2>&1
clippy_rc=$?
set -e
printf '%s\n' "$clippy_rc" > /evidence/cargo-clippy.exit
if [[ "$clippy_rc" -ne 0 ]]; then
  exit "$clippy_rc"
fi
set +e
cargo run --jobs 1 --locked --offline --quiet -- \
  --customer examples/customer.shipments.json \
  --source-id oo_w141 \
  --secret-ref secret://oo-dl01w1-41/reader \
  --driver-digest 622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8 \
  --permission readWrite \
  --budget-unit Rows \
  --budget-limit 16 \
  --out /evidence/decision-deny.json \
  --gap-out /evidence/gap-deny.json
deny_rc=$?
set -e
printf '%s\n' "$deny_rc" > /evidence/cli-deny.exit
if [[ "$deny_rc" -ne 3 ]]; then
  exit "$deny_rc"
fi
grep -q '"admitted": false' /evidence/decision-deny.json
grep -q 'PERMISSION_NOT_READONLY' /evidence/decision-deny.json
grep -q '"import_started": false' /evidence/gap-deny.json
set +e
cargo run --jobs 1 --locked --offline --quiet -- \
  --customer examples/customer.shipments.json \
  --source-id oo_w141 \
  --secret-ref secret://oo-dl01w1-41/reader \
  --driver-digest 622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8 \
  --permission read \
  --budget-unit Rows \
  --budget-limit 16 \
  --out /evidence/decision.json \
  --gap-out /evidence/gap.json
cli_rc=$?
set -e
printf '%s\n' "$cli_rc" > /evidence/cli.exit
if [[ "$cli_rc" -ne 2 ]]; then
  exit "$cli_rc"
fi
grep -q '"admitted": true' /evidence/decision.json
grep -q '"engine": "mongodb"' /evidence/gap.json
grep -q '"import_started": false' /evidence/gap.json
grep -q '"shared_probe": "not_run"' /evidence/gap.json
grep -q '"pg_budget_protocol_used": false' /evidence/gap.json
grep -q '"model_called": false' /evidence/gap.json
if grep -E 'mongodb://|CANARY-SECRET|postgres-native' /evidence/decision.json /evidence/decision-deny.json; then
  echo "secret or retag in decision" >&2
  exit 1
fi
if grep -E '"engine": "postgres-native"|"engine": "mysql"' /evidence/gap.json /evidence/gap-deny.json; then
  echo "engine retag in gap" >&2
  exit 1
fi
grep -q 'connectionStatus' /evidence/decision.json
grep -q 'connectionStatus' /evidence/decision-deny.json
grep -q 'db.grantRolesToUser' /evidence/decision.json
if grep -q 'PASSWORD' /evidence/decision.json /evidence/decision-deny.json; then
  echo "sql password checklist on mongodb" >&2
  exit 1
fi
echo "product decision check ok"
if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /work/tools/mongodb-preview /evidence
fi
