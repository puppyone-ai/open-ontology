#!/bin/bash
# Focused review tests and clippy on the fixed archive plus review_proof.rs.
set -u
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
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

files="
CONTRACT.md
Cargo.lock
Cargo.toml
src/jsonline.rs
src/lib.rs
src/main.rs
src/model.rs
src/pathutil.rs
src/sha256.rs
src/store.rs
tests/common/mod.rs
tests/common/oracle.rs
tests/proof.rs
"
blob_ok=0
for rel in $files; do
  if ! cmp -s "tools/cdc-quarantine/$rel" "/blobs/tools/cdc-quarantine/$rel"; then
    echo "blob mismatch $rel" >&2
    blob_ok=2
  fi
done
printf '%s\n' "$blob_ok" > /evidence/blob-cmp.exit
if [ "$blob_ok" -ne 0 ]; then
  exit 2
fi
if ! cmp -s tools/cdc-quarantine/tests/review_proof.rs /overlay/review_proof.rs; then
  echo "review overlay drifted" >&2
  printf '%s\n' 2 > /evidence/overlay-cmp.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-cmp.exit
find tools/cdc-quarantine -type l -print > /evidence/symlinks.txt
if [ -s /evidence/symlinks.txt ]; then
  echo "symlink in quarantine tree" >&2
  exit 2
fi

echo "=== lock subset ==="
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
set +e
/usr/local/bin/lock_subset /src/Cargo.lock /src/tools/cdc-quarantine/Cargo.lock > /evidence/lock-subset.txt 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$lock_exit"
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

echo "=== cargo test review_proof ==="
set +e
cargo test --manifest-path tools/cdc-quarantine/Cargo.toml --locked --offline --test review_proof -- --test-threads=1 \
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
