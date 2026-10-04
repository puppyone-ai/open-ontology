#!/bin/bash
# Corrected writer grant. Independent finish probe only.
set -u
echo "oo_task=reader-r4-probe-3"
echo "oo_toolchain_request=1.94.0"
echo "composition=f5f0a73b_archive_plus_r4_probe_writer_select"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a146-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a146-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a146-probe
git config --file "$GIT_CONFIG_GLOBAL" user.email a146-probe@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
got=$(rustc --version)
printf '%s\n' "$got"
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
printf 'rustc_host=%s\n' "$host_line"
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
  crates/oo-connectors/src/snapshot.rs \
  crates/oo-connectors/src/session.rs \
  crates/oo-connectors/src/stream.rs \
  contracts/source/pgsql/reader-lifecycle-v1.md \
  crates/oo-connectors/tests/pg_reader_r4_probe.rs \
  > /evidence/archive-sha256.txt
check_hash() {
  rel=$1
  expect=$2
  got=$(sha256sum "$rel" | awk '{print $1}')
  want=$(awk '{print $1}' "$expect")
  if [ "$got" != "$want" ] || [ -z "$got" ]; then
    echo "archive sha mismatch for $rel got=$got want=$want" >&2
    printf '%s\n' 2 > /evidence/snapshot.exit
    exit 2
  fi
}
check_hash crates/oo-connectors/src/snapshot.rs /evidence/expect-snapshot.sha256
check_hash crates/oo-connectors/src/session.rs /evidence/expect-session.sha256
check_hash crates/oo-connectors/src/stream.rs /evidence/expect-stream.sha256
check_hash contracts/source/pgsql/reader-lifecycle-v1.md /evidence/expect-lifecycle.sha256
check_hash crates/oo-connectors/tests/pg_reader_r4_probe.rs /evidence/expect-probe.sha256
echo "archive_hashes_match=1"
printf '%s\n' 0 > /evidence/snapshot.exit
export OO_PG_R4_FIXTURE=/secrets/pointer.json
export OO_PG_R4_OUTCOME=/evidence/r4-outcome.txt
echo "=== cargo test --test pg_reader_r4_probe ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_reader_r4_probe -- --test-threads=1
note pg_reader_r4_probe $?
if [ -f /pglog/postgresql.log ]; then
  cp /pglog/postgresql.log /evidence/postgres-window.log
fi
echo "elapsed_seconds=$SECONDS"
exit "$fail"
