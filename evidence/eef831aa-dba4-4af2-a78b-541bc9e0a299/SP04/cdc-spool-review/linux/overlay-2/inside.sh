#!/bin/bash
# Independent boundary overlay on a private copy of the fixed archive.
# The product tree is not modified. No PostgreSQL.
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
export REVIEW_HWM_DIR=/evidence

mkdir -p /work/tmp /evidence

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
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

echo "=== product copy ==="
rm -rf /work/cdc-spool
mkdir -p /work/cdc-spool
cp -R /tool/. /work/cdc-spool/
cd /work/cdc-spool
if [ -e tests/a299_boundary.rs ]; then
  echo "archive already contains the review overlay" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
cp /overlay/a299_boundary.rs tests/a299_boundary.rs
if ! cmp -s Cargo.toml /tool/Cargo.toml || ! cmp -s Cargo.lock /tool/Cargo.lock || ! cmp -s CONTRACT.md /tool/CONTRACT.md; then
  echo "product manifest drifted" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if ! cmp -s src/lib.rs /tool/src/lib.rs || ! cmp -s src/spool.rs /tool/src/spool.rs || ! cmp -s src/recover.rs /tool/src/recover.rs; then
  echo "product source drifted" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
sha256sum Cargo.lock > /evidence/lock-before.txt
cp tests/a299_boundary.rs /evidence/a299_boundary.rs
printf '%s\n' 0 > /evidence/source.exit

echo "=== fetch offline ==="
set +e
cargo fetch --locked --offline > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
printf '%s\n' "$clippy_add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$clippy_add_exit"
if [ "$clippy_add_exit" -ne 0 ]; then
  exit "$clippy_add_exit"
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

echo "=== boundary tests ==="
set +e
cargo test --locked --offline --test a299_boundary -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
if [ ! -s /evidence/seal-hwm.txt ] || [ ! -s /evidence/replay-hwm.txt ]; then
  echo "seal and replay VmHWM files were not written" >&2
  exit 4
fi

echo "=== registry triples ==="
set +e
rustc --edition 2024 -D warnings /review-tools/lock_subset.rs -o /work/lock_subset > /evidence/registry-rustc.stdout 2> /evidence/registry-rustc.stderr
reg_rustc=$?
printf '%s\n' "$reg_rustc" > /evidence/registry-rustc.exit
echo "PHASE registry_rustc_exit=$reg_rustc"
if [ "$reg_rustc" -ne 0 ]; then
  exit "$reg_rustc"
fi
/work/lock_subset /work/cdc-spool/Cargo.lock /root.lock > /evidence/registry.txt 2> /evidence/registry.stderr
reg_exit=$?
printf '%s\n' "$reg_exit" > /evidence/registry.exit
echo "PHASE registry_exit=$reg_exit"
if [ "$reg_exit" -ne 0 ]; then
  exit "$reg_exit"
fi
sha256sum Cargo.lock > /evidence/lock-after.txt
if ! cmp -s Cargo.lock /tool/Cargo.lock; then
  echo "Cargo.lock changed during the overlay" >&2
  exit 4
fi
echo OVERLAY_OK
exit 0
