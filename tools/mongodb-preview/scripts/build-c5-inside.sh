#!/bin/bash
# Linux Rust 1.94 build of mongodb-preview. Uses the machine default job count.
set -euo pipefail
unset CARGO_BUILD_JOBS || true
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.94.0}"
cd /work/tools/mongodb-preview
mkdir -p /evidence
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
rustup show active-toolchain | tee /evidence/rustup-active.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt
nproc | tee /evidence/nproc.txt
if [[ -r /sys/fs/cgroup/cpu.max ]]; then
  cat /sys/fs/cgroup/cpu.max | tee /evidence/cpu.max
else
  echo "cpu.max missing" | tee /evidence/cpu.max
fi
if ! rustup component list --installed --toolchain 1.94.0 | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0
fi
rustup component list --installed | tee /evidence/rustup-components.txt
export CARGO_TARGET_DIR=/target
run_step() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.txt" 2>&1
  local status=$?
  set -e
  printf '%s\n' "$status" > "/evidence/${name}.exit"
  if [[ "$status" -ne 0 ]]; then
    exit "$status"
  fi
}
run_step cargo-clippy cargo clippy --locked --all-targets -- -D warnings
run_step cargo-test cargo test --locked --all-targets
run_step cargo-build cargo build --locked --bins --lib
sha256sum /target/debug/mongodb-preview | tee /evidence/mongodb-preview.sha256
if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /target /evidence || true
fi
