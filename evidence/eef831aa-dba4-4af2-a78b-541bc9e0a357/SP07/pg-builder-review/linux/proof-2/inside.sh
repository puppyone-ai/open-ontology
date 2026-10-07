#!/bin/bash
# Fixed-archive smoke, then the a357 test-only oracle.
# /src is commit a13b1cc and is mounted read-only.
# Author tests and clippy run before the oracle exists.
# A work-tree re-export of RawReply exists only while the oracle compiles.
# It is restored from /src before the byte compare.
# Locks are not regenerated. PostgreSQL is not started. No model is called.
set -u
set -e
set -o pipefail
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/pg-builder
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
export A357_RECEIPTS=/receipts
mkdir -p /cargo-home/tmp /cargo-home/bin /evidence/phases /work
printf '%s\n' no > /evidence/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

run_phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.log" 2>&1
  local status=$?
  set -e
  record "$name" "$status"
  if [ "$status" -ne 0 ]; then
    exit "$status"
  fi
}

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
uname -a | tee /evidence/uname.txt
id
rustc_exit=0
grep -q 'release: 1.94' /evidence/rustc-version.txt || rustc_exit=1
grep -q 'aarch64-unknown-linux-gnu' /evidence/rustc-version.txt || rustc_exit=1
record toolchain "$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then
  exit "$rustc_exit"
fi

echo "=== copy the fixed archive; oracle stays absent ==="
rm -rf /work
mkdir -p /work
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
printf '%s\n' moved-aside > /evidence/toolchain-aside.txt
cd /work
if [ -e tools/pg-builder/tests/a357_consumer.rs ] || [ -e /src/tools/pg-builder/tests/a357_consumer.rs ]; then
  echo "oracle present before the author smoke" >&2
  record overlay-absent 2
  exit 2
fi
record overlay-absent 0
for lock in Cargo.lock tools/pg-builder/Cargo.lock testing/real-db/pg-builder/Cargo.lock; do
  if ! cmp -s "/src/$lock" "/work/$lock"; then
    echo "lock differs from the fixed archive: $lock" >&2
    record archive-locks 2
    exit 2
  fi
done
record archive-locks 0
sha256sum Cargo.lock > /evidence/root-lock-before.sha256
sha256sum tools/pg-builder/Cargo.lock testing/real-db/pg-builder/Cargo.lock > /evidence/child-locks-before.sha256

echo "=== fetch the committed builder lock ==="
run_phase fetch-builder cargo fetch --manifest-path tools/pg-builder/Cargo.toml --locked
run_phase fetch-builder-offline cargo fetch --manifest-path tools/pg-builder/Cargo.toml --locked --offline
run_phase fetch-fixture-offline cargo fetch --manifest-path testing/real-db/pg-builder/Cargo.toml --locked --offline

echo "=== author pg-builder tests, before the oracle exists ==="
run_phase test-builder cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- --test-threads=1
if ! grep -q 'test result: ok' /evidence/test-builder.log; then
  echo "author builder tests did not report ok" >&2
  record test-builder-ok 1
  exit 1
fi
if grep -q 'a357_consumer' /evidence/test-builder.log; then
  echo "author log names the oracle" >&2
  record test-builder-clean 2
  exit 2
fi
record test-builder-clean 0

echo "=== author pg-fixture tests ==="
export CARGO_TARGET_DIR=/target/pg-fixture
run_phase test-fixture cargo test --manifest-path testing/real-db/pg-builder/Cargo.toml --locked --offline --all-targets -- --test-threads=1
if ! grep -q 'test result: ok' /evidence/test-fixture.log; then
  echo "author fixture tests did not report ok" >&2
  record test-fixture-ok 1
  exit 1
fi
record test-fixture-ok 0
export CARGO_TARGET_DIR=/target/pg-builder

echo "=== clippy component ==="
set +e
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  component_exit=0
  printf '%s\n' already_installed > /evidence/clippy-component.log
else
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
  component_exit=$?
fi
set -e
record clippy-component "$component_exit"
if [ "$component_exit" -ne 0 ]; then
  exit "$component_exit"
fi

echo "=== product clippy before the oracle ==="
run_phase clippy-builder cargo clippy --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- -D warnings
export CARGO_TARGET_DIR=/target/pg-fixture
run_phase clippy-fixture cargo clippy --manifest-path testing/real-db/pg-builder/Cargo.toml --locked --offline --all-targets -- -D warnings
export CARGO_TARGET_DIR=/target/pg-builder

echo "=== work-tree re-export so the integration test can name RawReply ==="
cp tools/pg-builder/src/lib.rs /evidence/lib-before-reexport.rs
awk '
  $0 == "    ScriptedTransport," && !done { print "    RawReply,"; done=1 }
  { print }
