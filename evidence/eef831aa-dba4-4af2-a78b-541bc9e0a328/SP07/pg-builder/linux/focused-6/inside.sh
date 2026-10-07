#!/bin/bash
# Focused Linux arm64 Rust 1.94 test of the owned builder and fixture.
# The tree at /src is the fixed archive plus the owned overlay, mounted read-only.
# This script does not invoke the update subcommand and does not edit the root lock.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/pg-builder
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
mkdir -p /cargo-home/tmp /cargo-home/bin /evidence/linux/focused-6/phases /evidence/linux/focused-6/locks
printf '%s\n' no > /evidence/linux/focused-6/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/linux/focused-6/phases/$1.exit"
  echo "PHASE $1=$2"
}

echo "=== identity ==="
rustc -vV | tee /evidence/linux/focused-6/rustc-version.txt
cargo -V | tee /evidence/linux/focused-6/cargo-version.txt
uname -a | tee /evidence/linux/focused-6/uname.txt
id

echo "=== copy archive to writable work tree ==="
rm -rf /work
mkdir -p /work
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
cd /work
if [ ! -f tools/pg-builder/src/lib.rs ] || [ ! -f testing/real-db/pg-builder/src/lib.rs ]; then
  echo "owned overlay missing from the mounted tree" >&2
  record overlay 2
  exit 2
fi
if [ -d tools/pg-builder/target ] || [ -d testing/real-db/pg-builder/target ]; then
  echo "overlay brought a target directory" >&2
  record overlay 2
  exit 2
fi
record overlay 0
sha256sum tools/pg-builder/Cargo.toml tools/pg-builder/CONTRACT.md tools/pg-builder/src/*.rs \
  tools/pg-builder/tests/contract.rs tools/pg-builder/tests/common/mod.rs \
  testing/real-db/pg-builder/Cargo.toml testing/real-db/pg-builder/src/lib.rs \
  testing/real-db/pg-builder/src/main.rs \
  > /evidence/linux/focused-6/overlay-sha256.txt

echo "=== lock subset tool ==="
set +e
rustc --edition 2021 -D warnings /evidence/linux/focused-6/lock_subset.rs -o /cargo-home/bin/lock-subset \
  > /evidence/linux/focused-6/lock-subset-tool.log 2>&1
tool_exit=$?
set -e
record lock-subset-tool "$tool_exit"
if [ "$tool_exit" -ne 0 ]; then
  exit "$tool_exit"
fi

echo "=== builder lock from the fixed root lock ==="
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/focused-6/root-lock-before.sha256
rm -f tools/pg-builder/Cargo.lock
cp Cargo.lock tools/pg-builder/Cargo.lock
set +e
cargo fetch --manifest-path tools/pg-builder/Cargo.toml > /evidence/linux/focused-6/fetch-builder.log 2>&1
fetch_exit=$?
set -e
record fetch-builder "$fetch_exit"
cp tools/pg-builder/Cargo.lock /evidence/linux/focused-6/locks/pg-builder-Cargo.lock
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
sha256sum Cargo.lock | awk '{print $1}' > /evidence/linux/focused-6/root-lock-after.sha256
if ! cmp -s /evidence/linux/focused-6/root-lock-before.sha256 /evidence/linux/focused-6/root-lock-after.sha256; then
  echo "root lock bytes changed" >&2
  record root-lock 1
  exit 1
fi
record root-lock 0

echo "=== builder registry subset ==="
set +e
/cargo-home/bin/lock-subset Cargo.lock tools/pg-builder/Cargo.lock \
  > /evidence/linux/focused-6/lock-subset-builder.txt 2> /evidence/linux/focused-6/lock-subset-builder.err
subset_exit=$?
set -e
record lock-subset-builder "$subset_exit"
if [ "$subset_exit" -ne 0 ]; then
  exit "$subset_exit"
fi

echo "=== builder fetch locked offline ==="
set +e
cargo fetch --manifest-path tools/pg-builder/Cargo.toml --locked --offline \
  > /evidence/linux/focused-6/fetch-builder-offline.log 2>&1
offline_exit=$?
set -e
record fetch-builder-offline "$offline_exit"
if [ "$offline_exit" -ne 0 ]; then
  exit "$offline_exit"
fi

echo "=== builder tests ==="
set +e
cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- --test-threads=1 \
  > /evidence/linux/focused-6/test-builder.log 2>&1
test_exit=$?
set -e
record test-builder "$test_exit"
echo "PHASE test_builder=$test_exit"

echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/focused-6/clippy-component.log 2>&1
component_exit=$?
set -e
record clippy-component "$component_exit"
if [ "$component_exit" -ne 0 ]; then
  exit "$component_exit"
fi
rustup component list --toolchain 1.94.0 --installed | grep '^clippy-' > /evidence/linux/focused-6/clippy-component.txt

echo "=== builder clippy ==="
set +e
cargo clippy --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/focused-6/clippy-builder.log 2>&1
clippy_exit=$?
set -e
record clippy-builder "$clippy_exit"
echo "PHASE clippy_builder=$clippy_exit"

echo "=== fixture lock and tests ==="
export CARGO_TARGET_DIR=/target/pg-fixture
rm -f testing/real-db/pg-builder/Cargo.lock
set +e
cargo generate-lockfile --manifest-path testing/real-db/pg-builder/Cargo.toml --offline \
  > /evidence/linux/focused-6/lockgen-fixture.log 2>&1
fixture_lock_exit=$?
set -e
record lockgen-fixture "$fixture_lock_exit"
if [ "$fixture_lock_exit" -ne 0 ]; then
  exit "$fixture_lock_exit"
fi
cp testing/real-db/pg-builder/Cargo.lock /evidence/linux/focused-6/locks/pg-fixture-Cargo.lock
set +e
/cargo-home/bin/lock-subset Cargo.lock testing/real-db/pg-builder/Cargo.lock \
  > /evidence/linux/focused-6/lock-subset-fixture.txt 2> /evidence/linux/focused-6/lock-subset-fixture.err
fixture_subset_exit=$?
set -e
record lock-subset-fixture "$fixture_subset_exit"
set +e
cargo test --manifest-path testing/real-db/pg-builder/Cargo.toml --locked --offline --all-targets \
  > /evidence/linux/focused-6/test-fixture.log 2>&1
fixture_test_exit=$?
set -e
record test-fixture "$fixture_test_exit"
set +e
cargo clippy --manifest-path testing/real-db/pg-builder/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/focused-6/clippy-fixture.log 2>&1
fixture_clippy_exit=$?
set -e
record clippy-fixture "$fixture_clippy_exit"

worst=0
for code in "$test_exit" "$clippy_exit" "$fixture_subset_exit" "$fixture_test_exit" "$fixture_clippy_exit"; do
  if [ "$code" -ne 0 ]; then
    worst=$code
  fi
done
echo "worst_phase_exit=$worst"
exit "$worst"
