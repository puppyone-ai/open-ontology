#!/bin/bash
# Pre-edit Linux 1.94 smoke of the immutable 3fba40dd archive. No overlay. No PostgreSQL.
set -u
echo "oo_task=import-pg-cli-review-smoke"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=3fba40dd_archive_no_overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a164-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a164-gitsystem
unset RUST_LOG || true
unset CARGO_REGISTRIES_CRATES_IO_PROTOCOL || true
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a164-smoke
git config --file "$GIT_CONFIG_GLOBAL" user.email a164-smoke@invalid.example
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

sha256sum \
  Cargo.lock \
  Cargo.toml \
  contracts/pipeline/import-pg-cli-v1.md \
  contracts/pipeline/full-import-v1.md \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/src/session.rs \
  crates/oo-connectors/src/snapshot.rs \
  crates/oo-connectors/tests/pg_prepare_snapshot_live.rs \
  crates/oo-pipeline/Cargo.toml \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/full_import.rs \
  crates/oo-pipeline/migrations/0005_full_import_expand.sql \
  crates/oo-pipeline/tests/import_pg_cli_live.rs \
  > /evidence/archive-sha256.txt
if ! cmp -s /evidence/archive-sha256.txt /evidence/expect-sha256.txt; then
  echo "archive sha mismatch" >&2
  diff -u /evidence/expect-sha256.txt /evidence/archive-sha256.txt >&2 || true
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ -e crates/oo-pipeline/tests/import_pg_a164_review.rs ] || [ -e crates/oo-connectors/src/a164_depth.rs ]; then
  echo "smoke archive contains the review overlay" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
echo "archive_hashes_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit

echo "=== simple classifier: unsafe tokens in the changed sources ==="
grep -n -H 'unsafe' \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/src/session.rs \
  crates/oo-connectors/src/snapshot.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  > /evidence/unsafe-lines.txt || true
if ! grep -q 'forbid(unsafe_code)' crates/oo-connectors/src/lib.rs; then
  echo "connectors missing forbid(unsafe_code)" >&2
  printf '%s\n' 2 > /evidence/classifier.exit
  exit 2
fi
if ! grep -q 'forbid(unsafe_code)' crates/oo-pipeline/src/lib.rs; then
  echo "pipeline missing forbid(unsafe_code)" >&2
  printf '%s\n' 2 > /evidence/classifier.exit
  exit 2
fi
if grep -n 'unsafe' \
  crates/oo-connectors/src/session.rs \
  crates/oo-connectors/src/snapshot.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  | grep -v 'forbid(unsafe_code)'; then
  echo "changed sources contain an unsafe token" >&2
  printf '%s\n' 2 > /evidence/classifier.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/classifier.exit
echo "classifier=simple_grep_plus_forbid_unsafe_code"

set +e
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy-'; then
  echo "clippy_component=present" | tee /evidence/rustup-clippy.stdout
  printf '%s\n' 0 > /evidence/rustup-clippy.exit
else
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
  printf '%s\n' $? > /evidence/rustup-clippy.exit
fi
note rustup-clippy "$(cat /evidence/rustup-clippy.exit)"

cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
note fetch $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib > /evidence/connectors-lib.stdout 2> /evidence/connectors-lib.stderr
note connectors-lib $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib > /evidence/pipeline-lib.stdout 2> /evidence/pipeline-lib.stderr
note pipeline-lib $?

cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test import_pg_cli_live > /evidence/cli-errors.stdout 2> /evidence/cli-errors.stderr
note cli-errors $?

cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings > /evidence/connectors-clippy.stdout 2> /evidence/connectors-clippy.stderr
note connectors-clippy $?

cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings > /evidence/pipeline-clippy.stdout 2> /evidence/pipeline-clippy.stderr
note pipeline-clippy $?
set -e

echo "smoke_fail=$fail"
printf '%s\n' "$fail" > /evidence/inside.exit
exit "$fail"
