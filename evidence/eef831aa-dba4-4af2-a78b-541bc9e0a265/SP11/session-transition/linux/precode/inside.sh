#!/bin/bash
# Pre-code Linux arm64 Rust 1.94 smoke. Fixed archive 567d621e. No product crate.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true

echo "=== absence ==="
if [ ! -f /src/tools/pg-onboarding/Cargo.toml ]; then
  echo "fixed archive missing pg-onboarding" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -e /src/tools/session-transition ]; then
  echo "session-transition present before code" >&2
  printf '%s\n' 2 > /evidence/absence.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/absence.exit
printf '%s\n' 0 > /evidence/archive.exit

echo "=== rustc version gate ==="
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

echo "=== compile probe ==="
set +e
rustc --edition 2024 -D warnings -o /usr/local/bin/oo-a265-msrv-probe /probe.rs \
  > /evidence/probe-rustc.stdout 2> /evidence/probe-rustc.stderr
probe_rustc_exit=$?
set -e
printf '%s\n' "$probe_rustc_exit" > /evidence/probe-rustc.exit
echo "PHASE probe_rustc_exit=$probe_rustc_exit"
if [ "$probe_rustc_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/probe-run.exit
  exit "$probe_rustc_exit"
fi
set +e
/usr/local/bin/oo-a265-msrv-probe > /evidence/probe-run.stdout 2> /evidence/probe-run.stderr
probe_run_exit=$?
set -e
printf '%s\n' "$probe_run_exit" > /evidence/probe-run.exit
echo "PHASE probe_run_exit=$probe_run_exit"
exit "$probe_run_exit"
