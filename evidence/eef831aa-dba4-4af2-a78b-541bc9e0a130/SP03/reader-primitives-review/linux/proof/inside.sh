#!/bin/bash
# Focused composition proof. The archive is ccd93e1a plus this overlay test only.
set -u
echo "oo_task=reader-primitives-review-proof"
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
if [ ! -f crates/oo-connectors/tests/a130_review.rs ]; then
  echo "overlay test missing" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if ! sha256sum -c /evidence/snapshot-files.sha256; then
  echo "product bytes do not match ccd93e1a" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/snapshot.exit

echo "=== cargo test a130_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test a130_review -- --test-threads=1 --nocapture
code=$?
set +e
printf '%s\n' "$code" > /evidence/a130_review.exit
echo "phase_exit=$code phase=a130_review"

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
exit "$code"
