#!/bin/bash
# Locked offline tests and Clippy for the logistics acceptance crate.
set -u
export HOME=/tmp
export TMPDIR=/tmp/oracle
export CARGO_TARGET_DIR=/work/target
mkdir -p "$TMPDIR" /evidence/phases
cd /work
record() {
  printf '%s\n' "$2" > /evidence/phases/$1.exit
  echo "PHASE $1=$2"
}
if [ ! -f Cargo.lock ]; then
  cargo generate-lockfile --offline > /evidence/lock.log 2>&1
  record lockfile $?
else
  printf '%s\n' present > /evidence/lock.log
  record lockfile 0
fi
if [ "$(cat /evidence/phases/lockfile.exit)" != 0 ]; then
  exit 1
fi
cargo test --offline --locked -- --test-threads=1 > /evidence/test.log 2>&1
record test $?
if cargo clippy --version > /evidence/clippy-version.log 2>&1; then
  cargo clippy --offline --locked --all-targets -- -D warnings > /evidence/clippy.log 2>&1
  record clippy $?
  if [ "$(cat /evidence/phases/clippy.exit)" = 0 ]; then
    printf '%s\n' passed > /evidence/clippy.status
  else
    printf '%s\n' failed > /evidence/clippy.status
  fi
else
  cp /evidence/clippy-version.log /evidence/clippy.log
  printf '%s\n' "clippy component absent from pinned image 1.94.0-aarch64-unknown-linux-gnu; not installing" >> /evidence/clippy.log
  record clippy 127
  printf '%s\n' unavailable > /evidence/clippy.status
fi
if [ "$(cat /evidence/phases/test.exit)" != 0 ]; then
  printf '%s\n' 1 > /evidence/proof.exit
  echo "focused=tests-failed"
  exit 1
fi
if [ "$(cat /evidence/phases/clippy.exit)" = 0 ]; then
  printf '%s\n' 0 > /evidence/proof.exit
  echo "focused=ok"
  exit 0
fi
if [ "$(cat /evidence/phases/clippy.exit)" = 127 ]; then
  printf '%s\n' 127 > /evidence/proof.exit
  echo "focused=tests-ok clippy=unavailable"
  exit 0
fi
printf '%s\n' 1 > /evidence/proof.exit
echo "focused=clippy-failed"
exit 1
