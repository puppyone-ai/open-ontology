#!/bin/bash
# Re-read the independent mirror after a postmaster restart. No new writes except replay.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a321/SP04/cdc-offset-store-review/linux/overlay/inside.sh
# Change: CDC_OFFSET_RESTART=1 and only independent_restart.
set -u
echo "oo_task=sp04-cdc-offset-store-review-restart"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a321"
echo "oo_fixed=52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96"
echo "phase=restart"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export CDC_OFFSET_HOST=oo-a321-pg
export CDC_OFFSET_PORT=5432
export CDC_OFFSET_SERVER_NAME=oo-a321-pg
export CDC_OFFSET_ADMIN_PASS_FILE=/secrets/admin.pass
export CDC_OFFSET_CA_FILE=/secrets/ca.crt
export CDC_OFFSET_MIGRATIONS=/src/crates/oo-pipeline/migrations
export CDC_OFFSET_COUNTS=/cargo-home/work/independent-counts.json
export CDC_OFFSET_SECRET_DIR=/cargo-home/work/secrets
export CDC_OFFSET_RESTART=1
mkdir -p /evidence/phases
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt
if [ ! -f /cargo-home/work/independent-counts.json ]; then
  echo "independent counts are missing" >&2
  printf '%s\n' 2 > /evidence/phases/test.exit
  printf '%s\n' 2 > /evidence/smoke.exit
  exit 2
fi
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -R /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/smoke.exit; exit 2 ;;
esac
echo "=== cargo test independent_restart ==="
set +e
cargo test --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline --test review_a321 \
  -- --test-threads=1 --exact independent_restart > /evidence/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
echo "phase_exit=$test_exit phase=test"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
  component_exit=$?
  set -e
  printf '%s\n' "$component_exit" > /evidence/phases/clippy-component.exit
  echo "phase_exit=$component_exit phase=clippy-component"
  if [ "$component_exit" -ne 0 ]; then
    printf '%s\n' "$component_exit" > /evidence/smoke.exit
    exit "$component_exit"
  fi
else
  printf '%s\n' "already-installed" > /evidence/clippy-component.log
  printf '%s\n' 0 > /evidence/phases/clippy-component.exit
fi
echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path tools/cdc-offset-store/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.log 2>&1
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
echo "phase_exit=$clippy_exit phase=clippy"
fail=0
for code in "$test_exit" "$clippy_exit"; do
  if [ "$code" -ne 0 ]; then
    fail=$code
    break
  fi
done
printf '%s\n' "$fail" > /evidence/smoke.exit
if [ "$fail" -ne 0 ]; then
  exit "$fail"
fi
echo "proof=ok independent_restart"
exit 0
