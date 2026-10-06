#!/bin/bash
# Green run: repaired tool, focused tests, and Clippy.
# Linux arm64 Rust 1.94. No cargo update. No product crate. No PostgreSQL.
set -u
echo "oo_task=sp11-pg-onboarding-outcome-repair-green"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a268"
echo "oo_platform=linux/arm64"
echo "phase=green"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/onboarding
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/green/phases /target/onboarding
cd /onboarding

record() {
  mkdir -p /evidence/linux/green/phases
  printf '%s\n' "$2" > "/evidence/linux/green/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/green/green.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/green/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
printf '%s\n' "no" > /evidence/linux/green/cargo-update-invoked.txt

if ! grep -q 'changes_may_have_been_applied' src/outcome.rs; then
  echo "green source missing the repaired failure state" >&2
  record source-identity 2
fi
if ! grep -q 'duplicate' src/json_doc.rs; then
  echo "green source missing the JSON visitor" >&2
  record source-identity 2
fi
record source-identity 0

if [ -f Cargo.lock ]; then
  echo "lock=present" > /evidence/linux/green/lock-before.txt
else
  echo "lock missing" >&2
  record lock 2
fi
record lock 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/green/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --no-fail-fast -- --test-threads=1 \
  > /evidence/linux/green/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/green/clippy-component.log 2>&1
component_exit=$?
set -e
record clippy-component "$component_exit"

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/green/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

if grep -q 'cargo update' /evidence/linux/green/fetch.log /evidence/linux/green/test.log /evidence/linux/green/clippy.log; then
  echo "cargo update text appeared in a log" >&2
  record cargo-update-text 1
fi
record cargo-update-text 0

printf '%s\n' 0 > /evidence/linux/green/green.exit
echo "green=ok"
exit 0
