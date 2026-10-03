#!/bin/bash
# Author suite plus independent probe. Offline against the smoke cargo volume.
# airgap_claim=no. The volume was filled by the smoke fetch.
set -u
echo "oo_task=wire-accounting-review-1"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export RUST_BACKTRACE=1
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a126-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a126-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a126-review
git config --file "$GIT_CONFIG_GLOBAL" user.email a126-review@invalid.example
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
if ! sha256sum -c /evidence/snapshot-files.sha256; then
  echo "product bytes are not ff783ac9" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if ! sha256sum -c /evidence/probe.sha256; then
  echo "probe bytes changed" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/snapshot.exit

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
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set +e
  note clippy-component "$clippy_add"
  if [ "$clippy_add" -ne 0 ]; then
    echo "elapsed_seconds=$SECONDS"
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  note clippy-component 0
fi

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline
fetch_exit=$?
set +e
note fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fetch_exit"
fi

echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
note lib $?

echo "=== cargo test pg_wire_bounds ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds -- --test-threads=1 --nocapture
note bounds $?

echo "=== cargo test pg_wire_accounting ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_accounting -- --test-threads=1 --nocapture
note accounting $?

echo "=== cargo test pg_wire_accounting_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_accounting_review -- --test-threads=1 --nocapture
note review $?

echo "=== cargo clippy review test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_accounting_review -- -D warnings
note clippy-review $?

if ! sha256sum -c /evidence/snapshot-files.sha256; then
  echo "product bytes changed during the run" >&2
  note product-after 2
else
  note product-after 0
fi

echo "elapsed_seconds=$SECONDS"
exit "$fail"
