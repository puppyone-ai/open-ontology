#!/bin/bash
# Focused tests and clippy for cdc-quarantine on the fixed archive plus this crate.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
cargo -V
uname -a
id
if ! rustc -vV | grep -q 'release: 1.94.0'; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit
if ! cmp -s tools/cdc-transaction/src/sha256.rs /blobs/cdc-transaction-sha256.rs \
  || ! cmp -s tools/cdc-quarantine/src/sha256.rs /blobs/cdc-transaction-sha256.rs \
  || ! cmp -s tools/cdc-transaction/Cargo.lock /blobs/cdc-transaction-Cargo.lock
then
  echo "blob mismatch" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
find tools/cdc-quarantine -type l -print > /evidence/symlinks.txt
if [ -s /evidence/symlinks.txt ]; then
  echo "symlink in quarantine tree" >&2
  exit 2
fi

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path tools/cdc-quarantine/Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path tools/cdc-quarantine/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
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
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test cdc-quarantine ==="
set +e
cargo test --manifest-path tools/cdc-quarantine/Cargo.toml --locked --offline --all-targets -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
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

echo "=== cargo clippy cdc-quarantine ==="
set +e
cargo clippy --manifest-path tools/cdc-quarantine/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
