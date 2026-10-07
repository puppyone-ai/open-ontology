#!/bin/bash
# Pre-edit smoke, second launch. /src is the fixed archive, mounted read-only.
# /work is a bind mount and is not removed. Locks are not regenerated.
set -u
set -e
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/smoke
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
mkdir -p /cargo-home/tmp /evidence/phases
printf '%s\n' no > /evidence/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
uname -a | tee /evidence/uname.txt
id
rustc_exit=0
grep -q 'release: 1.94' /evidence/rustc-version.txt || rustc_exit=1
record toolchain "$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then
  exit "$rustc_exit"
fi

echo "=== copy archive into the mounted work tree ==="
find /work -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
cd /work
if [ ! -f crates/oo-builder/src/description_candidate.rs ]; then
  echo "archive copy missing description_candidate.rs" >&2
  record overlay 2
  exit 2
fi
record overlay 0
sha256sum Cargo.lock > /evidence/root-lock-before.sha256

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

echo "=== oo-builder description candidate API and CLI ==="
run_phase test-candidate cargo test --locked -p oo-builder --test description_candidate --test description_candidate_cli -- --test-threads=1

echo "=== description-review ==="
run_phase test-review cargo test --locked --manifest-path tools/description-review/Cargo.toml -- --test-threads=1

echo "=== description-pack ==="
run_phase test-pack cargo test --locked --manifest-path tools/description-pack/Cargo.toml -- --test-threads=1

echo "=== description-provider ==="
run_phase test-provider cargo test --locked --manifest-path tools/description-provider/Cargo.toml -- --test-threads=1

sha256sum Cargo.lock > /evidence/root-lock-after.sha256
if ! cmp -s /evidence/root-lock-before.sha256 /evidence/root-lock-after.sha256; then
  echo "root lock bytes changed" >&2
  record root-lock 1
  exit 1
fi
for lock in tools/description-review/Cargo.lock tools/description-pack/Cargo.lock tools/description-provider/Cargo.lock; do
  sha256sum "$lock" >> /evidence/child-locks.sha256
done
record root-lock 0
echo "SMOKE_OK"
