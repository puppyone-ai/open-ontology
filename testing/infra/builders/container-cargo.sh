#!/bin/bash
# Runs inside the pinned Rust image. The workspace mount is the frozen snapshot.
# Target and cargo home are separate writable volumes. This is not an airgap proof.
set -euo pipefail

: "${OO_TOOLCHAIN:?}"
: "${OO_PLATFORM:?}"
: "${OO_ENGINE_ARCH:?}"

echo "oo_toolchain=$OO_TOOLCHAIN"
echo "oo_platform=$OO_PLATFORM"
echo "oo_engine_arch=$OO_ENGINE_ARCH"
echo "performance_class=functional"
echo "network_phase=cargo_fetch_online"
echo "offline_phase=build_test_clippy"
echo "airgap_claim=no"
echo "offline_switch=cargo_cli"
echo "build_network_internal=no"
echo "container_rootfs_writable=yes"
echo "component_download_outside_image_digest=yes"
if [ "$OO_PLATFORM" = "linux/amd64" ] && [ "$OO_ENGINE_ARCH" != "x86_64" ]; then
  echo "execution=emulated"
else
  echo "execution=native_to_engine"
fi

script_hash=$(sha256sum /src/testing/infra/builders/container-cargo.sh | awk '{print $1}')
echo "container_cargo_sha256=$script_hash"
pins_hash=$(sha256sum /src/testing/infra/images.pins | awk '{print $1}')
echo "images_pins_sha256=$pins_hash"

if [ "$OO_TOOLCHAIN" = "1.94.0" ]; then
  # rust-toolchain.toml in the snapshot pins 1.98.1. RUSTUP_TOOLCHAIN wins over that file.
  export RUSTUP_TOOLCHAIN=1.94.0
  echo "msrv_override=RUSTUP_TOOLCHAIN=1.94.0"
  echo "snapshot_rust_toolchain_channel=1.98.1"
  echo "rustup_component_download=clippy_1.94.0"
  rustup component add clippy --toolchain 1.94.0
else
  echo "msrv_override=none"
  echo "rustup_component_download=rust_toolchain_toml_rustfmt_clippy"
fi

echo "=== identity ==="
rustc -Vv
rustup show
uname -a
cat /etc/os-release
if [ -r /proc/cpuinfo ]; then
  awk 'NR<=8 { print }' /proc/cpuinfo
fi

got=$(rustc --version)
case "$OO_TOOLCHAIN" in
  1.98.1) echo "$got" | grep -q 'rustc 1.98.1 ' ;;
  1.94.0) echo "$got" | grep -q 'rustc 1.94.0 ' ;;
  *) echo "refusing unknown toolchain request" >&2; exit 2 ;;
esac

cd /src
worst=0
run_phase() {
  local name=$1
  shift
  echo "=== $name ==="
  set +e
  "$@"
  local status=$?
  set -e
  echo "phase_exit=$status phase=$name"
  if [ "$status" -ne 0 ]; then
    worst=$status
  fi
}
run_phase "cargo fetch --locked" cargo fetch --locked
run_phase "cargo build --workspace --locked --offline" cargo build --workspace --locked --offline
run_phase "cargo test --workspace --locked --offline" cargo test --workspace --locked --offline
run_phase "cargo clippy --workspace --all-targets --locked --offline -- -D warnings" cargo clippy --workspace --all-targets --locked --offline -- -D warnings
echo "=== done ==="
echo "elapsed_seconds=$SECONDS"
echo "worst_phase_exit=$worst"
exit "$worst"
