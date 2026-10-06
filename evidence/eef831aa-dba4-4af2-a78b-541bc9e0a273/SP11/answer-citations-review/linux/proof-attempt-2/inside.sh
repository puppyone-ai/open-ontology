#!/bin/bash
# Focused tests and Clippy for the review harness on a fresh archive copy.
# Linux arm64 Rust 1.94. Fetch uses the private bridge. Test and Clippy are offline.
# No cargo update. No apt. No PostgreSQL.
set -u
echo "oo_task=sp11-answer-citations-review-proof"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a273"
echo "oo_fixed=9951cda7f128e30f1f6ce02e409d542d20a9e4fb"
echo "oo_platform=linux/arm64"
echo "phase=proof"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/proof-attempt-2/phases /cargo-home/bin
cd /src
printf '%s\n' "not-invoked" > /evidence/linux/proof-attempt-2/cargo-update-invoked.txt
printf '%s\n' 0 > /evidence/linux/proof-attempt-2/pg.txt
MANIFEST=tools/answer-citations/Cargo.toml

record() {
  mkdir -p /evidence/linux/proof-attempt-2/phases
  printf '%s\n' "$2" > "/evidence/linux/proof-attempt-2/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

fail() {
  record "$1" "$2"
  printf '%s\n' "$2" > /evidence/linux/proof-attempt-2/proof.exit
  exit "$2"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/proof-attempt-2/rustc-version.txt
rustc -Vv > /evidence/linux/proof-attempt-2/rustc-verbose.txt
cargo -V | tee /evidence/linux/proof-attempt-2/cargo-version.txt
rustup show > /evidence/linux/proof-attempt-2/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; fail toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/proof-attempt-2/fixed-base.txt
if [ "$sha" != "9951cda7f128e30f1f6ce02e409d542d20a9e4fb" ]; then
  echo "archive sha mismatch" >&2
  fail source 2
fi
if [ ! -f tools/answer-citations/tests/a273_review.rs ]; then
  echo "review harness is absent" >&2
  fail source 2
fi
cmp -s tools/answer-citations/tests/a273_review.rs /evidence/overlay/a273_review.rs
record harness-cmp $?
if [ "$(cat /evidence/linux/proof-attempt-2/phases/harness-cmp.exit)" != "0" ]; then
  fail harness-cmp 1
fi
cmp -s tools/answer-citations/tests/fixtures/desk-explicit.audit.jsonl /evidence/source/blobs/desk-explicit-a242.audit.jsonl
record fixture-cmp $?
if [ "$(cat /evidence/linux/proof-attempt-2/phases/fixture-cmp.exit)" != "0" ]; then
  fail fixture-cmp 1
fi
record source 0

if [ ! -x /cargo-home/bin/lock-subset ]; then
  rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
  record lock-subset-tool $?
else
  record lock-subset-tool 0
fi
/cargo-home/bin/lock-subset Cargo.lock tools/answer-citations/Cargo.lock \
  > /evidence/linux/proof-attempt-2/lock-subset.txt \
  2> /evidence/linux/proof-attempt-2/lock-subset.err
record lock-subset $?
if [ "$(cat /evidence/linux/proof-attempt-2/phases/lock-subset.exit)" != "0" ]; then
  fail lock-subset 1
fi

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/proof-attempt-2/component.stdout 2> /evidence/linux/proof-attempt-2/component.stderr
component_exit=$?
set +e
printf '%s\n' "$component_exit" > /evidence/linux/proof-attempt-2/component.exit
echo "PHASE component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  fail component "$component_exit"
fi
record component 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path "$MANIFEST" --locked > /evidence/linux/proof-attempt-2/fetch.stdout 2> /evidence/linux/proof-attempt-2/fetch.stderr
fetch_exit=$?
set +e
printf '%s\n' "$fetch_exit" > /evidence/linux/proof-attempt-2/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  fail fetch "$fetch_exit"
fi
record fetch 0

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path "$MANIFEST" --locked --offline --no-fail-fast -- --test-threads=1 > /evidence/linux/proof-attempt-2/test.stdout 2> /evidence/linux/proof-attempt-2/test.stderr
test_exit=$?
set +e
printf '%s\n' "$test_exit" > /evidence/linux/proof-attempt-2/test.exit
echo "PHASE test_exit=$test_exit"
record test "$test_exit"

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path "$MANIFEST" --locked --offline --all-targets -- -D warnings > /evidence/linux/proof-attempt-2/clippy.stdout 2> /evidence/linux/proof-attempt-2/clippy.stderr
clippy_exit=$?
set +e
printf '%s\n' "$clippy_exit" > /evidence/linux/proof-attempt-2/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
record clippy "$clippy_exit"

if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' "$test_exit" > /evidence/linux/proof-attempt-2/proof.exit
  exit "$test_exit"
fi
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' "$clippy_exit" > /evidence/linux/proof-attempt-2/proof.exit
  exit "$clippy_exit"
fi
printf '%s\n' 0 > /evidence/linux/proof-attempt-2/proof.exit
echo "proof_done"
