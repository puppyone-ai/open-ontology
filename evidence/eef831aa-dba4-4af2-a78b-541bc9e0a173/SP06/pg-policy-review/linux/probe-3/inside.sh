#!/bin/bash
# Review probe. One test, the clinic fixture. Author live test is not executed.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
echo "=== identity ==="
rustc -vV
cargo -V
echo "=== overlay present ==="
if [ ! -f crates/oo-gateway/tests/policy_pg_a173_review.rs ]; then
  echo "overlay missing" >&2
  echo 2 > /evidence/overlay-present.exit
  exit 2
fi
sha256sum crates/oo-gateway/tests/policy_pg_a173_review.rs > /evidence/overlay.sha256
echo 0 > /evidence/overlay-present.exit
if [ -e /run/oo-a173/fixture/ca.key ] || [ -e /run/oo-a173/fixture/server.key ]; then
  echo "client mount contains a private key" >&2
  echo 2 > /evidence/keys-absent.exit
  exit 2
fi
echo 0 > /evidence/keys-absent.exit
echo "=== cargo test review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test policy_pg_a173_review -- --test-threads=1 --nocapture > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
clippy_component_exit=$?
set -e
echo "$clippy_component_exit" > /evidence/rustup-clippy.exit
echo "PHASE clippy_component_exit=$clippy_component_exit"
echo "=== cargo clippy review ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test policy_pg_a173_review -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
{
  rustc -vV
  cargo -V
} > /evidence/rustc-version.txt
worst=$test_exit
if [ "$clippy_component_exit" -gt "$worst" ]; then
  worst=$clippy_component_exit
fi
if [ "$clippy_exit" -gt "$worst" ]; then
  worst=$clippy_exit
fi
echo "$worst" > /evidence/worst.exit
echo "worst_phase_exit=$worst"
exit "$worst"
