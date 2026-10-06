#!/bin/bash
# Post-edit focused tests and Clippy for tools/pg-onboarding.
# Linux arm64 Rust 1.94. Standalone workspace. No cargo update. No product crate build.
set -u
echo "oo_task=sp11-pg-onboarding-cli-postedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a261"
echo "oo_platform=linux/arm64"
echo "phase=postedit"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/onboarding
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/postedit/phases /target/onboarding
cd /onboarding

record() {
  mkdir -p /evidence/linux/postedit/phases
  printf '%s\n' "$2" > "/evidence/linux/postedit/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/postedit/postedit.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/postedit/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0
printf '%s\n' "no" > /evidence/linux/postedit/cargo-update-invoked.txt

if [ -f Cargo.lock ]; then
  echo "lock=present" > /evidence/linux/postedit/lock-before.txt
else
  echo "=== cargo generate-lockfile ==="
  set +e
  cargo generate-lockfile --manifest-path Cargo.toml > /evidence/linux/postedit/generate-lockfile.log 2>&1
  lock_exit=$?
  set -e
  record generate-lockfile "$lock_exit"
  echo "lock=generated" > /evidence/linux/postedit/lock-before.txt
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/postedit/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/linux/postedit/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/postedit/clippy-component.log 2>&1
component_exit=$?
set -e
record clippy-component "$component_exit"

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/postedit/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

if grep -q 'cargo update' /evidence/linux/postedit/fetch.log /evidence/linux/postedit/test.log /evidence/linux/postedit/clippy.log; then
  echo "cargo update text appeared in a log" >&2
  record cargo-update-text 1
fi
record cargo-update-text 0

printf '%s\n' 0 > /evidence/linux/postedit/postedit.exit
echo "postedit=ok"
exit 0
