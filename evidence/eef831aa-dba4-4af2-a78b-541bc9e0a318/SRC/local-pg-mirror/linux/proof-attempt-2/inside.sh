#!/bin/bash
# Linux arm64 Rust 1.94 focused tests and Clippy for local-pg-mirror.
# Fixed archive at /fixed. Product overlay at /overlay. One public-fixture PostgreSQL.
# Origin: evidence/.../a301/SP04/cdc-apply-pg/linux/proof/inside.sh
# sha256 f8564183a896f167f0b9e9356d338624d39b59ca3f4168de2156a557f15ab22f
# Change: this tool has no lock yet, so generate-lockfile runs before the subset check.
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
export OO_MIRROR_FIXTURE=/work/fixture.json

mkdir -p /work/tmp /evidence /work/src

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

echo "=== fixed archive ==="
if [ ! -f /fixed/Cargo.toml ] || [ ! -f /fixed/Cargo.lock ] || [ ! -f /fixed/crates/oo-pipeline/Cargo.toml ]; then
  echo "fixed archive is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if [ -e /fixed/tools/local-pg-mirror ]; then
  echo "fixed archive already contains local-pg-mirror" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
rm -rf /work/src
mkdir -p /work/src
cp -R /fixed/. /work/src/
mkdir -p /work/src/tools/local-pg-mirror
cp -R /overlay/. /work/src/tools/local-pg-mirror/
rm -rf /work/src/tools/local-pg-mirror/target
cd /work/src/tools/local-pg-mirror
if [ ! -f Cargo.toml ] || [ ! -f src/lib.rs ] || [ ! -f src/main.rs ] || [ ! -f CONTRACT.md ] || [ ! -f tests/live.rs ]; then
  echo "product crate is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
cp /work/src/Cargo.lock /evidence/root-Cargo.lock.before
find Cargo.toml CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
mkdir -p /evidence/source
cp -R Cargo.toml CONTRACT.md src tests /evidence/source/
printf '%s\n' 0 > /evidence/source.exit

echo "=== tool lock ==="
set +e
if [ -f Cargo.lock ]; then
  printf '%s\n' 0 > /evidence/generate-lockfile.exit
  echo "PHASE generate_lockfile_exit=0"
else
  cargo generate-lockfile > /evidence/generate-lockfile.stdout 2> /evidence/generate-lockfile.stderr
  gen_exit=$?
  printf '%s\n' "$gen_exit" > /evidence/generate-lockfile.exit
  echo "PHASE generate_lockfile_exit=$gen_exit"
  if [ "$gen_exit" -ne 0 ]; then
    exit "$gen_exit"
  fi
fi
cp Cargo.lock /evidence/Cargo.lock
sha256sum Cargo.lock > /evidence/lock-sha256.txt
cp Cargo.lock /evidence/source/Cargo.lock

echo "=== lock subset compiler ==="
rustc --edition 2024 -D warnings -o /usr/local/bin/oo-a318-lock-subset /lock_subset.rs > /evidence/lock-subset-rustc.stdout 2> /evidence/lock-subset-rustc.stderr
subset_rustc=$?
printf '%s\n' "$subset_rustc" > /evidence/lock-subset-rustc.exit
echo "PHASE lock_subset_rustc_exit=$subset_rustc"
if [ "$subset_rustc" -ne 0 ]; then
  exit "$subset_rustc"
fi

echo "=== lock subset ==="
allowed=(/work/src/Cargo.lock)
while IFS= read -r path; do
  allowed+=("$path")
done < <(find /work/src/tools -name Cargo.lock -type f ! -path '/work/src/tools/local-pg-mirror/*' | sort)
oo-a318-lock-subset /work/src/tools/local-pg-mirror/Cargo.lock "${allowed[@]}" > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
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
cmp -s /work/src/Cargo.lock /evidence/root-Cargo.lock.before || {
  echo "root lock changed before cargo" >&2
  exit 2
}

echo "=== clippy component ==="
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
printf '%s\n' "$clippy_add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$clippy_add_exit"
if [ "$clippy_add_exit" -ne 0 ]; then
  exit "$clippy_add_exit"
fi

echo "=== fetch locked ==="
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

cat > /work/fixture.json <<'EOF'
{
  "host": "oo-a318-pg",
  "port": 5432,
  "server_name": "oo-a318-pg",
  "source_database": "srcdb",
  "target_database": "mirrordb",
  "admin_user": "oo_fixture_admin",
  "source_user": "oo_src_read",
  "runtime_user": "oo_mirror_runtime",
  "reader_user": "oo_mirror_read",
  "admin_password": "/secrets/admin.pass",
  "source_password": "/secrets/src.pass",
  "runtime_password": "/secrets/runtime.pass",
  "reader_password": "/secrets/reader.pass",
  "ca": "/secrets/ca.crt"
}
EOF

echo "=== test locked ==="
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
if ! cmp -s /work/src/Cargo.lock /evidence/root-Cargo.lock.before; then
  echo "root lock changed" >&2
  printf '%s\n' 2 > /evidence/root-lock-stable.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-stable.exit
printf '%s\n' 0 > /evidence/root-lock-stable.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== clippy ==="
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "cargo clippy rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  exit 2
fi
exit "$clippy_exit"
