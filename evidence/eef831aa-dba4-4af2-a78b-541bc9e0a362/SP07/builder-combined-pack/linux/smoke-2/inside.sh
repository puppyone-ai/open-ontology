#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of the current pg-builder and description-pack.
# Recipe origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a357/SP07/pg-builder-review/linux/proof-4/inside.sh
# The archive is mounted read-only. Locks are not regenerated. PostgreSQL is not started.
# No model is called. cargo update is not invoked.
set -u
set -e
set -o pipefail
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/combined-smoke
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
mkdir -p /cargo-home/tmp /evidence/phases /work
printf '%s\n' no > /evidence/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

run_phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.log" 2>&1
  local status=$?
  set -e
  record "$name" "$status"
  if [ "$status" -ne 0 ]; then
    exit "$status"
  fi
}

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
uname -a | tee /evidence/uname.txt
id
rustc_exit=0
grep -q 'release: 1.94' /evidence/rustc-version.txt || rustc_exit=1
grep -q 'aarch64-unknown-linux-gnu' /evidence/rustc-version.txt || rustc_exit=1
record toolchain "$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then
  exit "$rustc_exit"
fi

echo "=== copy the pre-edit archive ==="
rm -rf /work
mkdir -p /work
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
printf '%s\n' moved-aside > /evidence/toolchain-aside.txt
cd /work
for lock in Cargo.lock tools/pg-builder/Cargo.lock tools/description-pack/Cargo.lock; do
  if ! cmp -s "/src/$lock" "/work/$lock"; then
    echo "lock differs from the archive: $lock" >&2
    record archive-locks 2
    exit 2
  fi
done
record archive-locks 0
sha256sum Cargo.lock > /evidence/root-lock-before.sha256
sha256sum tools/pg-builder/Cargo.lock tools/description-pack/Cargo.lock > /evidence/child-locks-before.sha256

echo "=== fetch the committed locks ==="
run_phase fetch-builder cargo fetch --manifest-path tools/pg-builder/Cargo.toml --locked
run_phase fetch-pack cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked
run_phase fetch-builder-offline cargo fetch --manifest-path tools/pg-builder/Cargo.toml --locked --offline
run_phase fetch-pack-offline cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked --offline

echo "=== existing tests ==="
run_phase test-builder cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- --test-threads=1
run_phase test-pack cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- --test-threads=1

echo "=== clippy component ==="
set +e
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  component_exit=0
  printf '%s\n' already_installed > /evidence/clippy-component.log
else
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
  component_exit=$?
fi
set -e
record clippy-component "$component_exit"
if [ "$component_exit" -ne 0 ]; then
  exit "$component_exit"
fi

echo "=== clippy ==="
run_phase clippy-builder cargo clippy --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- -D warnings
run_phase clippy-pack cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings

sha256sum Cargo.lock > /evidence/root-lock-after.sha256
sha256sum tools/pg-builder/Cargo.lock tools/description-pack/Cargo.lock > /evidence/child-locks-after.sha256
if ! cmp -s /evidence/root-lock-before.sha256 /evidence/root-lock-after.sha256; then
  echo "root lock changed" >&2
  record locks-unchanged 2
  exit 2
fi
if ! cmp -s /evidence/child-locks-before.sha256 /evidence/child-locks-after.sha256; then
  echo "child lock changed" >&2
  record locks-unchanged 2
  exit 2
fi
record locks-unchanged 0
echo PROOF_OK
