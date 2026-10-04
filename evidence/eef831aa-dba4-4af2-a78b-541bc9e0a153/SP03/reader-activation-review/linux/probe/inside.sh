#!/bin/bash
# Archive of bedd6a2c. One author live test. No product overlay.
set -u
echo "oo_task=reader-activation-review-probe"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "composition=bedd6a2c_archive_author_complete_live_once"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a153-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a153-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a153-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a153-probe@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
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

check_hash() {
  rel=$1
  expect=$2
  got=$(sha256sum "$rel" | awk '{print $1}')
  want=$(awk '{print $1}' "$expect")
  if [ "$got" != "$want" ] || [ -z "$got" ]; then
    echo "sha mismatch for $rel got=$got want=$want" >&2
    printf '%s\n' 2 > /evidence/archive.exit
    exit 2
  fi
}
check_hash crates/oo-connectors/src/model.rs /evidence/expect-model.sha256
check_hash crates/oo-connectors/src/types/mod.rs /evidence/expect-types.sha256
check_hash crates/oo-connectors/src/types/tests.rs /evidence/expect-types-tests.sha256
check_hash crates/oo-connectors/src/key_plan.rs /evidence/expect-keyplan.sha256
check_hash crates/oo-connectors/src/lib.rs /evidence/expect-lib.sha256
check_hash crates/oo-connectors/tests/live_pg.rs /evidence/expect-live.sha256
check_hash crates/oo-connectors/tests/lossless_live.rs /evidence/expect-lossless.sha256
check_hash crates/oo-connectors/tests/pg_reader_complete_live.rs /evidence/expect-complete.sha256
check_hash contracts/source/pgsql/bounded-stream-v1.md /evidence/expect-bounded.sha256
check_hash contracts/source/pgsql/reader-lifecycle-v1.md /evidence/expect-lifecycle.sha256
check_hash crates/oo-connectors/src/snapshot.rs /evidence/expect-snapshot.sha256
check_hash crates/oo-connectors/src/stream.rs /evidence/expect-stream.sha256
check_hash crates/oo-connectors/src/session.rs /evidence/expect-session.sha256
check_hash crates/oo-connectors/src/sql.rs /evidence/expect-sql.sha256
check_hash crates/oo-connectors/Cargo.toml /evidence/expect-cargo-toml.sha256
check_hash Cargo.lock /evidence/expect-cargo-lock.sha256
echo "archive_hashes_match=1"
printf '%s\n' 0 > /evidence/archive.exit
sed -n '9p' contracts/source/pgsql/bounded-stream-v1.md > /evidence/bounded-stream-line9.txt

export OO_PG_COMPLETE_FIXTURE=/secrets/pointer.json
export OO_PG_COMPLETE_OUTCOME=/evidence/complete-outcome.txt

echo "=== cargo test pg_reader_complete_live ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_complete_live -- --test-threads=1 --nocapture
note pg_reader_complete_live $?

if [ -f /pglog/postgresql.log ]; then
  cp /pglog/postgresql.log /evidence/postgresql.log
fi

echo "elapsed_seconds=$SECONDS"
exit "$fail"
