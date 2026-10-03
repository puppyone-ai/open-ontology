#!/bin/bash
# Overlay on fixed bd77b252. Scoped Clippy, library, ACL regression, then the existing stream tests once.
set -u
echo "oo_task=stream-acl"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=bd77b252_plus_acl_overlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a136-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a136-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a136-acl
git config --file "$GIT_CONFIG_GLOBAL" user.email a136-acl@invalid.example
rustc -Vv
got=$(rustc --version)
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
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
if [ ! -f crates/oo-connectors/tests/pg_stream_acl_live.rs ]; then
  echo "overlay test is missing" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
grep -q "has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'SELECT')" crates/oo-connectors/src/sql.rs
grep -q 'select_not_granted' crates/oo-connectors/src/snapshot.rs
printf '%s\n' 0 > /evidence/overlay.exit

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

echo "=== cargo clippy -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
note clippy $?
if [ "$(cat /evidence/clippy.exit)" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fail"
fi

echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
note lib $?

echo "=== pg_stream_acl_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_acl_live -- --nocapture
note acl $?

echo "=== pg_stream_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_live -- --nocapture
note live $?

echo "=== pg_stream_bounds ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_bounds -- --nocapture
note bounds $?

echo "elapsed_seconds=$SECONDS"
exit "$fail"
