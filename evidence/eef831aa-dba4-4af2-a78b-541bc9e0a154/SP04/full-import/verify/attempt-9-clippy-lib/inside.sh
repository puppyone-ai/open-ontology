#!/bin/bash
# Focused oo-pipeline tests against one private PostgreSQL 17.10.
set -u
echo "oo_task=full-import-verify"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a154-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a154-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
cd /src

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2
     printf '%s\n' 2 > /evidence/toolchain.exit
     exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line" | tee /evidence/rustc-host.txt
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2
     printf '%s\n' 2 > /evidence/toolchain.exit
     exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit

if [ ! -f crates/oo-pipeline/src/full_import.rs ]; then
  echo "archive is missing the full-import overlay" >&2
  exit 4
fi
if [ ! -f contracts/pipeline/full-import-v1.json ]; then
  echo "archive is missing the full-import contract" >&2
  exit 4
fi

run_phase() {
  name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/${name}.stdout" 2> "/evidence/${name}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/${name}.exit"
  echo "phase ${name} exit ${code}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

mkdir -p /evidence/installer /evidence/live
run_phase lib cargo test -p oo-pipeline --lib --locked --offline -- --test-threads=1
run_phase installer cargo test -p oo-pipeline --test target_installer_live --locked --offline -- --ignored --test-threads=1 --nocapture
run_phase full-import cargo test -p oo-pipeline --test full_import_live --locked --offline -- --ignored --test-threads=1 --nocapture
run_phase rustup-clippy rustup component add clippy --toolchain 1.94.0
run_phase clippy cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings
echo VERIFY_OK
printf '%s\n' 0 > /evidence/inside.exit
