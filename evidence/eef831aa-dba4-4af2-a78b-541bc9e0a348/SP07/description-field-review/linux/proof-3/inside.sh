#!/bin/bash
# Fixed-archive smoke, then the a348 test-only oracle.
# /src is commit c23db28 and is mounted read-only.
# /work is a bind mount and is not removed.
# The oracle is copied only after the author tests. It is removed before the byte compare.
# Locks are not regenerated. PostgreSQL is not started. No model is called.
set -u
set -e
set -o pipefail
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
mkdir -p /cargo-home/tmp /evidence/phases
printf '%s\n' no > /evidence/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

require_line() {
  local name="$1"
  local file="$2"
  local line="$3"
  if ! grep -F -q "$line" "$file"; then
    echo "missing result line in $file: $line" >&2
    record "$name" 2
    exit 2
  fi
  record "$name" 0
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
find /work -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
printf '%s\n' moved-aside > /evidence/toolchain-aside.txt
cd /work
if [ -e tools/description-pack/tests/a348_sealed.rs ] || [ -e /src/tools/description-pack/tests/a348_sealed.rs ]; then
  echo "oracle present before the author smoke" >&2
  record overlay-absent 2
  exit 2
fi
record overlay-absent 0
for lock in Cargo.lock tools/description-review/Cargo.lock tools/description-pack/Cargo.lock tools/description-provider/Cargo.lock; do
  if ! cmp -s "/src/$lock" "/work/$lock"; then
    echo "lock differs from the fixed archive: $lock" >&2
    record archive-locks 2
    exit 2
  fi
done
record archive-locks 0
sha256sum Cargo.lock > /evidence/root-lock-before.sha256
sha256sum tools/description-review/Cargo.lock tools/description-pack/Cargo.lock tools/description-provider/Cargo.lock > /evidence/child-locks-before.sha256

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

echo "=== author description candidate API and CLI ==="
run_phase test-candidate cargo test --locked -p oo-builder --test description_candidate --test description_candidate_cli -- --test-threads=1
require_line test-candidate-count /evidence/test-candidate.log "test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"
require_line test-candidate-cli-count /evidence/test-candidate.log "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"

echo "=== author description_candidate lib tests ==="
run_phase test-lib cargo test --locked -p oo-builder --lib description_candidate -- --test-threads=1
require_line test-lib-count /evidence/test-lib.log "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 33 filtered out;"
if ! grep -q 'prompt_version_is_the_sealed_constant' /evidence/test-lib.log; then
  echo "lib filter missed the sealed prompt test" >&2
  record test-lib-filter 1
  exit 1
fi
record test-lib-filter 0

echo "=== author description-review ==="
run_phase test-review cargo test --locked --manifest-path tools/description-review/Cargo.toml -- --test-threads=1
require_line test-review-count /evidence/test-review.log "test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"

echo "=== author description-pack, before the oracle exists ==="
run_phase test-pack cargo test --locked --manifest-path tools/description-pack/Cargo.toml -- --test-threads=1
require_line test-pack-count /evidence/test-pack.log "test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"
if grep -q 'a348_sealed' /evidence/test-pack.log; then
  echo "author pack log names the oracle" >&2
  record test-pack-clean 2
  exit 2
fi
record test-pack-clean 0

if [ ! -x /target/debug/description-candidate ]; then
  echo "description-candidate binary is missing" >&2
  record test-candidate-bin 1
  exit 1
fi
export OO_A263_DESCRIPTION_CANDIDATE=/target/debug/description-candidate
printf '%s\n' "$OO_A263_DESCRIPTION_CANDIDATE" > /evidence/description-candidate-bin.txt
record test-candidate-bin 0

echo "=== author description-provider ==="
run_phase test-provider cargo test --locked --manifest-path tools/description-provider/Cargo.toml -- --test-threads=1
require_line test-provider-count /evidence/test-provider.log "test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"

echo "=== copy the test-only oracle ==="
cp /overlay/a348_sealed.rs tools/description-pack/tests/a348_sealed.rs
oracle_got=$(sha256sum tools/description-pack/tests/a348_sealed.rs | awk '{print $1}')
oracle_want=$(awk '{print $1}' /evidence/oracle.sha256)
printf 'want=%s got=%s\n' "$oracle_want" "$oracle_got" > /evidence/oracle-copied.txt
if [ "$oracle_got" != "$oracle_want" ]; then
  echo "oracle hash drifted" >&2
  record oracle-copy 2
  exit 2
fi
record oracle-copy 0

echo "=== independent a348 oracle ==="
run_phase test-a348 cargo test --locked --offline --manifest-path tools/description-pack/Cargo.toml --test a348_sealed -- --test-threads=1
require_line test-a348-count /evidence/test-a348.log "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"

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

echo "=== clippy oo-builder description candidate ==="
run_phase clippy-builder cargo clippy --locked --offline -p oo-builder --lib --bin description-candidate --test description_candidate --test description_candidate_cli -- -D warnings

echo "=== clippy description-review ==="
run_phase clippy-review cargo clippy --locked --offline --manifest-path tools/description-review/Cargo.toml --all-targets -- -D warnings

echo "=== clippy description-pack, including the oracle ==="
run_phase clippy-pack cargo clippy --locked --offline --manifest-path tools/description-pack/Cargo.toml --all-targets -- -D warnings

echo "=== clippy description-provider ==="
run_phase clippy-provider cargo clippy --locked --offline --manifest-path tools/description-provider/Cargo.toml --all-targets -- -D warnings

echo "=== description-pack lock subset ==="
set +e
rustc --edition 2024 -D warnings /evidence/lock_subset.rs -o /usr/local/bin/lock_subset > /evidence/lock-compile.log 2>&1
lock_compile=$?
set -e
record lock-compile "$lock_compile"
if [ "$lock_compile" -ne 0 ]; then
  exit "$lock_compile"
fi
set +e
/usr/local/bin/lock_subset tools/description-pack/Cargo.lock /work/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
record lock-subset "$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  exit "$lock_exit"
fi
if ! grep -q 'summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true' /evidence/lock-subset.stdout; then
  echo "pack lock summary drifted" >&2
  record lock-summary 2
  exit 2
fi
record lock-summary 0

echo "=== remove the oracle and compare product bytes to the archive ==="
rm -f tools/description-pack/tests/a348_sealed.rs
if [ -e tools/description-pack/tests/a348_sealed.rs ]; then
  echo "oracle file remained" >&2
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
if [ -n "$(find tools/description-pack/tests -name 'a348_sealed.rs' -print)" ]; then
  echo "oracle still under tests" >> /evidence/product-diff.txt
  product_exit=2
fi
record product-bytes "$product_exit"
if [ "$product_exit" -ne 0 ]; then
  exit "$product_exit"
fi

sha256sum Cargo.lock > /evidence/root-lock-after.sha256
sha256sum tools/description-review/Cargo.lock tools/description-pack/Cargo.lock tools/description-provider/Cargo.lock > /evidence/child-locks.sha256
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
