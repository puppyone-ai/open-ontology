#!/bin/bash
# Pre-edit smoke of the archived pg-onboarding tool. No fixture overlay.
set -u
echo "oo_task=sp11-real-pg-100k-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a349"
echo "phase=smoke"
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
printf '%s\n' "$got" > /evidence/linux/smoke/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt
if [ -f tests/pg_e2e_overlay.rs ]; then
  echo "fixture overlay is present in the archive tool" >&2
  record overlay 2
fi
record overlay 0

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

sum_field() {
  grep '^test result:' /evidence/linux/smoke/test.log \
    | sed -n "s/.* \\([0-9][0-9]*\\) $1.*/\\1/p" \
    | awk '{s+=$1} END{print s+0}'
}
printf 'passed=%s failed=%s ignored=%s\n' "$(sum_field passed)" "$(sum_field failed)" "$(sum_field ignored)" \
  > /evidence/linux/smoke/test-count.txt
if [ "$(cat /evidence/linux/smoke/test-count.txt)" != "passed=24 failed=0 ignored=1" ]; then
  echo "unexpected test count" >&2
  record test-count 1
fi
record test-count 0
if ! grep -F 'test live_chain ... ignored' /evidence/linux/smoke/test.log >/dev/null; then
  echo "live_chain was not ignored" >&2
  record live-ignored 1
fi
record live-ignored 0

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/smoke/clippy-component.log 2>&1
component_exit=$?
set -e
record clippy-component "$component_exit"

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/smoke/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

if grep -q 'cargo update' /evidence/linux/smoke/test.log /evidence/linux/smoke/clippy.log /evidence/linux/smoke/fetch.log; then
  echo "cargo update text appeared in a log" >&2
  record cargo-update-text 1
fi
record cargo-update-text 0
printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "smoke=ok"
exit 0
