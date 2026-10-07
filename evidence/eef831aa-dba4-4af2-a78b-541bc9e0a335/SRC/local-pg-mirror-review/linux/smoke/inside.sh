#!/bin/bash
# Pre-overlay identity smoke for the review fixed commit.
# The archive already contains tools/local-pg-mirror. This script does not compile it.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
uname -a
id

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

if [ ! -f /src/tools/local-pg-mirror/src/lib.rs ] || [ ! -f /src/tools/local-pg-mirror/Cargo.lock ] || [ ! -f /src/tools/local-pg-mirror/tests/live.rs ]; then
  echo "fixed archive is missing tools/local-pg-mirror" >&2
  printf '%s\n' 2 > /evidence/tool-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/tool-present.exit

echo "=== probe ==="
set +e
rustc --edition 2024 -D warnings -o /target/a335-probe /probe.rs > /evidence/probe-rustc.stdout 2> /evidence/probe-rustc.stderr
probe_rustc=$?
printf '%s\n' "$probe_rustc" > /evidence/probe-rustc.exit
echo "PHASE probe_rustc_exit=$probe_rustc"
if [ "$probe_rustc" -ne 0 ]; then
  exit "$probe_rustc"
fi
/target/a335-probe > /evidence/probe.stdout 2> /evidence/probe.stderr
probe_exit=$?
printf '%s\n' "$probe_exit" > /evidence/probe.exit
echo "PHASE probe_exit=$probe_exit"
if [ "$probe_exit" -ne 0 ]; then
  exit "$probe_exit"
fi
grep -q '^probe_ok$' /evidence/probe.stdout
echo "probe_ok"
exit 0
