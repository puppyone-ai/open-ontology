#!/bin/bash
# Builder library tests and scoped Clippy on the immutable fixed-base archive.
# No source edits. No pipeline crate.
set -u
echo "oo_task=captured-pg-draft-pre-edit-smoke"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "fixed_base=e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a156-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a156-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
cd /src

rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/smoke/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2
     printf '%s\n' 2 > /evidence/linux/smoke/toolchain.exit
     exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line" | tee /evidence/linux/smoke/rustc-host.txt
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2
     printf '%s\n' 2 > /evidence/linux/smoke/toolchain.exit
     exit 2 ;;
esac
printf '%s\n' 0 > /evidence/linux/smoke/toolchain.exit

if [ ! -f crates/oo-builder/src/lib.rs ]; then
  echo "archive is missing oo-builder" >&2
  printf '%s\n' 4 > /evidence/linux/smoke/snapshot.exit
  exit 4
fi
if [ -f crates/oo-builder/src/captured_pg.rs ]; then
  echo "smoke archive already contains the captured-pg overlay" >&2
  printf '%s\n' 4 > /evidence/linux/smoke/snapshot.exit
  exit 4
fi
if [ ! -f examples/ontology-draft/failures/adapter.code ] || [ ! -f examples/ontology-draft/MANIFEST.sha256 ]; then
  echo "archive is missing checked-in ontology-draft goldens" >&2
  printf '%s\n' 4 > /evidence/linux/smoke/snapshot.exit
  exit 4
fi
sha256sum crates/oo-builder/src/lib.rs crates/oo-builder/src/draft.rs crates/oo-builder/src/pgsql.rs > /evidence/linux/smoke/preimage.sha256
printf '%s\n' 0 > /evidence/linux/smoke/snapshot.exit

run_phase() {
  name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/linux/smoke/${name}.stdout" 2> "/evidence/linux/smoke/${name}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/linux/smoke/${name}.exit"
  echo "phase ${name} exit ${code}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

run_phase rustup-clippy rustup component add clippy --toolchain 1.94.0
run_phase fetch cargo fetch --locked
run_phase lib cargo test -p oo-builder --lib --locked --offline
run_phase clippy cargo clippy -p oo-builder --lib --locked --offline -- -D warnings
echo SMOKE_OK
printf '%s\n' 0 > /evidence/linux/smoke/inside.exit
