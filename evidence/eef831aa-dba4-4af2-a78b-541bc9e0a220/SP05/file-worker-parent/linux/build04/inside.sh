#!/bin/bash
# Fixed-archive smoke, then this task's overlay, then the parent test and Clippy.
# Rust 1.94, Linux arm64. No PostgreSQL. No Python.
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

echo "=== archive pins before overlay ==="
if [ -f crates/oo-connectors/src/file_worker_parent.rs ] \
  || [ -f crates/oo-connectors/tests/file_worker_parent.rs ] \
  || [ -f contracts/connectors/file-worker-parent-v1.md ]
then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
sha256sum \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/src/bin/oo-file-parser-worker.rs \
  crates/oo-connectors/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/archive-sha256.txt
echo "PHASE archive_pins=0"

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/smoke.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test file_parser_worker before overlay ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test file_parser_worker -- --test-threads=1 \
  > /evidence/smoke.stdout 2> /evidence/smoke.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke.exit
echo "PHASE smoke_exit=$smoke_exit"
if [ "$smoke_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$smoke_exit"
fi

echo "=== copy overlay ==="
cp /overlay/file_worker_parent.rs crates/oo-connectors/src/file_worker_parent.rs
cp /overlay/lib.rs crates/oo-connectors/src/lib.rs
cp /overlay/file_worker_parent.rs.test crates/oo-connectors/tests/file_worker_parent.rs
cp /overlay/file-worker-parent-v1.md contracts/connectors/file-worker-parent-v1.md
sha256sum \
  crates/oo-connectors/src/file_worker_parent.rs \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/tests/file_worker_parent.rs \
  contracts/connectors/file-worker-parent-v1.md \
  > /evidence/overlay-sha256.txt
if ! cmp -s crates/oo-connectors/src/file_worker_parent.rs /overlay/file_worker_parent.rs \
  || ! cmp -s crates/oo-connectors/src/lib.rs /overlay/lib.rs \
  || ! cmp -s crates/oo-connectors/tests/file_worker_parent.rs /overlay/file_worker_parent.rs.test \
  || ! cmp -s contracts/connectors/file-worker-parent-v1.md /overlay/file-worker-parent-v1.md
then
  echo "overlay copy drifted" >&2
  printf '%s\n' 2 > /evidence/overlay-copy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-copy.exit
echo "PHASE overlay_copy_exit=0"

echo "=== cargo test file_worker_parent ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test file_worker_parent -- --test-threads=1 \
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
  exit "$component_exit"
fi

echo "=== cargo clippy file worker parent ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib --test file_worker_parent -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
