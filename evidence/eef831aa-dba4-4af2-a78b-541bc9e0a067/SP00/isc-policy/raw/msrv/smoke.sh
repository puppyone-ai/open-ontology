#!/bin/bash
# Focused MSRV smoke. Fetch the locked graph, then check oo-core offline.
# This is not a workspace matrix, not clippy, and not an air-gap claim.
set -u
echo "oo_task=isc-policy-msrv-smoke"
echo "oo_source=f61de8d83c3923fee8d98915eac290471841b83f"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "network_phase=cargo_fetch_online"
echo "offline_phase=cargo_check_oo_core"
echo "airgap_claim=no"
echo "fetch_argv=cargo fetch --locked"
echo "check_argv=cargo check -p oo-core --locked --offline"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
echo "CARGO_HOME=$CARGO_HOME"
rustc -Vv
rustup show
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-core/Cargo.toml ]; then
  echo "snapshot missing cargo inputs" >&2
  exit 2
fi
echo "=== cargo fetch --locked ==="
set +e
cargo fetch --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== cargo check -p oo-core --locked --offline ==="
set +e
cargo check -p oo-core --locked --offline
check_exit=$?
set -e
echo "phase_exit=$check_exit phase=cargo_check_oo_core"
echo "elapsed_seconds=$SECONDS"
exit "$check_exit"
