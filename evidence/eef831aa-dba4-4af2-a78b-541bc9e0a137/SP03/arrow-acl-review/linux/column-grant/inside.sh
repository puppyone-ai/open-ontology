#!/bin/bash
# Column-grant representative only. Original live, bounds, and ACL tests already ran.
set -u
echo "oo_task=arrow-acl-column-grant"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a137-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a137-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a137-colg
git config --file "$GIT_CONFIG_GLOBAL" user.email a137-colg@invalid.example
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
if ! grep -q 'inherited_role_grant_streams_the_arrow_value' crates/oo-connectors/tests/pg_stream_acl_review.rs; then
  echo "overlay is not the inherited-grant representative" >&2
  printf '%s\n' 2 > /evidence/review.exit
  exit 2
fi
echo "=== pg_stream_acl_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_acl_review -- --nocapture
code=$?
printf '%s\n' "$code" > /evidence/review.exit
echo "phase_exit=$code phase=review"
echo "elapsed_seconds=$SECONDS"
exit "$code"
