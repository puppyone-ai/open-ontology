#!/bin/bash
# Offline build, then four real Pi runs: tool, abort, disconnect, restore.
# Network is none. The provider is local. This is not a sandbox certificate.
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
LIVE=/target/debug/pi-harness-live
fail=0
for mode in tool abort disconnect restore; do
  echo "=== ${mode} ==="
  mkdir -p "/proof/${mode}"
  set +e
  "$LIVE" "$mode" "$PI" "$NODE" "/proof/${mode}" > "/proof/${mode}.stdout" 2> "/proof/${mode}.stderr"
  mode_exit=$?
  set -e
  printf '%s\n' "$mode_exit" > "/proof/${mode}.exit"
  echo "PHASE ${mode}_exit=$mode_exit"
  if [ "$mode_exit" -ne 0 ]; then
    fail=1
  fi
done
printf '%s\n' "$fail" > /proof/live.exit
exit "$fail"
