#!/bin/bash
# Original pg_stream_live, pg_stream_bounds, then the independent review overlay.
# Product sources stay the fixed commit. The only added file is the review test.
set -u
echo "oo_task=arrow-review-functional"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "composition=fixed_commit_75b993d0_plus_review_test_only"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a134-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a134-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a134-functional
git config --file "$GIT_CONFIG_GLOBAL" user.email a134-functional@invalid.example
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
if [ ! -f crates/oo-connectors/tests/pg_stream_review.rs ]; then
  echo "review overlay missing" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
buf=$(sha256sum crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs | awk '{print $1}')
want=$(awk '{print $1}' /evidence/expect-buffered.sha256)
if [ "$buf" != "$want" ]; then
  echo "functional tree buffered.rs drifted" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
stream=$(sha256sum crates/oo-connectors/src/stream.rs | awk '{print $1}')
want_stream=$(awk '{print $1}' /evidence/expect-stream.sha256)
if [ "$stream" != "$want_stream" ]; then
  echo "stream.rs drifted" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ ! -f contracts/source/pgsql/empty-catalog.json ]; then
  echo "empty-catalog.json missing" >&2
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

echo "=== cargo test pg_stream_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_live -- --test-threads=1 --nocapture
note pg_stream_live $?

echo "=== cargo test pg_stream_bounds ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_bounds -- --test-threads=1 --nocapture
note pg_stream_bounds $?

echo "=== cargo test pg_stream_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_stream_review -- --test-threads=1 --nocapture
note pg_stream_review $?

if [ -f /pglog/postgresql.log ]; then
  cp /pglog/postgresql.log /evidence/postgresql.log || true
fi
echo "elapsed_seconds=$SECONDS"
exit "$fail"
