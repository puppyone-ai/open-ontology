#!/bin/bash
# Linux arm64 Rust 1.94 focused tests and Clippy for cdc-spool.
# The product crate is copied with cp -R, not cp -a. No PostgreSQL.
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
export CDC_SPOOL_HWM_OUT=/evidence/scale-hwm.txt

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
rm -rf /work/cdc-spool
mkdir -p /work/cdc-spool
cp -R /tool/. /work/cdc-spool/
cd /work/cdc-spool
if [ ! -f Cargo.lock ] || [ ! -f src/lib.rs ] || [ ! -f src/main.rs ] || [ ! -f CONTRACT.md ]; then
  echo "product crate is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if ! grep -q 'name = "cdc-spool"' Cargo.lock; then
  echo "lock does not name cdc-spool" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if grep -q 'name = "cdc-transaction"' Cargo.lock; then
  echo "lock still names cdc-transaction" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if ! grep -F '4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba' Cargo.lock >/dev/null; then
  echo "serde pin missing" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if ! grep -F '67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48' Cargo.lock >/dev/null; then
  echo "serde_core pin missing" >&2
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
exit "$clippy_exit"
