#!/bin/bash
# Overlay on the immutable 3fba40dd tree. One private PostgreSQL 17.10.
set -u
echo "oo_task=import-pg-cli-review-probe"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=3fba40dd_archive_plus_a164_overlay"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a164-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a164-gitsystem
unset RUST_LOG || true
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a164-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a164-probe@invalid.example
rustc -Vv
cargo -V
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
rustc -Vv > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt

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

head -n 1189 crates/oo-connectors/src/session.rs > /tmp/session-prefix.rs
if ! cmp -s /tmp/session-prefix.rs /archive/crates/oo-connectors/src/session.rs; then
  echo "session prefix differs from the fixed commit" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
for path in \
  crates/oo-pipeline/src/full_import.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/migrations/0005_full_import_expand.sql \
  crates/oo-connectors/src/snapshot.rs \
  Cargo.lock
do
  if ! cmp -s "$path" "/archive/$path"; then
    echo "fixed source differs: $path" >&2
    printf '%s\n' 2 > /evidence/snapshot.exit
    exit 2
  fi
done
sha256sum \
  /archive/crates/oo-connectors/src/session.rs \
  crates/oo-connectors/src/session.rs \
  crates/oo-pipeline/src/full_import.rs \
  crates/oo-pipeline/src/import_cli.rs \
  > /evidence/probe-sha256.txt
if ! grep -q 'mod a164_rollback_depth' crates/oo-connectors/src/session.rs; then
  echo "depth overlay missing" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ ! -f crates/oo-pipeline/tests/import_pg_a164_review.rs ]; then
  echo "review test missing" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
echo "archive_prefix_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit

mkdir -p /evidence/out
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib -- --ignored --test-threads=1 a164_rollback_depth > /evidence/depth.stdout 2> /evidence/depth.stderr
note depth $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test import_pg_a164_review -- --ignored --test-threads=1 > /evidence/review.stdout 2> /evidence/review.stderr
note review $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_prepare_snapshot_live -- --ignored --test-threads=1 prepare_snapshot_reads_the_public_session > /evidence/prepare-existing.stdout 2> /evidence/prepare-existing.stderr
note prepare-existing $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test import_pg_cli_live -- --ignored --test-threads=1 import_pg_binary_on_private_postgres > /evidence/import-existing.stdout 2> /evidence/import-existing.stderr
note import-existing $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test import_pg_cli_live > /evidence/cli-errors.stdout 2> /evidence/cli-errors.stderr
note cli-errors $?
set -e

echo "probe_fail=$fail"
printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
