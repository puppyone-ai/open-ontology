#!/bin/bash
# Focused tests and Clippy of the zero-dependency fixture driver.
# No PostgreSQL. No cargo update. No root lock.
set -u
echo "oo_task=sp11-real-pg-e2e-focused"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a317"
echo "phase=focused"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/pg-e2e
export CARGO_TERM_COLOR=never
export HOME=/tmp
export TMPDIR=/cargo-home/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/focused/phases /cargo-home/tmp /target/pg-e2e
cd /fixture

record() {
  mkdir -p /evidence/linux/focused/phases
  printf '%s\n' "$2" > "/evidence/linux/focused/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/focused/focused.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/focused/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
printf '%s\n' "no" > /evidence/linux/focused/cargo-update-invoked.txt

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/focused/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/linux/focused/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

sum_field() {
  grep '^test result:' /evidence/linux/focused/test.log \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > /evidence/linux/focused/test-count.txt
if [ "$(cat /evidence/linux/focused/test-count.txt)" != "passed=9 failed=0 ignored=0" ]; then
  echo "unexpected test count" >&2
  record test-count 1
fi
record test-count 0

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/focused/clippy-component.log 2>&1
component_exit=$?
set -e
record clippy-component "$component_exit"

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/focused/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

if grep -F 'cargo update' /evidence/linux/focused/fetch.log /evidence/linux/focused/test.log /evidence/linux/focused/clippy.log >/dev/null 2>&1; then
  echo "cargo update text appeared" >&2
  record cargo-update-text 1
fi
record cargo-update-text 0
printf '%s\n' 0 > /evidence/linux/focused/focused.exit
echo "focused=ok"
exit 0
