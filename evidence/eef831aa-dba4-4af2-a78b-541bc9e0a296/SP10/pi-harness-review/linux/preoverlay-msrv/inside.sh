#!/bin/bash
# Author suite on the fixed archive, then the independent registry oracle.
# Clippy is installed before cargo clippy. A missing component is not a pass.
# Origin: a282 clippy-after-1/inside.sh. Added: registry and source oracle.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
export CARGO_TERM_COLOR=never
cd /work
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show > /proof/toolchain.txt
rustc -vV >> /proof/toolchain.txt
rustup component list --installed > /proof/components-before.txt
echo "=== fetch ==="
set +e
cargo fetch --locked > /proof/fetch.stdout 2> /proof/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /proof/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== install clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /proof/clippy-install.stdout 2> /proof/clippy-install.stderr
install_exit=$?
set -e
printf '%s\n' "$install_exit" > /proof/clippy-install.exit
echo "PHASE clippy_install_exit=$install_exit"
rustup component list --installed > /proof/components-after.txt
if [ "$install_exit" -ne 0 ]; then
  echo "clippy component add failed; cargo clippy was not started" > /proof/clippy-not-run.txt
  exit "$install_exit"
fi
if ! grep -q '^clippy' /proof/components-after.txt; then
  echo "clippy component absent after a zero install status; cargo clippy was not started" > /proof/clippy-not-run.txt
  printf '%s\n' 2 > /proof/clippy.exit
  exit 2
fi
fail=0
echo "=== tests ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /proof/test.stdout 2> /proof/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /proof/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  fail=$test_exit
fi
echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /proof/clippy.stdout 2> /proof/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /proof/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$fail" -eq 0 ] && [ "$clippy_exit" -ne 0 ]; then
  fail=$clippy_exit
fi
echo "=== review oracle ==="
set +e
rustc --edition 2021 -O -o /usr/local/bin/review-oracle /oracle/review_oracle.rs > /proof/oracle-rustc.stdout 2> /proof/oracle-rustc.stderr
oracle_rustc_exit=$?
set -e
printf '%s\n' "$oracle_rustc_exit" > /proof/oracle-rustc.exit
echo "PHASE oracle_rustc_exit=$oracle_rustc_exit"
if [ "$oracle_rustc_exit" -ne 0 ]; then
  if [ "$fail" -eq 0 ]; then
    fail=1
  fi
else
  set +e
  /usr/local/bin/review-oracle registry /work/Cargo.lock /work/tests/fixtures/registry-triples.txt /root-lock/Cargo.lock /work/Cargo.toml > /proof/registry-oracle.stdout 2> /proof/registry-oracle.stderr
  registry_exit=$?
  /usr/local/bin/review-oracle source /work > /proof/source-oracle.stdout 2> /proof/source-oracle.stderr
  source_exit=$?
  set -e
  printf '%s\n' "$registry_exit" > /proof/registry-oracle.exit
  printf '%s\n' "$source_exit" > /proof/source-oracle.exit
  echo "PHASE registry_oracle_exit=$registry_exit source_oracle_exit=$source_exit"
  if [ "$fail" -eq 0 ] && [ "$registry_exit" -ne 0 ]; then
    fail=$registry_exit
  fi
  if [ "$fail" -eq 0 ] && [ "$source_exit" -ne 0 ]; then
    fail=$source_exit
  fi
fi
exit "$fail"
