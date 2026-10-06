#!/bin/bash
# Linux arm64 Rust 1.94 focused tests, Clippy, and the oo-core position overlay.
# Fixed archive for oo-core. The product crate is copied with cp -R, not cp -a.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export TMPDIR=/work/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target/tool
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0

mkdir -p /work/tmp /evidence

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
id

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

echo "=== product copy ==="
rm -rf /work/cdc-transaction
mkdir -p /work/cdc-transaction
cp -R /tool/. /work/cdc-transaction/
cd /work/cdc-transaction
if [ ! -f Cargo.lock ] || [ ! -f src/lib.rs ] || [ ! -f src/main.rs ]; then
  echo "product crate is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if grep -q 'name = "session-transition"' Cargo.lock; then
  echo "lock still names session-transition" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if ! grep -F '4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' Cargo.lock >/dev/null; then
  echo "serde pin missing" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if ! grep -F 'c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14' Cargo.lock >/dev/null; then
  echo "serde_json pin missing" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
mkdir -p /evidence/source
cp -R Cargo.toml Cargo.lock CONTRACT.md src tests /evidence/source/
printf '%s\n' 0 > /evidence/source.exit

echo "=== fetch ==="
set +e
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
printf '%s\n' "$clippy_add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$clippy_add_exit"
if [ "$clippy_add_exit" -ne 0 ]; then
  exit "$clippy_add_exit"
fi

echo "=== test ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

echo "=== position overlay ==="
if [ -e /core/crates/oo-core/tests/cdc_d01_position.rs ]; then
  echo "fixed archive already contains the overlay test" >&2
  printf '%s\n' 2 > /evidence/position.exit
  exit 2
fi
if [ ! -f /core/crates/oo-core/src/lib.rs ] || [ ! -f /core/Cargo.lock ]; then
  echo "fixed core archive is incomplete" >&2
  printf '%s\n' 2 > /evidence/position.exit
  exit 2
fi
rm -rf /work/core
mkdir -p /work/core
cp -R /core/. /work/core/
mkdir -p /work/core/crates/oo-core/tests
cp /oracle/cdc_d01_position.rs /work/core/crates/oo-core/tests/cdc_d01_position.rs
sha256sum /oracle/cdc_d01_position.rs > /evidence/oracle-sha256.txt
cd /work/core
export CARGO_TARGET_DIR=/target/core
set +e
cargo test --locked -p oo-core --test cdc_d01_position --no-run > /evidence/position-fetch.stdout 2> /evidence/position-fetch.stderr
position_fetch_exit=$?
printf '%s\n' "$position_fetch_exit" > /evidence/position-fetch.exit
echo "PHASE position_fetch_exit=$position_fetch_exit"
if [ "$position_fetch_exit" -ne 0 ]; then
  printf '%s\n' "$position_fetch_exit" > /evidence/position.exit
  exit "$position_fetch_exit"
fi
set +e
cargo test --locked --offline -p oo-core --test cdc_d01_position -- --test-threads=1 \
  > /evidence/position.stdout 2> /evidence/position.stderr
position_exit=$?
printf '%s\n' "$position_exit" > /evidence/position.exit
echo "PHASE position_exit=$position_exit"
exit "$position_exit"
