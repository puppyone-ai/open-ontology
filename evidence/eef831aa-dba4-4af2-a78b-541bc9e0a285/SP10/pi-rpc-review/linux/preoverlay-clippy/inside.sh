#!/bin/bash
# Install clippy on the private bridge, then run the real cargo clippy.
# Do not invent exit 127. The archive is the same pre-overlay spike. No source edit.
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
echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /proof/clippy.stdout 2> /proof/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /proof/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
exit "$clippy_exit"
