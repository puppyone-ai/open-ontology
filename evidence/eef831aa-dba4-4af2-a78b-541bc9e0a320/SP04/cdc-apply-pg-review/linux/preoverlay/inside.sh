#!/bin/bash
# Author suite on the archived product, before the review overlay exists.
# Origin shape: a301 proof inside.sh. This copy does not overlay a live tree.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export TMPDIR=/work/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target/tool
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export OO_A301_HOST=oo-a320-pg
export OO_A301_PORT=5432
export OO_A301_ADMIN_USER=oo_fixture_admin
export OO_A301_ADMIN_PASS_FILE=/secrets/admin.pass
export OO_A301_RUNTIME_PASS_FILE=/secrets/runtime.pass
export OO_A301_CA=/secrets/ca.crt

mkdir -p /work/tmp /evidence

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
uname -a
id

echo "=== rustc version gate ==="
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

echo "=== fixed archive ==="
if [ ! -f /fixed/tools/cdc-apply-pg/Cargo.toml ] || [ ! -f /fixed/Cargo.lock ] || [ ! -f /fixed/tools/cdc-spool/CONTRACT.md ]; then
  echo "fixed archive is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if [ -e /fixed/tools/cdc-apply-pg/tests/a320_apply.rs ]; then
  echo "archive already contains the review overlay" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
rm -rf /work/src
mkdir -p /work/src
cp -R /fixed/. /work/src/
rm -rf /work/src/tools/cdc-apply-pg/target
cd /work/src/tools/cdc-apply-pg
if [ -e tests/a320_apply.rs ]; then
  echo "review overlay is present before the copy" >&2
  printf '%s\n' 1 > /evidence/overlay-absent.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
mkdir -p /evidence/source
cp -R Cargo.toml Cargo.lock CONTRACT.md src tests /evidence/source/
sha256sum Cargo.lock > /evidence/lock-sha256.txt
cp Cargo.lock /evidence/Cargo.lock
(
  cd /fixed
  find crates/oo-core crates/oo-pipeline/migrations -type f | sort | xargs sha256sum
) > /evidence/core-migrations.sha256
printf '%s\n' 0 > /evidence/source.exit

echo "=== lock subset compiler ==="
set +e
rustc --edition 2024 -D warnings -o /usr/local/bin/oo-a320-lock-subset /lock_subset.rs > /evidence/lock-subset-rustc.stdout 2> /evidence/lock-subset-rustc.stderr
subset_rustc=$?
printf '%s\n' "$subset_rustc" > /evidence/lock-subset-rustc.exit
echo "PHASE lock_subset_rustc_exit=$subset_rustc"
if [ "$subset_rustc" -ne 0 ]; then
  exit "$subset_rustc"
fi

echo "=== lock subset ==="
set +e
oo-a320-lock-subset \
  /work/src/tools/cdc-apply-pg/Cargo.lock \
  /work/src/Cargo.lock \
  /work/src/tools/cdc-spool/Cargo.lock \
  /work/src/tools/session-store/Cargo.lock \
  > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
subset_exit=$?
printf '%s\n' "$subset_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$subset_exit"
if [ "$subset_exit" -ne 0 ]; then
  exit "$subset_exit"
fi
if ! grep -q 'lock_subset_ok tool_checksums=284' /evidence/lock-subset.stdout; then
  echo "registry checksum count is not 284" >&2
  exit 1
fi
cmp -s Cargo.lock /evidence/Cargo.lock || {
  echo "lock changed before cargo" >&2
  exit 2
}

echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
printf '%s\n' "$clippy_add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$clippy_add_exit"
if [ "$clippy_add_exit" -ne 0 ]; then
  exit "$clippy_add_exit"
fi

echo "=== fetch locked ==="
set +e
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== test locked offline ==="
set +e
cargo test --locked --offline -j 1 -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "cargo test rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  printf '%s\n' 2 > /evidence/lock-stable.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-stable.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

echo "=== unchanged trees ==="
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/product-work.sha256
cmp -s /evidence/source-sha256.txt /evidence/product-work.sha256
product_same=$?
printf '%s\n' "$product_same" > /evidence/product-unchanged.exit
(
  cd /work/src
  find crates/oo-core crates/oo-pipeline/migrations -type f | sort | xargs sha256sum
) > /evidence/core-migrations-after.sha256
cmp -s /evidence/core-migrations.sha256 /evidence/core-migrations-after.sha256
core_same=$?
printf '%s\n' "$core_same" > /evidence/core-migrations-unchanged.exit
if [ "$product_same" -ne 0 ] || [ "$core_same" -ne 0 ]; then
  echo "product or core tree changed" >&2
  exit 1
fi
awk '/^test result:/{print}' /evidence/test.stdout > /evidence/author-results.txt
exit 0
