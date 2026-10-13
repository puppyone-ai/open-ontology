#!/bin/bash
# Build the fixed mirror binary and the acceptance judge. No mirror test suite. No clippy of the mirror.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export TMPDIR=/target/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target/tool
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_RELEASE_DEBUG=0
mkdir -p /target/tmp /evidence
cd /src

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit
git -C /src rev-parse HEAD > /evidence/repo-head.txt || true
git -C /src rev-parse 5df69049267bf16eca36d311a1b3082f27aa9754 > /evidence/fixed-commit.txt

echo "=== mirror build ==="
cd /src/tools/local-pg-mirror
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
cargo build --locked --offline --release --bin local-pg-mirror > /evidence/build.stdout 2> /evidence/build.stderr
build_exit=$?
printf '%s\n' "$build_exit" > /evidence/build.exit
echo "PHASE build=$build_exit"
if [ "$build_exit" -ne 0 ]; then
  exit "$build_exit"
fi
sha256sum /target/tool/release/local-pg-mirror > /evidence/binary.sha256

echo "=== acceptance judge ==="
rm -rf /target/acceptance-src
mkdir -p /target/acceptance-src
tar -C /src/testing/local-copy-acceptance -cf - --exclude target . | tar -C /target/acceptance-src -xf -
cd /target/acceptance-src
export CARGO_TARGET_DIR=/target/acceptance
cargo test --offline --all-targets > /evidence/acceptance-test.stdout 2> /evidence/acceptance-test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/acceptance-test.exit
echo "PHASE acceptance_test=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
fi
cargo clippy --offline --all-targets -- -D warnings > /evidence/acceptance-clippy.stdout 2> /evidence/acceptance-clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/acceptance-clippy.exit
echo "PHASE acceptance_clippy=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi
cargo build --offline --bin local-copy-acceptance > /evidence/acceptance-build.stdout 2> /evidence/acceptance-build.stderr
acc_exit=$?
printf '%s\n' "$acc_exit" > /evidence/acceptance-build.exit
cp Cargo.lock /evidence/acceptance-Cargo.lock
echo "PHASE acceptance_build=$acc_exit"
exit "$acc_exit"
