#!/bin/bash
# Fetch the locked subset, run the adapter tests, then real cargo clippy.
# Do not treat a missing clippy binary as a pass.
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
echo "=== tests ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /proof/test.stdout 2> /proof/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /proof/test.exit
echo "PHASE test_exit=$test_exit"
echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /proof/clippy.stdout 2> /proof/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /proof/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
