#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 smoke of fixed commit 6e1e6e57.
# The review harness must be absent. Clippy waits for the overlay.
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

echo "=== archive pins ==="
if [ -f crates/oo-pipeline/tests/init_pg_a232_review.rs ]; then
  echo "review overlay present during smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
if [ ! -f crates/oo-pipeline/src/bin/init-pg.rs ] \
  || [ ! -f crates/oo-pipeline/tests/init_pg_cli.rs ] \
  || [ ! -f contracts/pipeline/init-pg-cli-v1.md ]
then
  echo "frozen init-pg files are absent" >&2
  printf '%s\n' 2 > /evidence/product-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/product-present.exit
sha256sum \
  contracts/pipeline/init-pg-cli-v1.md \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/tests/init_pg_cli.rs \
  > /evidence/product-sha256.txt
if ! cmp -s /evidence/product-sha256.txt /expected-product-sha256.txt; then
  echo "frozen product bytes differ from the commit blobs" >&2
  printf '%s\n' 2 > /evidence/product-match.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/product-match.exit
sha256sum \
  crates/oo-pipeline/src/target_installer.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/base-sha256.txt
sha256sum Cargo.lock > /evidence/lock-before.txt

echo "=== cargo fetch locked offline ==="
set +e
cargo +1.94.0 fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo +1.94.0 fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
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
  exit "$fetch_exit"
fi

echo "=== cargo test init_pg_cli offline ==="
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --test init_pg_cli \
  -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

sha256sum Cargo.lock > /evidence/lock-after.txt
if ! cmp -s /evidence/lock-before.txt /evidence/lock-after.txt; then
  echo "Cargo.lock changed during smoke" >&2
  printf '%s\n' 2 > /evidence/lock-unchanged.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-unchanged.exit
printf '%s\n' deferred_until_overlay > /evidence/clippy.exit
echo "worst_phase_exit=$test_exit"
exit "$test_exit"
