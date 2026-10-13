#!/bin/bash
# Compile the StarRocks preflight on Linux Rust 1.94. Jobs stay at 1.
set -u
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/driver
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
mkdir -p /evidence/build /cargo-home /target/driver
cd /src
rustup run 1.94.0 rustc -vV > /evidence/build/rustc-vv.txt
if ! grep -q '^release: 1.94.0$' /evidence/build/rustc-vv.txt; then
  printf '91\n' > /evidence/build/build.exit
  exit 91
fi
set +e
if [ ! -f Cargo.lock ]; then
  rustup run 1.94.0 cargo generate-lockfile > /evidence/build/generate-lock.stdout 2> /evidence/build/generate-lock.stderr
  gen=$?
  printf '%s\n' "$gen" > /evidence/build/generate-lock.exit
  if [ "$gen" != "0" ]; then
    printf '%s\n' "$gen" > /evidence/build/build.exit
    exit "$gen"
  fi
fi
cp Cargo.lock /evidence/build/Cargo.lock
rm -f /evidence/build/lock-accepted /evidence/build/lock-rejected
touch /evidence/build/lock-ready
for _ in $(seq 1 180); do
  if [ -f /evidence/build/lock-accepted ]; then
    break
  fi
  if [ -f /evidence/build/lock-rejected ]; then
    printf '3\n' > /evidence/build/build.exit
    exit 3
  fi
  sleep 1
done
if [ ! -f /evidence/build/lock-accepted ]; then
  printf '4\n' > /evidence/build/build.exit
  exit 4
fi
rustup run 1.94.0 cargo test --offline --locked --jobs 1 --all-targets \
  > /evidence/build/test.stdout 2> /evidence/build/test.stderr
test_code=$?
printf '%s\n' "$test_code" > /evidence/build/test.exit
if [ "$test_code" != "0" ]; then
  printf '%s\n' "$test_code" > /evidence/build/build.exit
  exit "$test_code"
fi
if ! rustup run 1.94.0 cargo clippy --version >/dev/null 2>&1; then
  rustup component add clippy --toolchain 1.94.0 \
    > /evidence/build/clippy-component.stdout 2> /evidence/build/clippy-component.stderr
  component=$?
  printf '%s\n' "$component" > /evidence/build/clippy-component.exit
  if [ "$component" != "0" ]; then
    printf '%s\n' "$component" > /evidence/build/build.exit
    exit "$component"
  fi
fi
rustup run 1.94.0 cargo clippy --offline --locked --jobs 1 --all-targets -- -D warnings \
  > /evidence/build/clippy.stdout 2> /evidence/build/clippy.stderr
clippy_code=$?
printf '%s\n' "$clippy_code" > /evidence/build/clippy.exit
if [ "$clippy_code" != "0" ]; then
  printf '%s\n' "$clippy_code" > /evidence/build/build.exit
  exit "$clippy_code"
fi
rustup run 1.94.0 cargo build --offline --locked --jobs 1 \
  > /evidence/build/build.stdout 2> /evidence/build/build.stderr
build_code=$?
printf '%s\n' "$build_code" > /evidence/build/build.exit
if [ "$build_code" != "0" ]; then
  exit "$build_code"
fi
sha256sum /target/driver/debug/starrocks-probe /target/driver/debug/starrocks-decode > /evidence/build/binaries.sha256
exit 0
