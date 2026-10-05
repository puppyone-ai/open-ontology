#!/bin/bash
# Overlay proof on fixed archive c2db9c43 plus the three owned init-pg files.
# Rust 1.94, locked, offline. One private PostgreSQL is already on the network.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc +1.94.0 -vV | tee /evidence/rustc-version.txt
echo "rustc_vv_exit=${PIPESTATUS[0]}"
cargo +1.94.0 -V | tee /evidence/cargo-version.txt
echo "cargo_v_exit=${PIPESTATUS[0]}"
uname -a | tee /evidence/uname.txt
printf '%s\n' no > /evidence/cargo-update-invoked.txt
if ! grep -q 'release: 1.94.0' /evidence/rustc-version.txt; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/rustc.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/rustc.exit

echo "=== overlay pins ==="
if [ ! -f crates/oo-pipeline/src/bin/init-pg.rs ] \
  || [ ! -f crates/oo-pipeline/tests/init_pg_cli.rs ] \
  || [ ! -f contracts/pipeline/init-pg-cli-v1.md ]
then
  echo "init-pg overlay missing" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
sha256sum \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/tests/init_pg_cli.rs \
  contracts/pipeline/init-pg-cli-v1.md \
  > /evidence/overlay-sha256.txt
sha256sum \
  crates/oo-pipeline/src/target_installer.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/base-sha256.txt
if ! cmp -s /evidence/base-sha256.txt /smoke-base-sha256.txt; then
  echo "base files differ from the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/base-match.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/base-match.exit
sha256sum Cargo.lock > /evidence/lock-before.txt

echo "=== cargo fetch locked offline ==="
set +e
cargo +1.94.0 fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' "$component_exit" > /evidence/clippy.exit
  exit "$component_exit"
fi
set +e
cargo +1.94.0 clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-pipeline --bin init-pg --test init_pg_cli -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

echo "=== cargo test init_pg_cli ==="
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline \
  -p oo-pipeline --test init_pg_cli -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== cargo test init_pg_cli ignored ==="
mkdir -p /tmp/a223-live-secrets /evidence/live
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline \
  -p oo-pipeline --test init_pg_cli -- --test-threads=1 --ignored init_pg_binary_on_private_postgres \
  > /evidence/live.stdout 2> /evidence/live.stderr
live_exit=$?
set -e
printf '%s\n' "$live_exit" > /evidence/live.exit
echo "PHASE live_exit=$live_exit"

sha256sum Cargo.lock > /evidence/lock-after.txt
if ! cmp -s /evidence/lock-before.txt /evidence/lock-after.txt; then
  echo "Cargo.lock changed during proof" >&2
  printf '%s\n' 2 > /evidence/lock-unchanged.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-unchanged.exit
echo "worst_phase_exit=$live_exit"
exit "$live_exit"
