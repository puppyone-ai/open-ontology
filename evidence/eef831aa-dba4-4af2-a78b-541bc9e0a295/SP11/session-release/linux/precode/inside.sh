#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of the fixed session-transition and session-store crates.
# session-release must be absent. No clippy and no PostgreSQL in this phase.
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
command -v bash || true
command -v kill || true

echo "=== absence ==="
if [ ! -f /src/tools/session-transition/src/reduce.rs ]; then
  echo "fixed archive missing the reducer" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/tools/session-store/src/store.rs ]; then
  echo "fixed archive missing session-store" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -e /src/tools/session-release ]; then
  echo "session-release present before code" >&2
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

rm -rf /work/tools
mkdir -p /work
cp -R /src/tools /work/tools
cd /work/tools/session-transition
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/reducer-sha256.txt
cd /work/tools/session-store
find Cargo.toml Cargo.lock CONTRACT.md migrations src tests -type f | sort | xargs sha256sum > /evidence/store-sha256.txt

echo "=== cargo fetch transition ==="
cd /work/tools/session-transition
set +e
cargo fetch --locked > /evidence/fetch-transition.stdout 2> /evidence/fetch-transition.stderr
fetch_t=$?
set -e
printf '%s\n' "$fetch_t" > /evidence/fetch-transition.exit
echo "PHASE fetch_transition_exit=$fetch_t"
if [ "$fetch_t" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test-transition.exit
  printf '%s\n' not_run > /evidence/test-store.exit
  exit "$fetch_t"
fi

echo "=== cargo test transition locked offline ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test-transition.stdout 2> /evidence/test-transition.stderr
test_t=$?
set -e
printf '%s\n' "$test_t" > /evidence/test-transition.exit
echo "PHASE test_transition_exit=$test_t"
if [ "$test_t" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test-store.exit
  exit "$test_t"
fi

echo "=== cargo fetch store ==="
cd /work/tools/session-store
set +e
cargo fetch --locked > /evidence/fetch-store.stdout 2> /evidence/fetch-store.stderr
fetch_s=$?
set -e
printf '%s\n' "$fetch_s" > /evidence/fetch-store.exit
echo "PHASE fetch_store_exit=$fetch_s"
if [ "$fetch_s" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test-store.exit
  exit "$fetch_s"
fi

echo "=== cargo test store lib locked offline ==="
set +e
cargo test --locked --offline --lib -- --test-threads=1 > /evidence/test-store.stdout 2> /evidence/test-store.stderr
test_s=$?
set -e
printf '%s\n' "$test_s" > /evidence/test-store.exit
echo "PHASE test_store_exit=$test_s"
if [ -e /work/tools/session-release ]; then
  echo "session-release appeared during smoke" >&2
  exit 2
fi
exit "$test_s"
