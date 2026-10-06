#!/bin/bash
# Focused business-calendar tests and clippy.
# Fixed archive 0bcd4944 plus the tool overlay. Copy uses cp -R.
# No cargo update. No PostgreSQL. No apt.
set -u
echo "oo_task=sp08-business-calendar-impl-1"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a269"
echo "oo_fixed=0bcd494457091d4b55c82fc6b9922b5bc5dc44c4"
echo "oo_platform=linux/arm64"
echo "phase=impl-1"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/impl-1/phases /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src

record() {
  mkdir -p /evidence/linux/impl-1/phases
  printf '%s\n' "$2" > "/evidence/linux/impl-1/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/impl-1/smoke.exit
    exit "$2"
  fi
}

if [ -e /src/tools/business-calendar ]; then
  echo "fixed archive already contains business-calendar" >&2
  record source 2
fi
if [ ! -f /src/examples/schema/calendar.yaml ]; then
  echo "fixed archive missing examples/schema/calendar.yaml" >&2
  record source 2
fi
printf '%s\n' "calendar_absent_before_overlay" > /evidence/linux/impl-1/overlay-before.txt
cp -R /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/linux/impl-1/toolchain-aside.txt
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/impl-1/lock-before.sha256
printf '%s\n' "no" > /evidence/linux/impl-1/cargo-update-invoked.txt

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/impl-1/rustc-version.txt
cargo -V | tee /evidence/linux/impl-1/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?

rm -rf tools/business-calendar
mkdir -p tools
cp -R /calendar/. tools/business-calendar/
rm -rf tools/business-calendar/target
if [ -f tools/business-calendar/Cargo.lock ]; then
  echo "overlay had a lock; replacing it from the root lock" > /evidence/linux/impl-1/lock-note.txt
  rm -f tools/business-calendar/Cargo.lock
fi
cp Cargo.lock tools/business-calendar/Cargo.lock
printf '%s\n' "calendar_overlaid" > /evidence/linux/impl-1/overlay-after.txt
record source 0

echo "=== cargo fetch calendar ==="
set +e
cargo fetch --manifest-path tools/business-calendar/Cargo.toml > /evidence/linux/impl-1/fetch.log 2>&1
fetch_exit=$?
set -e
if [ -f tools/business-calendar/Cargo.lock ]; then
  cp tools/business-calendar/Cargo.lock /evidence/linux/impl-1/business-calendar-Cargo.lock
fi
record fetch "$fetch_exit"

echo "=== lock subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/business-calendar/Cargo.lock \
  > /evidence/linux/impl-1/lock-subset.txt 2> /evidence/linux/impl-1/lock-subset.err
subset_exit=$?
set -e
record lock-subset "$subset_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/impl-1/clippy-component.log 2>&1
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
else
  printf '%s\n' "already-installed" > /evidence/linux/impl-1/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch calendar --locked --offline ==="
set +e
cargo fetch --manifest-path tools/business-calendar/Cargo.toml --locked --offline \
  > /evidence/linux/impl-1/fetch-offline.log 2>&1
offline_exit=$?
set -e
record fetch-offline "$offline_exit"

echo "=== cargo test ==="
set +e
cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline \
  -- --test-threads=1 > /evidence/linux/impl-1/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/linux/impl-1/phases/test.exit
echo "phase_exit=$test_exit phase=test"

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path tools/business-calendar/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/impl-1/clippy.log 2>&1
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/linux/impl-1/phases/clippy.exit
echo "phase_exit=$clippy_exit phase=clippy"
if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' "$test_exit" > /evidence/linux/impl-1/smoke.exit
  exit "$test_exit"
fi
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' "$clippy_exit" > /evidence/linux/impl-1/smoke.exit
  exit "$clippy_exit"
fi

sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/impl-1/lock-after.sha256
before=$(cat /evidence/linux/impl-1/lock-before.sha256)
after=$(cat /evidence/linux/impl-1/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "root lock changed before=$before after=$after" > /evidence/linux/impl-1/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/impl-1/lock-hash.txt
record lock-hash 0
printf '%s\n' 0 > /evidence/linux/impl-1/smoke.exit
echo "proof=ok"
exit 0
