#!/bin/bash
# Focused Linux arm64 Rust 1.94 check of the delimited reader overlay.
# Archive is git HEAD plus the four owned files. No PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export OO_A192_ORACLE_DIR=/evidence/oracle
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
echo "=== overlay presence ==="
for path in \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/tests/delimited.rs \
  contracts/connectors/delimited-reader-v1.md \
  contracts/source/pgsql/empty-catalog.json
do
  if [ ! -f "$path" ]; then
    echo "missing $path" >&2
    echo 2 > /evidence/absence.exit
    exit 2
  fi
done
echo 0 > /evidence/absence.exit
echo "=== archive hashes ==="
sha256sum \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-connectors/src/lib.rs \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/jsonl.rs \
  crates/oo-connectors/tests/delimited.rs \
  crates/oo-connectors/Cargo.toml \
  contracts/connectors/delimited-reader-v1.md \
  contracts/source/pgsql/empty-catalog.json \
  > /evidence/archive-sha256.txt
if sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo 0 > /evidence/hash-compare.exit
else
  echo "archive hash mismatch" >&2
  echo 2 > /evidence/hash-compare.exit
  exit 2
fi
echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
echo "$offline_fetch_exit" > /evidence/fetch-offline.exit
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
echo "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "not_run" > /evidence/test-lib.exit
  echo "not_run" > /evidence/test-delimited.exit
  echo "not_run" > /evidence/clippy.exit
  echo "worst_phase_exit=$fetch_exit"
  exit "$fetch_exit"
fi
echo "=== cargo test oo-connectors lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib > /evidence/test-lib.stdout 2> /evidence/test-lib.stderr
lib_exit=$?
set -e
echo "$lib_exit" > /evidence/test-lib.exit
echo "PHASE test_lib_exit=$lib_exit"
echo "=== cargo test delimited ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test delimited > /evidence/test-delimited.stdout 2> /evidence/test-delimited.stderr
delim_exit=$?
set -e
echo "$delim_exit" > /evidence/test-delimited.exit
echo "PHASE test_delimited_exit=$delim_exit"
echo "=== clippy ==="
set +e
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
  component_exit=$?
else
  component_exit=0
  echo "clippy already installed" > /evidence/clippy-component.stdout
fi
echo "$component_exit" > /evidence/clippy-component.exit
if [ "$component_exit" -ne 0 ]; then
  echo "$component_exit" > /evidence/clippy.exit
  clippy_exit=$component_exit
else
  cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
  clippy_exit=$?
  echo "$clippy_exit" > /evidence/clippy.exit
fi
set -e
echo "PHASE clippy_exit=$clippy_exit"
worst=0
for code in "$lib_exit" "$delim_exit" "$clippy_exit"; do
  if [ "$code" -ne 0 ]; then
    worst=$code
    break
  fi
done
echo "worst_phase_exit=$worst"
exit "$worst"
