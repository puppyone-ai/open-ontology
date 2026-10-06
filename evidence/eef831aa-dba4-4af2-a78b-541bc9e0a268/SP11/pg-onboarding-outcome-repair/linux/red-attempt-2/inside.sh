#!/bin/bash
# Red run: unrepaired archive sources plus the repaired tests.
# A non-zero test exit is the expected result and is preserved.
set -u
echo "oo_task=sp11-pg-onboarding-outcome-repair-red-attempt-2"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a268"
echo "oo_platform=linux/arm64"
echo "phase=red-attempt-2"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/onboarding
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/red-attempt-2/phases /target/onboarding
cd /onboarding

record() {
  mkdir -p /evidence/linux/red-attempt-2/phases
  printf '%s\n' "$2" > "/evidence/linux/red-attempt-2/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/red-attempt-2/red.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/red-attempt-2/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
printf '%s\n' "no" > /evidence/linux/red-attempt-2/cargo-update-invoked.txt

if grep -q 'changes_may_have_been_applied' src/outcome.rs; then
  echo "red source already contains the repair" >&2
  record source-identity 2
fi
if ! grep -q 'unchanged_by_this_tool' src/outcome.rs; then
  echo "red source missing the frozen failure string" >&2
  record source-identity 2
fi
if ! grep -q 'changes_may_have_been_applied' tests/orchestration.rs; then
  echo "red tests do not contain the repaired assertion" >&2
  record source-identity 2
fi
record source-identity 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/red-attempt-2/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/linux/red-attempt-2/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/linux/red-attempt-2/phases/test.exit
echo "phase_exit=$test_exit phase=test"
printf '%s\n' "$test_exit" > /evidence/linux/red-attempt-2/red.exit
if [ "$test_exit" -eq 0 ]; then
  echo "red tests passed on the unrepaired archive" >&2
  exit 2
fi
echo "red=failed-as-expected"
exit "$test_exit"
