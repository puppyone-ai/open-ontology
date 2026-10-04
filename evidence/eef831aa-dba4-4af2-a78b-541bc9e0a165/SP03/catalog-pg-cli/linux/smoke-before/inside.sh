#!/bin/bash
# Pre-edit Linux 1.94 focused pipeline smoke. Archive of HEAD. No PostgreSQL.
set -u
echo "oo_task=catalog-pg-cli-smoke-before"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=head_archive_before_edit"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
unset RUST_LOG || true
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line"
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
rustc -Vv > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt

cd /src
fail=0
note() {
  name=$1
  code=$2
  printf '%s\n' "$code" > "/evidence/${name}.exit"
  echo "phase_exit=$code phase=$name"
  if [ "$code" -ne 0 ] && [ "$fail" -eq 0 ]; then
    fail=$code
  fi
}

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
  note rustup-clippy $?
  if [ "$(cat /evidence/rustup-clippy.exit)" -ne 0 ]; then
    printf '%s\n' "$(cat /evidence/rustup-clippy.exit)" > /evidence/inside.exit
    exit "$(cat /evidence/rustup-clippy.exit)"
  fi
else
  echo "clippy_component=already_installed"
  note rustup-clippy 0
fi

echo "=== cargo fetch --locked ==="
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
note fetch $?
if [ "$(cat /evidence/fetch.exit)" -ne 0 ]; then
  printf '%s\n' "$(cat /evidence/fetch.exit)" > /evidence/inside.exit
  exit "$(cat /evidence/fetch.exit)"
fi

echo "=== pipeline lib ==="
cargo test -p oo-pipeline --lib --locked --offline > /evidence/pipeline-lib.stdout 2> /evidence/pipeline-lib.stderr
note pipeline-lib $?

echo "=== import-pg cli focused ==="
cargo test -p oo-pipeline --test import_pg_cli_live --locked --offline > /evidence/import-cli.stdout 2> /evidence/import-cli.stderr
note import-cli $?

echo "=== pipeline clippy ==="
cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings > /evidence/pipeline-clippy.stdout 2> /evidence/pipeline-clippy.stderr
note pipeline-clippy $?

printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
