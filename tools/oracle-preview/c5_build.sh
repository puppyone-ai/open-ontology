#!/bin/bash
# Linux rustc 1.94 build of source-driver-worker only.
# No CARGO_BUILD_JOBS and no CPU quota. Does not build oo-core, oo-builder, or oo-pipeline.
set -u
mkdir -p /out
exec > >(tee -a /out/build.log) 2>&1
echo "build_start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
unset CARGO_BUILD_JOBS
export PATH="/usr/local/cargo/bin:${PATH}"
# The repo root rust-toolchain.toml asks for 1.98.1. This card stays on 1.94.
export RUSTUP_TOOLCHAIN=1.94.0
if [ -d /opt/oo-cargo/registry ]; then
  rm -rf /usr/local/cargo/registry
  ln -s /opt/oo-cargo/registry /usr/local/cargo/registry
fi
echo "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS-<unset>}"
echo "PATH=${PATH}"
command -v rustc
command -v cargo
rustc --version
cargo --version
nproc
if [ -r /sys/fs/cgroup/cpu.max ]; then echo "cpu.max=$(cat /sys/fs/cgroup/cpu.max)"; fi
if [ -r /sys/fs/cgroup/cpu/cpu.cfs_quota_us ]; then echo "cpu.cfs_quota_us=$(cat /sys/fs/cgroup/cpu/cpu.cfs_quota_us)"; fi
cd /src/tools/source-driver-worker
echo "STEP rustup run 1.94.0"
rustup run 1.94.0 rustc --version
echo "rustup_show $(rustup show active-toolchain)"
case "$(rustup run 1.94.0 rustc --version)" in
  "rustc 1.94.0 "*) ;;
  *) echo "rustc_not_1_94"; exit 4 ;;
esac
oo_cargo() {
  rustup run 1.94.0 cargo "$@"
}
sha256sum Cargo.lock | tee /out/cargo-lock.before.sha256
test_exit=1
echo "STEP test offline"
if oo_cargo test -p source-driver-worker --locked --offline --all-targets -- --test-threads=8; then
  test_exit=0
else
  echo "STEP test offline failed, retry online"
  if oo_cargo test -p source-driver-worker --locked --all-targets -- --test-threads=8; then
    test_exit=0
  fi
fi
echo "test_exit ${test_exit}"
if grep -E 'Compiling (oo-core|oo-builder|oo-pipeline|source-semantics) ' /out/build.log; then
  echo "foreign_compile"
  exit 3
fi
clippy_exit=1
if ! rustup component list --toolchain 1.94.0 --installed | grep -q clippy; then
  echo "STEP clippy component add 1.94.0"
  rustup component add clippy --toolchain 1.94.0 || true
fi
if rustup component list --toolchain 1.94.0 --installed | grep -q clippy; then
  echo "STEP clippy offline"
  if oo_cargo clippy -p source-driver-worker --locked --offline --all-targets -- -D warnings; then
    clippy_exit=0
  else
    echo "STEP clippy offline failed, retry online"
    if oo_cargo clippy -p source-driver-worker --locked --all-targets -- -D warnings; then
      clippy_exit=0
    fi
  fi
else
  echo "clippy_component_absent"
  clippy_exit=127
fi
echo "clippy_exit ${clippy_exit}"
build_exit=1
echo "STEP build offline"
if oo_cargo build -p source-driver-worker --locked --offline --bins; then
  build_exit=0
else
  echo "STEP build offline failed, retry online"
  if oo_cargo build -p source-driver-worker --locked --bins; then
    build_exit=0
  fi
fi
echo "build_exit ${build_exit}"
bin="${CARGO_TARGET_DIR}/debug/oo-c5-arrow"
if [ -x "$bin" ]; then
  sha256sum "$bin" | tee /out/oo-c5-arrow.sha256
  file "$bin" | tee /out/oo-c5-arrow.file
fi
sha256sum Cargo.lock | tee /out/cargo-lock.after.sha256
echo "build_end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
if [ "${test_exit}" -ne 0 ] || [ "${build_exit}" -ne 0 ]; then
  exit 1
fi
if [ "${clippy_exit}" -ne 0 ] && [ "${clippy_exit}" -ne 127 ]; then
  exit 1
fi
exit 0
