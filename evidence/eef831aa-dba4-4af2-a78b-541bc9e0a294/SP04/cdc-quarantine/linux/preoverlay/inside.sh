#!/bin/bash
# Linux arm64 Rust 1.94 smoke of cdc-transaction at c8588faa. No quarantine overlay.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true
if ! rustc -vV | grep -q 'release: 1.94.0'; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

echo "=== source pins ==="
sha256sum \
  tools/cdc-transaction/CONTRACT.md \
  tools/cdc-transaction/Cargo.toml \
  tools/cdc-transaction/Cargo.lock \
  tools/cdc-transaction/src/sha256.rs \
  > /evidence/archive-sha256.txt
if ! cmp -s tools/cdc-transaction/CONTRACT.md /blobs/cdc-transaction-CONTRACT.md \
  || ! cmp -s tools/cdc-transaction/Cargo.toml /blobs/cdc-transaction-Cargo.toml \
  || ! cmp -s tools/cdc-transaction/Cargo.lock /blobs/cdc-transaction-Cargo.lock \
  || ! cmp -s tools/cdc-transaction/src/sha256.rs /blobs/cdc-transaction-sha256.rs
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -e tools/cdc-quarantine ]; then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
if ! sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo "archive hash mismatch" >&2
  printf '%s\n' 2 > /evidence/hash-compare.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/hash-compare.exit

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path tools/cdc-transaction/Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path tools/cdc-transaction/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' skipped_foreign_crate > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test cdc-transaction ==="
set +e
cargo test --manifest-path tools/cdc-transaction/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
printf '%s\n' skipped_foreign_crate > /evidence/clippy.exit
echo "PHASE clippy_skipped_foreign_crate"
exit "$test_exit"
