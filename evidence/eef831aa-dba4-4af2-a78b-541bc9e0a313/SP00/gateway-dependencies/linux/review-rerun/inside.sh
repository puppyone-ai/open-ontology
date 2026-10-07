#!/bin/bash
# Review re-run from a read-only git archive. Does not update the root lock.
set -u
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
BASE=/src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a313/SP00/gateway-dependencies
LOCK_SHA=16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a

phase() {
  printf '%s\n' "$2" > "/evidence/${1}.exit"
  echo "PHASE $1=$2"
}

echo "=== identity ==="
rustc -Vv > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
rustup show > /evidence/rustup-show.txt
cat /evidence/rustc-version.txt

echo "=== archived root lock ==="
got=$(sha256sum /src/Cargo.lock | awk '{print $1}')
printf '%s\n' "$got" > /evidence/lock-sha256.txt
if [ "$got" != "$LOCK_SHA" ]; then
  echo "archived Cargo.lock sha256 mismatch" >&2
  phase lock 2
  exit 2
fi
if ! grep -q 'version = "=0.8.9"' /src/Cargo.toml; then
  echo "archived Cargo.toml has no axum pin" >&2
  phase pin 2
  exit 2
fi
phase pin 0
phase lock 0

echo "=== compile lockdiff ==="
rustc --edition 2024 -D warnings -o /target/lockdiff "$BASE/tools/lockdiff.rs" > /evidence/lockdiff-build.stdout 2> /evidence/lockdiff-build.stderr
phase lockdiff_build $?
/target/lockdiff selfcheck > /evidence/lockdiff-selfcheck.stdout 2> /evidence/lockdiff-selfcheck.stderr
phase lockdiff_selfcheck $?
/target/lockdiff subset /src/Cargo.lock "$BASE/probe/Cargo.lock" "$BASE/upstream/new-crate-pins.txt" 2026-10-06T12:47:28Z > /evidence/lockdiff-subset.stdout 2> /evidence/lockdiff-subset.stderr
phase lockdiff_subset $?
/target/lockdiff exact /src/Cargo.lock /src/Cargo.lock > /evidence/lockdiff-exact-self.stdout 2> /evidence/lockdiff-exact-self.stderr
phase lockdiff_exact_self $?

echo "=== clippy component ==="
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy > /evidence/clippy-install.stdout 2> /evidence/clippy-install.stderr
  phase clippy_install $?
else
  printf '%s\n' already_installed > /evidence/clippy-install.stdout
  phase clippy_install 0
fi

echo "=== probe test ==="
export CARGO_NET_OFFLINE=true
set +e
cargo test --manifest-path "$BASE/probe/Cargo.toml" --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
phase test "$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== probe clippy ==="
set +e
cargo clippy --manifest-path "$BASE/probe/Cargo.toml" --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
phase clippy "$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

sha256sum /src/Cargo.toml /src/Cargo.lock "$BASE/probe/Cargo.toml" "$BASE/probe/Cargo.lock" > /evidence/manifest-sha256-after.txt
echo "inside_ok=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit 0
