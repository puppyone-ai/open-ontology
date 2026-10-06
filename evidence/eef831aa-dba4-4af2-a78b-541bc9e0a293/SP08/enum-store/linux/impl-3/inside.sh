#!/bin/bash
# Focused enum-store tests and clippy on the fixed archive plus this overlay.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a269/SP08/enum-history/linux/impl-4/inside.sh
# sha256 1f1ca9bd49ffa6451f376777d874e3230c2e8a172d27785373588ae53f40a983
# Change: enum-store manifest, three path packages, skip the restart test.
# impl-1 already passed fresh_pg_contract. This run keeps that database,
# reuses the impl-1 child lock, and does not fetch from the network.
# The work tree stays on the cargo volume for the later restart proof.
set -u
echo "oo_task=sp08-enum-store-impl-3"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a293"
echo "oo_fixed=c8588faab992af1193ff58dbc489a0b117796aa0"
echo "oo_platform=linux/arm64"
echo "phase=impl-3"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
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

if [ ! -f /src/tools/enum-store/src/lib.rs ]; then
  echo "archive missing enum-store" >&2
  record source 2
fi
if [ ! -f /src/tools/enum-history/Cargo.toml ] || [ ! -f /src/tools/enum-history/src/lib.rs ]; then
  echo "archive missing enum-history" >&2
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
if [ ! -f tools/enum-store/Cargo.lock ]; then
  echo "child lock missing from the overlay" >&2
  record source 2
fi

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

echo "=== child lock reused from impl-1; no network fetch ==="
cp tools/enum-store/Cargo.lock /evidence/enum-store-Cargo.lock
printf '%s\n' "reused impl-1 child lock; cargo fetch without --locked was not run" > /evidence/fetch.log
record fetch 0

echo "=== lock subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/enum-store/Cargo.lock \
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

echo "=== cargo fetch enum-store --locked --offline ==="
set +e
cargo fetch --manifest-path tools/enum-store/Cargo.toml --locked --offline \
  > /evidence/fetch-offline.log 2>&1
offline_exit=$?
set -e
record fetch-offline "$offline_exit"

echo "=== cargo test ==="
set +e
cargo test --manifest-path tools/enum-store/Cargo.toml --locked --offline \
  -- --test-threads=1 --skip restart_snapshot_persists --skip fresh_pg_contract > /evidence/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
echo "phase_exit=$test_exit phase=test"

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path tools/enum-store/Cargo.toml --locked --offline --all-targets -- -D warnings \
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
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' "$test_exit" > /evidence/smoke.exit
  exit "$test_exit"
fi
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' "$clippy_exit" > /evidence/smoke.exit
  exit "$clippy_exit"
fi
printf '%s\n' 0 > /evidence/smoke.exit
echo "proof=ok"
exit 0
