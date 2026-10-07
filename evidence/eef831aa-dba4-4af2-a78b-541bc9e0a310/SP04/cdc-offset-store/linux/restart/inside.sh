#!/bin/bash
# Replay the persisted control mirror after the PostgreSQL process restart.
# The work tree and target dir stay on the task volumes. No fetch.
set -u
echo "oo_task=sp04-cdc-offset-store-restart"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a310"
echo "phase=restart"
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
export CDC_OFFSET_RESTART=1
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt
mkdir -p /evidence/phases
cd /cargo-home/work/src
got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/smoke.exit; exit 2 ;;
esac
if [ ! -f /cargo-home/work/restart-counts.json ]; then
  echo "restart counts are missing" >&2
  printf '%s\n' 2 > /evidence/smoke.exit
  exit 2
fi
set +e
cargo test --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline --test live \
  -- --test-threads=1 --exact restart_counts > /evidence/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
echo "phase_exit=$test_exit phase=test"
printf '%s\n' "$test_exit" > /evidence/smoke.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
grep -q 'restart_counts_ok' /evidence/test.log
echo "proof=ok"
exit 0
