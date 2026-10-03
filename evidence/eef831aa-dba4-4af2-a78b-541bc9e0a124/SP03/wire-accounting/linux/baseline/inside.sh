#!/bin/bash
# Immutable baseline. Fetch, library tests, and Clippy. No product edits inside.
# airgap_claim=no. Fetch uses the network. Later phases pass --offline.
set -u
echo "oo_task=wire-accounting-baseline"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a124-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a124-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a124-baseline
git config --file "$GIT_CONFIG_GLOBAL" user.email a124-baseline@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
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

cd /src
if [ ! -f crates/oo-connectors/tests/pg_wire_bounds.rs ]; then
  echo "snapshot missing pg_wire_bounds" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ -f crates/oo-connectors/tests/pg_wire_accounting.rs ]; then
  echo "baseline snapshot already contains the accounting test" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/snapshot.exit

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set +e
  printf '%s\n' "$clippy_add" > /evidence/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /evidence/clippy-component.exit
fi

echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set +e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fetch_exit"
fi

echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
lib_exit=$?
set +e
echo "phase_exit=$lib_exit phase=cargo_test_oo_connectors_lib"
printf '%s\n' "$lib_exit" > /evidence/lib.exit

echo "=== cargo clippy -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set +e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_connectors"
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit

echo "elapsed_seconds=$SECONDS"
if [ "$lib_exit" -ne 0 ]; then
  exit "$lib_exit"
fi
exit "$clippy_exit"
