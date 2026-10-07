#!/bin/bash
# Review overlay on the archived product. The only added file is tests/a320_apply.rs.
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
export OO_A320_HOST=oo-a320-pg
export OO_A320_PORT=5432
export OO_A320_ADMIN_USER=oo_fixture_admin
export OO_A320_ADMIN_PASS_FILE=/secrets/admin.pass
export OO_A320_RUNTIME_PASS_FILE=/secrets/runtime.pass
export OO_A320_CA=/secrets/ca.crt

mkdir -p /work/tmp /evidence

echo "=== identity ==="
rustc -vV
cargo -V
uname -a
id

if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

if [ ! -f /fixed/tools/cdc-apply-pg/Cargo.toml ] || [ ! -f /review/a320_apply.rs ]; then
  echo "archive or review test is missing" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
rm -rf /work/src
mkdir -p /work/src
cp -R /fixed/. /work/src/
rm -rf /work/src/tools/cdc-apply-pg/target
cd /work/src/tools/cdc-apply-pg
if [ -e tests/a320_apply.rs ]; then
  echo "product archive already has the review test" >&2
  printf '%s\n' 1 > /evidence/overlay-absent.exit
  exit 1
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/product-before.sha256
cp /review/a320_apply.rs tests/a320_apply.rs
sha256sum tests/a320_apply.rs > /evidence/overlay-sha256.txt
sha256sum Cargo.lock > /evidence/lock-sha256.txt
cp Cargo.lock /evidence/Cargo.lock
mkdir -p /evidence/source
cp -R Cargo.toml Cargo.lock CONTRACT.md src tests /evidence/source/
(
  cd /fixed
  find crates/oo-core crates/oo-pipeline/migrations -type f | sort | xargs sha256sum
) > /evidence/core-migrations.sha256
printf '%s\n' 0 > /evidence/source.exit

set +e
rustc --edition 2024 -D warnings -o /usr/local/bin/oo-a320-lock-subset /lock_subset.rs > /evidence/lock-subset-rustc.stdout 2> /evidence/lock-subset-rustc.stderr
subset_rustc=$?
printf '%s\n' "$subset_rustc" > /evidence/lock-subset-rustc.exit
if [ "$subset_rustc" -ne 0 ]; then
  exit "$subset_rustc"
fi
oo-a320-lock-subset \
  /work/src/tools/cdc-apply-pg/Cargo.lock \
  /work/src/Cargo.lock \
  /work/src/tools/cdc-spool/Cargo.lock \
  /work/src/tools/session-store/Cargo.lock \
  > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
subset_exit=$?
printf '%s\n' "$subset_exit" > /evidence/lock-subset.exit
if [ "$subset_exit" -ne 0 ]; then
  exit "$subset_exit"
fi
if ! grep -q 'lock_subset_ok tool_checksums=284' /evidence/lock-subset.stdout; then
  echo "registry checksum count is not 284" >&2
  exit 1
fi

rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
printf '%s\n' "$clippy_add_exit" > /evidence/clippy-add.exit
if [ "$clippy_add_exit" -ne 0 ]; then
  exit "$clippy_add_exit"
fi

cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

cargo test --locked --offline -j 1 --test a320_apply -- --test-threads=1 --nocapture > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  printf '%s\n' 2 > /evidence/lock-stable.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-stable.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

find Cargo.toml Cargo.lock CONTRACT.md src tests -type f ! -name a320_apply.rs | sort | xargs sha256sum > /evidence/product-after.sha256
cmp -s /evidence/product-before.sha256 /evidence/product-after.sha256
product_same=$?
printf '%s\n' "$product_same" > /evidence/product-unchanged.exit
(
  cd /work/src
  find crates/oo-core crates/oo-pipeline/migrations -type f | sort | xargs sha256sum
) > /evidence/core-migrations-after.sha256
cmp -s /evidence/core-migrations.sha256 /evidence/core-migrations-after.sha256
core_same=$?
printf '%s\n' "$core_same" > /evidence/core-migrations-unchanged.exit
oo-a320-lock-subset \
  /work/src/tools/cdc-apply-pg/Cargo.lock \
  /work/src/Cargo.lock \
  /work/src/tools/cdc-spool/Cargo.lock \
  /work/src/tools/session-store/Cargo.lock \
  > /evidence/lock-subset-after.stdout 2> /evidence/lock-subset-after.stderr
after_exit=$?
printf '%s\n' "$after_exit" > /evidence/lock-subset-after.exit
if [ "$product_same" -ne 0 ] || [ "$core_same" -ne 0 ] || [ "$after_exit" -ne 0 ]; then
  echo "product, core, or lock subset changed" >&2
  exit 1
fi
if ! grep -q 'lock_subset_ok tool_checksums=284' /evidence/lock-subset-after.stdout; then
  echo "registry checksum count changed" >&2
  exit 1
fi
exit 0
