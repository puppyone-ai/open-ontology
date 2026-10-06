#!/bin/bash
# Independent boundary tests on a copy of the fixed archive plus one test file.
# Origin: a282 clippy-after-1/inside.sh. The product tree is not mounted.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
export CARGO_TERM_COLOR=never
cd /work
echo "=== identity ==="
rustc -vV
cargo -V
rustup show > /proof/toolchain.txt
rustup component list --installed > /proof/components-before.txt
if [ ! -f /work/tests/a296_boundary.rs ]; then
  echo "overlay test missing" > /proof/missing-overlay.txt
  exit 2
fi
if ! grep -q '^clippy' /proof/components-before.txt; then
  echo "=== install clippy ==="
  set +e
  rustup component add clippy --toolchain 1.94.0 > /proof/clippy-install.stdout 2> /proof/clippy-install.stderr
  install_exit=$?
  set -e
  printf '%s\n' "$install_exit" > /proof/clippy-install.exit
  echo "PHASE clippy_install_exit=$install_exit"
  if [ "$install_exit" -ne 0 ]; then
    echo "clippy component add failed; cargo clippy was not started" > /proof/clippy-not-run.txt
    exit "$install_exit"
  fi
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
rustup component list --installed > /proof/components-after.txt
if ! grep -q '^clippy' /proof/components-after.txt; then
  echo "clippy component absent; cargo clippy was not started" > /proof/clippy-not-run.txt
  printf '%s\n' 2 > /proof/clippy.exit
  if [ "$fail" -eq 0 ]; then
    fail=2
  fi
  exit "$fail"
fi
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /proof/clippy.stdout 2> /proof/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /proof/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$fail" -eq 0 ] && [ "$clippy_exit" -ne 0 ]; then
  fail=$clippy_exit
fi
exit "$fail"
