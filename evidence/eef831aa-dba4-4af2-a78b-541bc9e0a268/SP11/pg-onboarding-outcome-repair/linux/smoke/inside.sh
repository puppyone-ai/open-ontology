#!/bin/bash
# Focused pre-edit smoke for the archived tools/pg-onboarding tree.
# Linux arm64 Rust 1.94. No cargo update. No product crate. No PostgreSQL.
set -u
echo "oo_task=sp11-pg-onboarding-outcome-repair-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a268"
echo "oo_platform=linux/arm64"
echo "phase=smoke"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/onboarding
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/smoke/phases /target/onboarding
cd /onboarding

record() {
  mkdir -p /evidence/linux/smoke/phases
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt
cargo --version > /evidence/linux/smoke/cargo-version.txt
rustc --version --verbose > /evidence/linux/smoke/rustc-verbose.txt

if grep -q 'changes_may_have_been_applied' src/outcome.rs; then
  echo "archive already contains the repair" >&2
  record archive-identity 2
fi
if ! grep -q 'unchanged_by_this_tool' src/outcome.rs; then
  echo "archive missing the frozen failure string" >&2
  record archive-identity 2
fi
record archive-identity 0

echo "=== rustc sha256_manifest ==="
set +e
rustc --edition 2024 /tools/sha256_manifest.rs -o /target/sha256_manifest \
  > /evidence/linux/smoke/sha256-tool.log 2>&1
tool_exit=$?
set -e
record sha256-tool "$tool_exit"
/target/sha256_manifest /onboarding /evidence/linux/smoke/tool-sha256.txt \
  > /evidence/linux/smoke/sha256-tool-run.txt
record sha256-walk 0

if [ -f Cargo.lock ]; then
  echo "lock=present" > /evidence/linux/smoke/lock-before.txt
else
  echo "lock missing" >&2
  record lock 2
fi
record lock 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/linux/smoke/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

if grep -q 'cargo update' /evidence/linux/smoke/fetch.log /evidence/linux/smoke/test.log; then
  echo "cargo update text appeared in a log" >&2
  record cargo-update-text 1
fi
record cargo-update-text 0

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "smoke=ok"
exit 0
