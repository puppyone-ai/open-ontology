#!/bin/bash
# Focused cdc-offset-store tests and clippy on the fixed archive plus this overlay.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/linux/impl-1/inside.sh
# Change: one path package, two private databases, live_chain only. Restart is a later container.
set -u
echo "oo_task=sp04-cdc-offset-store-impl-4"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a310"
echo "oo_fixed=cab5e8e219d1924070df7336f5ab5d4ce7cf40dc"
echo "oo_platform=linux/arm64"
echo "phase=impl-4"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export CDC_OFFSET_HOST=oo-a310-pg
export CDC_OFFSET_PORT=5432
export CDC_OFFSET_SERVER_NAME=oo-a310-pg
export CDC_OFFSET_ADMIN_PASS_FILE=/secrets/admin.pass
export CDC_OFFSET_CA_FILE=/secrets/ca.crt
export CDC_OFFSET_MIGRATIONS=/src/crates/oo-pipeline/migrations
export CDC_OFFSET_COUNTS=/cargo-home/work/restart-counts.json
export CDC_OFFSET_SECRET_DIR=/cargo-home/work/secrets
mkdir -p /evidence/phases /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

record() {
  mkdir -p /evidence/phases
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/smoke.exit
    exit "$2"
  fi
}

if [ ! -f /src/tools/cdc-offset-store/src/lib.rs ]; then
  echo "archive missing cdc-offset-store" >&2
  record source 2
fi
if [ ! -f /src/crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql ]; then
  echo "archive missing target receipt SQL" >&2
  record source 2
fi
if [ ! -f /src/Cargo.toml ] || [ ! -f /src/Cargo.lock ]; then
  echo "archive missing the root manifest or lock" >&2
  record source 2
fi
cp -R /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt
sha256sum Cargo.lock | awk '{print $1}' > /evidence/lock-before.sha256
rm -f tools/cdc-offset-store/Cargo.lock
cp Cargo.lock tools/cdc-offset-store/Cargo.lock

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

mkdir -p /cargo-home/bin
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?

echo "=== cargo fetch cdc-offset-store ==="
set +e
cargo fetch --manifest-path tools/cdc-offset-store/Cargo.toml > /evidence/fetch.log 2>&1
fetch_exit=$?
set -e
if [ -f tools/cdc-offset-store/Cargo.lock ]; then
  cp tools/cdc-offset-store/Cargo.lock /evidence/cdc-offset-store-Cargo.lock
fi
record fetch "$fetch_exit"

echo "=== lock subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/cdc-offset-store/Cargo.lock \
  > /evidence/lock-subset.txt 2> /evidence/lock-subset.err
subset_exit=$?
set -e
record lock-subset "$subset_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
else
  printf '%s\n' "already-installed" > /evidence/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch cdc-offset-store --locked --offline ==="
set +e
cargo fetch --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline \
  > /evidence/fetch-offline.log 2>&1
offline_exit=$?
set -e
record fetch-offline "$offline_exit"

echo "=== cargo test contract ==="
set +e
cargo test --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline --test contract \
  -- --test-threads=1 > /evidence/contract.log 2>&1
contract_exit=$?
set -e
printf '%s\n' "$contract_exit" > /evidence/phases/contract.exit
echo "phase_exit=$contract_exit phase=contract"

echo "=== cargo test statements ==="
set +e
cargo test --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline --test live \
  -- --test-threads=1 --exact statements_keep_dollar_blocks > /evidence/statements.log 2>&1
statements_exit=$?
set -e
printf '%s\n' "$statements_exit" > /evidence/phases/statements.exit
echo "phase_exit=$statements_exit phase=statements"

echo "=== cargo test live_chain ==="
set +e
cargo test --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline --test live \
  -- --test-threads=1 --exact live_chain > /evidence/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
echo "phase_exit=$test_exit phase=test"

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.log 2>&1
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
echo "phase_exit=$clippy_exit phase=clippy"

sha256sum Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
before=$(cat /evidence/lock-before.sha256)
after=$(cat /evidence/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "root lock changed before=$before after=$after" > /evidence/lock-hash.txt
  printf '%s\n' 1 > /evidence/phases/lock-hash.exit
  printf '%s\n' 1 > /evidence/smoke.exit
  exit 1
fi
echo "unchanged $after" > /evidence/lock-hash.txt
printf '%s\n' 0 > /evidence/phases/lock-hash.exit
fail=0
for code in "$contract_exit" "$statements_exit" "$test_exit" "$clippy_exit"; do
  if [ "$code" -ne 0 ]; then
    fail=$code
    break
  fi
done
printf '%s\n' "$fail" > /evidence/smoke.exit
if [ "$fail" -ne 0 ]; then
  exit "$fail"
fi
echo "proof=ok"
exit 0
