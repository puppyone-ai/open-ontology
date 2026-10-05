#!/bin/bash
# Review overlay on fixed commit 6e1e6e57. Rust 1.94, locked, offline.
# One private PostgreSQL is already on the network. Product files are not edited.
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
if [ ! -f crates/oo-pipeline/tests/init_pg_a232_review.rs ]; then
  echo "review overlay missing" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
sha256sum crates/oo-pipeline/tests/init_pg_a232_review.rs > /evidence/overlay-sha256.txt
if ! cmp -s /evidence/overlay-sha256.txt /overlay-sha256.txt; then
  echo "review harness bytes differ" >&2
  printf '%s\n' 2 > /evidence/overlay-match.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-match.exit
sha256sum \
  contracts/pipeline/init-pg-cli-v1.md \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/tests/init_pg_cli.rs \
  > /evidence/product-sha256.txt
if ! cmp -s /evidence/product-sha256.txt /expected-product-sha256.txt; then
  echo "product bytes differ from the commit blobs" >&2
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

echo "=== cargo test review ==="
mkdir -p /tmp/a232-live-secrets /evidence/live
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline \
  -p oo-pipeline --test init_pg_a232_review -- --test-threads=1 --ignored \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

echo "=== clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
clippy_exit=$component_exit
if [ "$component_exit" -eq 0 ]; then
  set +e
  cargo +1.94.0 clippy --manifest-path Cargo.toml --locked --offline \
    -p oo-pipeline --bin init-pg --test init_pg_cli --test init_pg_a232_review -- -D warnings \
    > /evidence/clippy.stdout 2> /evidence/clippy.stderr
  clippy_exit=$?
  set -e
fi
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

secret_scan=0
if grep -R -F -q -e 's3cret-a232-marker' -e 'decoy-a232-env' -e 'PRIVATE KEY' -e 'BEGIN CERTIFICATE' /evidence/live \
  || grep -R -F -q -f /run/init-secrets/admin.pass /evidence/live
then
  echo "secret or marker appeared in live evidence" >&2
  secret_scan=2
fi
printf '%s\n' "$secret_scan" > /evidence/secret-scan.exit
echo "PHASE secret_scan_exit=$secret_scan"

sha256sum Cargo.lock > /evidence/lock-after.txt
if ! cmp -s /evidence/lock-before.txt /evidence/lock-after.txt; then
  echo "Cargo.lock changed during proof" >&2
  printf '%s\n' 2 > /evidence/lock-unchanged.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-unchanged.exit

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$secret_scan" -ne 0 ]; then worst=$secret_scan; fi
echo "worst_phase_exit=$worst"
exit "$worst"
