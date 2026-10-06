#!/bin/bash
# Focused session-transition tests and clippy on the sealed crate copy.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
uname -a
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

rm -rf /work/src /work/tests /work/Cargo.toml /work/Cargo.lock /work/CONTRACT.md
mkdir -p /work
cp -a /src/Cargo.toml /src/CONTRACT.md /src/src /src/tests /work/
if [ -f /src/Cargo.lock ]; then
  cp -a /src/Cargo.lock /work/Cargo.lock
fi
cd /work
find Cargo.toml CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt

if [ ! -f Cargo.lock ]; then
  echo "=== cargo generate-lockfile ==="
  set +e
  cargo generate-lockfile > /evidence/lock.stdout 2> /evidence/lock.stderr
  lock_exit=$?
  set -e
  printf '%s\n' "$lock_exit" > /evidence/lock.exit
  echo "PHASE lock_exit=$lock_exit"
  if [ "$lock_exit" -ne 0 ]; then
    printf '%s\n' not_run > /evidence/fetch.exit
    printf '%s\n' not_run > /evidence/test.exit
    printf '%s\n' not_run > /evidence/clippy-component.exit
    printf '%s\n' not_run > /evidence/clippy.exit
    exit "$lock_exit"
  fi
else
  printf '%s\n' present > /evidence/lock.exit
fi
cp Cargo.lock /evidence/Cargo.lock

echo "=== cargo fetch locked ==="
set +e
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test locked offline ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$test_exit" -ne 0 ]; then
    exit "$test_exit"
  fi
  exit "$component_exit"
fi

echo "=== cargo clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
