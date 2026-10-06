#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of the fixed session-transition crate.
# session-store must be absent. No clippy and no PostgreSQL in this phase.
set -u
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
id

echo "=== absence ==="
if [ ! -f /src/tools/session-transition/src/reduce.rs ]; then
  echo "fixed archive missing the reducer" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -e /src/tools/session-store ]; then
  echo "session-store present before code" >&2
  printf '%s\n' 2 > /evidence/absence.exit
  exit 2
fi
if [ -f /src/rust-toolchain.toml ]; then
  echo "toolchain file must stay out of this archive" >&2
  printf '%s\n' 2 > /evidence/archive.exit
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

rm -rf /work/session-transition
mkdir -p /work
cp -R /src/tools/session-transition /work/session-transition
cd /work/session-transition
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt

echo "=== cargo fetch locked ==="
set +e
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  exit "$fetch_exit"
fi

echo "=== cargo test locked offline ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
exit "$test_exit"
