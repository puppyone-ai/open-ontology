#!/bin/bash
# Post-rollback gate: clippy, library tests, then both ignored live tests.
set -u
echo "oo_task=import-pg-cli-live-6"
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
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
  note rustup-clippy $?
  if [ "$(cat /evidence/rustup-clippy.exit)" -ne 0 ]; then
    printf '%s\n' "$(cat /evidence/rustup-clippy.exit)" > /evidence/inside.exit
    exit "$(cat /evidence/rustup-clippy.exit)"
  fi
else
  note rustup-clippy 0
fi
cd /src
set +e
echo "=== connectors lib ==="
cargo test -p oo-connectors --lib --locked --offline > /evidence/connectors-lib.stdout 2> /evidence/connectors-lib.stderr
note connectors-lib $?
echo "=== connectors clippy ==="
cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings > /evidence/connectors-clippy.stdout 2> /evidence/connectors-clippy.stderr
note connectors-clippy $?
echo "=== pipeline lib ==="
cargo test -p oo-pipeline --lib --locked --offline > /evidence/pipeline-lib.stdout 2> /evidence/pipeline-lib.stderr
note pipeline-lib $?
echo "=== pipeline clippy ==="
cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings > /evidence/pipeline-clippy.stdout 2> /evidence/pipeline-clippy.stderr
note pipeline-clippy $?
echo "=== import-pg cli errors ==="
cargo test -p oo-pipeline --test import_pg_cli_live --locked --offline > /evidence/cli-errors.stdout 2> /evidence/cli-errors.stderr
note cli-errors $?
echo "=== prepare_snapshot live ==="
cargo test -p oo-connectors --test pg_prepare_snapshot_live --locked --offline -- --ignored --test-threads=1 prepare_snapshot_reads_the_public_session \
  > /evidence/prepare.stdout 2> /evidence/prepare.stderr
note prepare $?
echo "=== import-pg binary live ==="
cargo test -p oo-pipeline --test import_pg_cli_live --locked --offline -- --ignored --test-threads=1 import_pg_binary_on_private_postgres \
  > /evidence/import.stdout 2> /evidence/import.stderr
note import $?
fail=0
for phase in connectors-lib connectors-clippy pipeline-lib pipeline-clippy cli-errors prepare import; do
  code=$(cat "/evidence/${phase}.exit")
  if [ "$code" -ne 0 ]; then
    fail=$code
  fi
done
printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
