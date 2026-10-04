#!/bin/bash
# Scoped Clippy of oo-pipeline on the immutable fixed-base archive.
# No source edits.
set -u
echo "oo_task=full-import-pre-edit-smoke"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "fixed_base=bedd6a2ced65edf6a44ac85ecd40551e1b79a45c"
uname -a
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

rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/smoke/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2
     printf '%s\n' 2 > /evidence/smoke/toolchain.exit
     exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line" | tee /evidence/smoke/rustc-host.txt
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2
     printf '%s\n' 2 > /evidence/smoke/toolchain.exit
     exit 2 ;;
esac
printf '%s\n' 0 > /evidence/smoke/toolchain.exit

if [ ! -f crates/oo-pipeline/src/lib.rs ]; then
  echo "archive is missing oo-pipeline" >&2
  printf '%s\n' 4 > /evidence/smoke/snapshot.exit
  exit 4
fi
if [ -f crates/oo-pipeline/src/full_import.rs ]; then
  echo "smoke archive already contains the full-import overlay" >&2
  printf '%s\n' 4 > /evidence/smoke/snapshot.exit
  exit 4
fi
sha256sum crates/oo-pipeline/src/lib.rs crates/oo-pipeline/src/target_installer.rs crates/oo-pipeline/src/copy_encoder.rs > /evidence/smoke/preimage.sha256
printf '%s\n' 0 > /evidence/smoke/snapshot.exit

run_phase() {
  name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/smoke/${name}.stdout" 2> "/evidence/smoke/${name}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/smoke/${name}.exit"
  echo "phase ${name} exit ${code}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

run_phase rustup-clippy rustup component add clippy --toolchain 1.94.0
run_phase fetch cargo fetch --locked
run_phase clippy cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings
echo CLIPPY_OK
printf '%s\n' 0 > /evidence/smoke/inside.exit
