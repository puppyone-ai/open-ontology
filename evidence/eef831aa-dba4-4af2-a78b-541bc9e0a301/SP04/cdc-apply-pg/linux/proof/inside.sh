#!/bin/bash
# Linux arm64 Rust 1.94 focused tests and Clippy for cdc-apply-pg.
# Fixed archive at /fixed. Product overlay at /overlay. One private PostgreSQL.
# Origin shape: evidence/.../a291/SP04/cdc-spool/linux/impl-7/inside.sh
# commit 96afe70fd68a56ae8eec7504bf20e09ecd3cdf65
# sha256 9529fe516e5d1eaddca29bd0939d5140aa36d37e2df784a33da3509e8a4d7a52
# Changes: copy the fixed archive, overlay only this tool, first resolve writes
# the lock, subset-check it, then Clippy --locked --offline. Tests talk to PG.
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
export OO_A301_HOST=oo-a301-pg
export OO_A301_PORT=5432
export OO_A301_ADMIN_USER=oo_fixture_admin
export OO_A301_ADMIN_PASS_FILE=/secrets/admin.pass
export OO_A301_RUNTIME_PASS_FILE=/secrets/runtime.pass
export OO_A301_CA=/secrets/ca.crt

mkdir -p /work/tmp /evidence

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
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
if [ ! -f /fixed/Cargo.toml ] || [ ! -f /fixed/Cargo.lock ] || [ ! -f /fixed/tools/cdc-spool/CONTRACT.md ]; then
  echo "fixed archive is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if [ ! -f /fixed/tools/session-store/Cargo.lock ] || [ ! -f /fixed/crates/oo-pipeline/Cargo.toml ]; then
  echo "fixed archive is missing lock inputs" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if [ -e /fixed/tools/cdc-apply-pg ]; then
  echo "fixed archive already contains cdc-apply-pg" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
rm -rf /work/src
mkdir -p /work/src
cp -R /fixed/. /work/src/
mkdir -p /work/src/tools/cdc-apply-pg
cp -R /overlay/. /work/src/tools/cdc-apply-pg/
rm -rf /work/src/tools/cdc-apply-pg/target
cd /work/src/tools/cdc-apply-pg
if [ ! -f Cargo.toml ] || [ ! -f src/lib.rs ] || [ ! -f src/main.rs ] || [ ! -f CONTRACT.md ]; then
  echo "product crate is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if [ ! -f Cargo.lock ]; then
  echo "subset-checked lock is required; refusing a fresh resolve" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
mkdir -p /evidence/source
cp -R Cargo.toml Cargo.lock CONTRACT.md src tests /evidence/source/
sha256sum Cargo.lock > /evidence/lock-sha256.txt
cp Cargo.lock /evidence/Cargo.lock
printf '%s\n' 0 > /evidence/source.exit

echo "=== lock subset compiler ==="
set +e
rustc --edition 2024 -D warnings -o /usr/local/bin/oo-a301-lock-subset /lock_subset.rs > /evidence/lock-subset-rustc.stdout 2> /evidence/lock-subset-rustc.stderr
subset_rustc=$?
printf '%s\n' "$subset_rustc" > /evidence/lock-subset-rustc.exit
echo "PHASE lock_subset_rustc_exit=$subset_rustc"
if [ "$subset_rustc" -ne 0 ]; then
  exit "$subset_rustc"
fi

echo "=== lock subset ==="
set +e
oo-a301-lock-subset \
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

echo "=== test locked ==="
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
exit "$clippy_exit"
