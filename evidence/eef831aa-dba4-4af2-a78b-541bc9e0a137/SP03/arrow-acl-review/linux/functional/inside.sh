#!/bin/bash
# Exact dd5bf441 archive plus the column-grant review test only.
set -u
echo "oo_task=arrow-acl-review"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=dd5bf441_plus_column_grant_overlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a137-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a137-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a137-review
git config --file "$GIT_CONFIG_GLOBAL" user.email a137-review@invalid.example
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
  echo "fixed archive is missing pg_stream_acl_live.rs" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
if [ ! -f crates/oo-connectors/tests/pg_stream_acl_review.rs ]; then
  echo "review overlay is missing" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
if ! grep -q 'aclexplode' crates/oo-connectors/src/sql.rs; then
  echo "CURRENT_CATALOG has no aclexplode" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
if ! grep -q 'select_not_granted' crates/oo-connectors/src/snapshot.rs; then
  echo "guard has no select_not_granted" >&2
  printf '%s\n' 2 > /evidence/overlay.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay.exit

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

echo "=== pg_stream_acl_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_acl_review -- --nocapture
note review $?

echo "elapsed_seconds=$SECONDS"
exit "$fail"
