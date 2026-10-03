#!/bin/bash
# Unmodified author tests against the immutable ccd93e1a archive.
set -u
echo "oo_task=reader-primitives-review-author"
echo "airgap_claim=no"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a130-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a130-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
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
if [ -f crates/oo-connectors/tests/a130_review.rs ]; then
  echo "author snapshot must not contain the review overlay" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if ! sha256sum -c /evidence/snapshot-files.sha256; then
  echo "snapshot bytes do not match ccd93e1a" >&2
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

run_test() {
  name=$1
  shift
  echo "=== cargo test $name ==="
  set +e
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test "$name" -- --test-threads=1 --nocapture
  code=$?
  set +e
  note "$name" "$code"
}

run_test catalog_rate_live
run_test snapshot_live
run_test pg_portal_bounds
run_test pg_portal_live

{
  printf 'first_byte='
  od -An -t u1 -N 1 /pglog/postgresql.log | tr -d ' '
  printf 'size='
  wc -c < /pglog/postgresql.log
} > /evidence/log-prefix.txt
leak=0
for file in /secrets/reader.pass /secrets/writer.pass /secrets/guc.pass /secrets/super.pass /secrets/lot-reader.pass /secrets/lot-writer.pass; do
  if [ -s "$file" ] && grep -F -q -f "$file" /pglog/postgresql.log; then
    leak=1
  fi
done
if [ "$leak" -ne 0 ]; then
  echo "log_leak=yes" > /evidence/log-leak.txt
  echo "elapsed_seconds=$SECONDS"
  exit 1
fi
echo "log_leak=no" > /evidence/log-leak.txt
cp /pglog/postgresql.log /evidence/postgresql.log
echo "elapsed_seconds=$SECONDS"
exit "$fail"
