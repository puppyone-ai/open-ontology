#!/bin/bash
# Focused oo-pipeline smoke of the immutable 9a71efa archive.
# Fetch has no -p. Offline test and clippy run only after fetch exits 0.
# The ignored live PostgreSQL test is not executed here.
set -u
echo "oo_task=sp04-segment-null-review"
echo "oo_source=9a71efaf9cb63a2d63061a241f6b8ee8b00426df"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
echo "CARGO_HOME=$CARGO_HOME"
rustc -Vv
cargo -V
rustup show
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-pipeline/Cargo.toml ]; then
  echo "snapshot missing cargo inputs" >&2
  exit 2
fi
if [ ! -f crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql ]; then
  echo "snapshot missing 0002" >&2
  exit 2
fi
check_hash() {
  file=$1
  expect=$2
  got=$(sha256sum "$file" | awk '{print $1}')
  echo "sha256 $file $got"
  if [ "$got" != "$expect" ]; then
    echo "hash mismatch $file" >&2
    exit 2
  fi
}
check_hash crates/oo-pipeline/migrations/0000_bootstrap_roles.sql 37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208
check_hash crates/oo-pipeline/migrations/0001_target_fence_expand.sql 97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9
check_hash crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql 59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512
mkdir -p /evidence
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set -e
  echo "phase_exit=$clippy_add phase=rustup_component_add_clippy"
  printf '%s\n' "$clippy_add" > /evidence/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    printf '%s\n' "$clippy_add" > /evidence/worst.exit
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /evidence/clippy-component.exit
fi
echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' "$fetch_exit" > /evidence/worst.exit
  chown -R "$(stat -c '%u' /inside.sh)":"$(stat -c '%g' /inside.sh)" /evidence || true
  exit "$fetch_exit"
fi
echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_oo_pipeline"
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_pipeline"
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "elapsed_seconds=$SECONDS"
worst=$test_exit
if [ "$clippy_exit" -gt "$worst" ]; then
  worst=$clippy_exit
fi
printf '%s\n' "$worst" > /evidence/worst.exit
chown -R "$(stat -c '%u' /inside.sh)":"$(stat -c '%g' /inside.sh)" /evidence || true
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
