#!/bin/bash
# Independent oracle on a fresh public fixture, after the author suite.
# Origin: a318 proof-10 inside.sh 329e5e0b9c188ef48ad76b6170f432673d1fe14877f8dd6253b735c27661fef5
# Changes: overlay only tests/independent.rs, databases revsrc/revmirror, no generate-lockfile.
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
export OO_REVIEW_NOTE=/evidence/independent-notes.txt

mkdir -p /work/tmp /evidence /work/src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
uname -a
id

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

if [ ! -f /fixed/tools/local-pg-mirror/Cargo.lock ] || [ ! -f /oracle/independent.rs ]; then
  echo "archive or oracle is missing" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
rm -rf /work/src
mkdir -p /work/src
cp -R /fixed/. /work/src/
cp /oracle/independent.rs /work/src/tools/local-pg-mirror/tests/independent.rs
cd /work/src/tools/local-pg-mirror
cp /work/src/Cargo.lock /evidence/root-Cargo.lock.before
sha256sum tests/independent.rs > /evidence/oracle-sha256.txt
cp tests/independent.rs /evidence/independent.rs
printf '%s\n' 0 > /evidence/source.exit

set +e
if [ ! -f Cargo.lock ]; then
  echo "committed tool lock is absent" >&2
  printf '%s\n' 2 > /evidence/generate-lockfile.exit
  exit 2
fi
printf '%s\n' "committed lock present; cargo generate-lockfile was not run" > /evidence/generate-lockfile.note
printf '%s\n' 0 > /evidence/generate-lockfile.exit
cp Cargo.lock /evidence/Cargo.lock
sha256sum Cargo.lock > /evidence/lock-sha256.txt

rustc --edition 2024 -D warnings -o /usr/local/bin/oo-a335-lock-subset /lock_subset.rs > /evidence/lock-subset-rustc.stdout 2> /evidence/lock-subset-rustc.stderr
subset_rustc=$?
printf '%s\n' "$subset_rustc" > /evidence/lock-subset-rustc.exit
echo "PHASE lock_subset_rustc_exit=$subset_rustc"
if [ "$subset_rustc" -ne 0 ]; then
  exit "$subset_rustc"
fi

allowed=(/work/src/Cargo.lock)
while IFS= read -r path; do
  allowed+=("$path")
done < <(find /work/src/tools -name Cargo.lock -type f ! -path '/work/src/tools/local-pg-mirror/*' | sort)
oo-a335-lock-subset /work/src/tools/local-pg-mirror/Cargo.lock "${allowed[@]}" > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
subset_exit=$?
printf '%s\n' "$subset_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$subset_exit"
if [ "$subset_exit" -ne 0 ]; then
  exit "$subset_exit"
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
  "host": "oo-a335-pg",
  "port": 5432,
  "server_name": "oo-a335-pg",
  "source_database": "revsrc",
  "target_database": "revmirror",
  "admin_user": "oo_fixture_admin",
  "source_user": "oo_rev_src",
  "runtime_user": "oo_rev_runtime",
  "reader_user": "oo_rev_read",
  "admin_password": "/secrets/admin.pass",
  "source_password": "/secrets/src.pass",
  "runtime_password": "/secrets/runtime.pass",
  "reader_password": "/secrets/reader.pass",
  "ca": "/secrets/ca.crt"
}
EOF

echo "=== independent test ==="
cargo test --locked --offline -j 1 --test independent -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
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

echo "=== clippy component ==="
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
printf '%s\n' "$clippy_add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$clippy_add_exit"
if [ "$clippy_add_exit" -ne 0 ]; then
  exit "$clippy_add_exit"
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
