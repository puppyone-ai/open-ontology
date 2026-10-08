#!/bin/bash
# Focused locked test and clippy for tools/local-analysis. No cargo update.
set -o pipefail
set -u
export PATH="/usr/local/cargo/bin:/usr/local/bin:${PATH}"
export HOME=/tmp
export RUSTUP_HOME=/usr/local/rustup
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
cd /src/local-analysis
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
rustup component list --installed > /evidence/components.txt 2>&1 || true
find /usr/local/rustup /usr/local/cargo -name 'cargo-clippy' -o -name 'clippy-driver' > /evidence/clippy-bins.txt 2>/dev/null || true
cargo test --locked --offline --all-targets -- --test-threads=1 > /evidence/test.log 2>&1
test_rc=$?
printf '%s\n' "$test_rc" > /evidence/test.exit
if grep -q '^clippy' /evidence/components.txt 2>/dev/null || [ -s /evidence/clippy-bins.txt ]; then
  cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.log 2>&1
  clippy_rc=$?
else
  printf '%s\n' "clippy component is not installed in the pinned image; not fetching" > /evidence/clippy.log
  clippy_rc=127
fi
printf '%s\n' "$clippy_rc" > /evidence/clippy.exit
if [ "$test_rc" -ne 0 ]; then
  exit "$test_rc"
fi
if [ "$clippy_rc" -eq 127 ]; then
  exit 0
fi
exit "$clippy_rc"
