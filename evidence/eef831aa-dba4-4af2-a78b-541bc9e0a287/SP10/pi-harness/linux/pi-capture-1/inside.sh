#!/bin/bash
# Offline build of the capture probe, then one fresh Pi 1.0.4 tool turn.
# The probe records stdout. It is not the adapter event loop.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
export CARGO_TERM_COLOR=never
cd /work
echo "=== identity ==="
rustc -vV
cargo -V
echo "=== build ==="
set +e
cargo build --locked --offline --bin pi-harness-live --bin fixture-provider > /proof/build.stdout 2> /proof/build.stderr
build_exit=$?
set -e
printf '%s\n' "$build_exit" > /proof/build.exit
echo "PHASE build_exit=$build_exit"
if [ "$build_exit" -ne 0 ]; then
  exit "$build_exit"
fi
PI=$(readlink -f /opt/pi-prefix/install/node_modules/.bin/pi)
NODE=$(readlink -f /opt/pi-prefix/node/bin/node)
if [ -L "$PI" ] || [ -L "$NODE" ]; then
  echo "resolved path is still a symlink" > /proof/path-error.txt
  exit 2
fi
printf '%s\n' "$PI" > /proof/pi-path.txt
printf '%s\n' "$NODE" > /proof/node-path.txt
ls -l "$PI" "$NODE" > /proof/path-ls.txt
"$NODE" --version > /proof/node-version.txt
mkdir -p /proof/out
echo "=== capture ==="
set +e
/target/debug/pi-harness-live capture "$PI" "$NODE" /proof/out > /proof/capture.stdout 2> /proof/capture.stderr
cap_exit=$?
set -e
printf '%s\n' "$cap_exit" > /proof/capture.exit
echo "PHASE capture_exit=$cap_exit"
exit "$cap_exit"