' tools/pg-builder/src/lib.rs > /cargo-home/tmp/lib.rs.a357
mv /cargo-home/tmp/lib.rs.a357 tools/pg-builder/src/lib.rs
if ! grep -q 'RawReply,' tools/pg-builder/src/lib.rs; then
  echo "RawReply re-export was not inserted" >&2
  record reexport 2
  exit 2
fi
record reexport 0
printf '%s\n' 'worktree-only pub use of RawReply; restored before the byte compare' > /evidence/reexport.txt

echo "=== copy the test-only oracle ==="
cp /overlay/a357_consumer.rs tools/pg-builder/tests/a357_consumer.rs
oracle_got=$(sha256sum tools/pg-builder/tests/a357_consumer.rs | awk '{print $1}')
oracle_want=$(awk '{print $1}' /evidence/oracle.sha256)
printf 'want=%s got=%s\n' "$oracle_want" "$oracle_got" > /evidence/oracle-copied.txt
if [ "$oracle_got" != "$oracle_want" ]; then
  echo "oracle hash drifted" >&2
  record oracle-copy 2
  exit 2
fi
record oracle-copy 0

echo "=== independent a357 oracle ==="
run_phase test-a357 cargo test --manifest-path tools/pg-builder/Cargo.toml --locked --offline --test a357_consumer -- --test-threads=1

echo "=== clippy including the oracle ==="
run_phase clippy-oracle cargo clippy --manifest-path tools/pg-builder/Cargo.toml --locked --offline --all-targets -- -D warnings

echo "=== lock subset, recorded, locks not replaced ==="
set +e
rustc --edition 2024 -D warnings /evidence/lock_subset.rs -o /cargo-home/bin/lock-subset > /evidence/lock-compile.log 2>&1
lock_compile=$?
set -e
record lock-compile "$lock_compile"
if [ "$lock_compile" -eq 0 ]; then
  set +e
  /cargo-home/bin/lock-subset tools/pg-builder/Cargo.lock /work/Cargo.lock > /evidence/lock-subset-builder.stdout 2> /evidence/lock-subset-builder.stderr
  subset_builder=$?
  /cargo-home/bin/lock-subset testing/real-db/pg-builder/Cargo.lock /work/Cargo.lock > /evidence/lock-subset-fixture.stdout 2> /evidence/lock-subset-fixture.stderr
  subset_fixture=$?
  set -e
  record lock-subset-builder "$subset_builder"
  record lock-subset-fixture "$subset_fixture"
else
  record lock-subset-builder "$lock_compile"
  record lock-subset-fixture "$lock_compile"
fi

echo "=== remove the oracle, restore lib.rs, compare product bytes ==="
rm -f tools/pg-builder/tests/a357_consumer.rs
cp /src/tools/pg-builder/src/lib.rs tools/pg-builder/src/lib.rs
if [ -e tools/pg-builder/tests/a357_consumer.rs ]; then
  echo "oracle file remained" >&2
  record oracle-removed 2
  exit 2
fi
if ! cmp -s /src/tools/pg-builder/src/lib.rs tools/pg-builder/src/lib.rs; then
  echo "lib.rs was not restored" >&2
  record oracle-removed 2
  exit 2
fi
record oracle-removed 0
: > /evidence/product-diff.txt
product_exit=0
while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  if [ "$rel" = "rust-toolchain.toml" ]; then
    continue
  fi
  if [ ! -f "/work/$rel" ]; then
    echo "missing after proof $rel" >> /evidence/product-diff.txt
    product_exit=2
    continue
  fi
  if ! cmp -s "/src/$rel" "/work/$rel"; then
    echo "differs $rel" >> /evidence/product-diff.txt
    product_exit=2
  fi
done < <(cd /src && find . -type f ! -name '._*' ! -name '.DS_Store' | sed 's|^\./||' | sort)
while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  if [ ! -f "/src/$rel" ]; then
    echo "extra after proof $rel" >> /evidence/product-diff.txt
    product_exit=2
  fi
done < <(cd /work && find . -type f ! -name '._*' ! -name '.DS_Store' | sed 's|^\./||' | sort)
record product-bytes "$product_exit"
if [ "$product_exit" -ne 0 ]; then
  exit "$product_exit"
fi

sha256sum Cargo.lock > /evidence/root-lock-after.sha256
sha256sum tools/pg-builder/Cargo.lock testing/real-db/pg-builder/Cargo.lock > /evidence/child-locks.sha256
if ! cmp -s /evidence/root-lock-before.sha256 /evidence/root-lock-after.sha256; then
  echo "root lock bytes changed" >&2
  record root-lock 1
  exit 1
fi
if ! cmp -s /evidence/child-locks-before.sha256 /evidence/child-locks.sha256; then
  echo "child lock bytes changed" >&2
  record root-lock 1
  exit 1
fi
record root-lock 0
echo "PROOF_OK"
