#!/bin/bash
# Post-edit Linux 1.94 build. One builder. Both bins, focused tests, then the private catalog run.
set -u
echo "oo_task=catalog-pg-cli-post-2"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=head_archive_plus_owned_overlay"
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
if [ ! -f /src/crates/oo-pipeline/src/catalog_cli.rs ]; then
  echo "overlay is missing catalog_cli.rs" >&2
  printf '%s\n' 2 > /evidence/inside.exit
  exit 2
fi

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

echo "=== cargo build both bins ==="
cargo build -p oo-pipeline -p oo-builder --bins --locked --offline > /evidence/build-bins.stdout 2> /evidence/build-bins.stderr
note build-bins $?

echo "=== pipeline lib ==="
cargo test -p oo-pipeline --lib --locked --offline > /evidence/pipeline-lib.stdout 2> /evidence/pipeline-lib.stderr
note pipeline-lib $?

echo "=== import-pg cli focused ==="
cargo test -p oo-pipeline --test import_pg_cli_live --locked --offline > /evidence/import-cli.stdout 2> /evidence/import-cli.stderr
note import-cli $?

echo "=== catalog-pg cli focused ==="
cargo test -p oo-pipeline --test catalog_pg_cli_live --locked --offline > /evidence/catalog-cli.stdout 2> /evidence/catalog-cli.stderr
note catalog-cli $?

echo "=== pipeline clippy ==="
cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings > /evidence/pipeline-clippy.stdout 2> /evidence/pipeline-clippy.stderr
note pipeline-clippy $?

echo "=== captured-pg-draft cli ==="
cargo test -p oo-builder --test captured_pg_cli --locked --offline > /evidence/captured-cli.stdout 2> /evidence/captured-cli.stderr
note captured-cli $?

echo "=== review-pg-draft cli ==="
cargo test -p oo-builder --test review_pg_cli --locked --offline > /evidence/review-cli.stdout 2> /evidence/review-cli.stderr
note review-cli $?

if [ "$fail" -eq 0 ]; then
  echo "=== catalog-pg binary live ==="
  cargo test -p oo-pipeline --test catalog_pg_cli_live --locked --offline -- --ignored --test-threads=1 catalog_pg_binary_on_private_postgres \
    > /evidence/catalog-live.stdout 2> /evidence/catalog-live.stderr
  note catalog-live $?
else
  echo "skipping catalog live because an earlier phase failed"
fi

printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
