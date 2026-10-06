#!/bin/bash
# Read the calendar view after the host restarted PostgreSQL.
# Uses the impl-2 work tree and cargo cache. Does not fetch and does not update.
set -u
echo "oo_task=sp08-calendar-store-restart"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a289"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/phases
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt
if [ ! -f /cargo-home/work/src/tools/calendar-store/Cargo.lock ]; then
  echo "impl work tree is missing" >&2
  printf '%s\n' 2 > /evidence/smoke.exit
  exit 2
fi
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  echo "toolchain file returned" >&2
  printf '%s\n' 2 > /evidence/smoke.exit
  exit 2
fi
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/smoke.exit; exit 2 ;;
esac
set +e
cargo test --manifest-path tools/calendar-store/Cargo.toml --locked --offline --test live -- \
  restart_snapshot_persists --test-threads=1 > /evidence/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
printf '%s\n' "$test_exit" > /evidence/smoke.exit
echo "phase_exit=$test_exit phase=restart-test"
exit "$test_exit"
