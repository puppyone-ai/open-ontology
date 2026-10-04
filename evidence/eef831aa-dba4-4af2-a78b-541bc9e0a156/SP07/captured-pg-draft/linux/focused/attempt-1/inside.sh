#!/bin/bash
# Focused oo-builder regression on the fixed-base archive plus the a156 overlay.
set -u
echo "oo_task=captured-pg-draft-focused"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
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

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/focused/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2
     printf '%s\n' 2 > /evidence/linux/focused/toolchain.exit
     exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line" | tee /evidence/linux/focused/rustc-host.txt
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2
     printf '%s\n' 2 > /evidence/linux/focused/toolchain.exit
     exit 2 ;;
esac
printf '%s\n' 0 > /evidence/linux/focused/toolchain.exit

if [ ! -f crates/oo-builder/src/captured_pg.rs ]; then
  echo "focused archive is missing the captured-pg overlay" >&2
  printf '%s\n' 4 > /evidence/linux/focused/snapshot.exit
  exit 4
fi
if [ ! -f examples/ontology-draft/failures/adapter.code ]; then
  echo "archive is missing checked-in ontology-draft goldens" >&2
  printf '%s\n' 4 > /evidence/linux/focused/snapshot.exit
  exit 4
fi
sha256sum \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/draft.rs \
  crates/oo-builder/src/pgsql.rs \
  crates/oo-builder/src/captured_pg.rs \
  crates/oo-builder/tests/captured_pg.rs \
  > /evidence/linux/focused/overlay.sha256
printf '%s\n' 0 > /evidence/linux/focused/snapshot.exit

run_phase() {
  name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/linux/focused/${name}.stdout" 2> "/evidence/linux/focused/${name}.stderr"
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/linux/focused/${name}.exit"
  echo "phase ${name} exit ${code}"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

run_phase rustup-rustfmt rustup component add rustfmt --toolchain 1.94.0
run_phase rustfmt rustfmt --check --edition 2024 \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/draft.rs \
  crates/oo-builder/src/pgsql.rs \
  crates/oo-builder/src/captured_pg.rs \
  crates/oo-builder/tests/captured_pg.rs
run_phase lib cargo test -p oo-builder --lib --locked --offline
run_phase captured_pg cargo test -p oo-builder --test captured_pg --locked --offline
run_phase structure_draft_cli cargo test -p oo-builder --test structure_draft_cli --locked --offline
run_phase clippy cargo clippy -p oo-builder --lib --test captured_pg --locked --offline -- -D warnings
echo FOCUSED_OK
printf '%s\n' 0 > /evidence/linux/focused/inside.exit
