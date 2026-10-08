#!/bin/bash
# Install Clippy in this private builder. The child lock was generated once
# (see failed-attempt-1). Fetch that lock, then test and Clippy stay locked
# and offline. Do not regenerate the lock and do not update crates.
set -u
cd /src
rustc --version | tee /evidence/linux/full-test/rustc.txt
cargo --version | tee -a /evidence/linux/full-test/rustc.txt
rustup component add clippy
rustup component list --installed > /evidence/linux/full-test/components.txt
if ! grep -E '^clippy' /evidence/linux/full-test/components.txt >/dev/null; then
  echo "clippy component missing"
  printf '%s\n' 1 > /evidence/linux/full-test/clippy.exit
  exit 1
fi
cargo fetch --locked
fetch=$?
printf '%s\n' "$fetch" > /evidence/linux/full-test/fetch.exit
if [ "$fetch" -ne 0 ]; then
  exit "$fetch"
fi
set +e
cargo test --locked --offline --jobs 1 -- --test-threads=1
test_code=$?
printf '%s\n' "$test_code" > /evidence/linux/full-test/test.exit
cargo clippy --locked --offline --all-targets --jobs 1 -- -D warnings
clippy_code=$?
printf '%s\n' "$clippy_code" > /evidence/linux/full-test/clippy.exit
if [ "$test_code" -ne 0 ]; then
  exit "$test_code"
fi
exit "$clippy_code"
