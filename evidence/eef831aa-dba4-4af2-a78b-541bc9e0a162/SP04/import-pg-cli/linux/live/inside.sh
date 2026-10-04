#!/bin/bash
# Ignored live tests, one after another, on the private PostgreSQL alias.
set -u
echo "oo_task=import-pg-cli-live"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
unset RUST_LOG || true
note() {
  printf '%s\n' "$2" > "/evidence/$1.exit"
  echo "phase=$1 exit=$2"
}
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/inside.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2; printf '%s\n' 2 > /evidence/inside.exit; exit 2 ;;
esac
cd /src
set +e
echo "=== prepare_snapshot live ==="
cargo test -p oo-connectors --test pg_prepare_snapshot_live --locked --offline -- --ignored --test-threads=1 prepare_snapshot_reads_the_public_session \
  > /evidence/prepare.stdout 2> /evidence/prepare.stderr
note prepare $?
echo "=== import-pg binary live ==="
cargo test -p oo-pipeline --test import_pg_cli_live --locked --offline -- --ignored --test-threads=1 import_pg_binary_on_private_postgres \
  > /evidence/import.stdout 2> /evidence/import.stderr
note import $?
fail=0
for phase in prepare import; do
  code=$(cat "/evidence/${phase}.exit")
  if [ "$code" -ne 0 ]; then
    fail=$code
  fi
done
printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
