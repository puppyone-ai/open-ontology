#!/bin/bash
# Linux/arm64 Rust 1.94. Library tests, the keys live test, then scoped Clippy.
set -u
echo "oo_task=reader-r3-keys"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a138-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a138-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a138-keys
git config --file "$GIT_CONFIG_GLOBAL" user.email a138-keys@invalid.example
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

if [ ! -f crates/oo-connectors/src/key_plan.rs ] || [ ! -f crates/oo-connectors/tests/pg_reader_keys_live.rs ]; then
  echo "keys overlay is missing" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay.exit

echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
note lib $?
if [ "$(cat /evidence/lib.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo test --test pg_reader_keys_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_keys_live -- --test-threads=1
note keys $?
if [ "$(cat /evidence/keys.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  note clippy-component $?
  if [ "$(cat /evidence/clippy-component.exit)" -ne 0 ]; then
    echo "elapsed_seconds=$SECONDS"
    exit "$fail"
  fi
else
  echo "clippy_component=already_installed"
  note clippy-component 0
fi

echo "=== cargo clippy -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
note clippy $?

echo "elapsed_seconds=$SECONDS"
exit "$fail"
