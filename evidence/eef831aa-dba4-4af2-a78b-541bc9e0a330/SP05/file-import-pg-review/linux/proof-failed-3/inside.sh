#!/bin/bash
# Review proof. Fixed archive at /fixed already contains tools/file-import-pg.
# Author refuse, clippy, and live run before the review oracle is copied in.
# Origin: a204 preoverlay inside.sh b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636
# and a308 file-import inside.sh a9abf3e0fd3e9cece76eaedcb75e7ba1646bf870f603091fe6a278676068eef6.
# Changes: archive includes the committed tool and its lock, commands stay --locked,
# author live uses this task's Postgres via OO_A308_PG_HOST, then the review oracle
# is copied and run. The lock is not copied back to the product tree.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export TMPDIR=/work/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export PATH="/cargo-home/bin:${PATH}"

mkdir -p /work/tmp /evidence/phases /evidence/source /cargo-home/bin

phase() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

run_phase() {
  name=$1
  shift
  set +e
  "$@" > "/evidence/${name}.stdout" 2> "/evidence/${name}.stderr"
  code=$?
  phase "$name" "$code"
  return "$code"
}

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
uname -a
id

if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  phase version 2
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  phase version 2
  exit 2
fi
phase version 0

echo "=== fixed archive ==="
if [ ! -f /fixed/Cargo.toml ] || [ ! -f /fixed/Cargo.lock ] || [ ! -f /fixed/crates/oo-connectors/src/bin/oo-file-parser-worker.rs ]; then
  echo "fixed archive is incomplete" >&2
  phase source 2
  exit 2
fi
if [ ! -f /fixed/tools/file-import-pg/Cargo.lock ] || [ ! -f /fixed/tools/file-import-pg/src/lib.rs ] || [ ! -f /fixed/tools/file-import-pg/tests/live.rs ]; then
  echo "fixed tool is incomplete" >&2
  phase source 2
  exit 2
fi
if [ -e /fixed/tools/file-import-pg/tests/review_oracle.rs ]; then
  echo "fixed archive contains the review overlay" >&2
  phase source 2
  exit 2
fi
rm -rf /work/src
mkdir -p /work/src
cp -R /fixed/. /work/src/
rm -rf /work/src/tools/file-import-pg/target
cd /work/src/tools/file-import-pg
if [ -e tests/review_oracle.rs ]; then
  echo "work tree contains the review overlay before author tests" >&2
  phase source 2
  exit 2
fi
rm -rf /evidence/source/fixed-crate
mkdir -p /evidence/source/fixed-crate
cp -R Cargo.toml Cargo.lock CONTRACT.md src tests /evidence/source/fixed-crate/
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort > /work/tmp/tool-files.txt
phase source 0

echo "=== sha256 tool ==="
if ! run_phase sha256-tool rustc --edition 2021 -D warnings -o /cargo-home/bin/sha256 /tools/sha256.rs; then
  exit "$(cat /evidence/phases/sha256-tool.exit)"
fi
: > /evidence/source-sha256.txt
while IFS= read -r file; do
  /cargo-home/bin/sha256 file "/work/src/tools/file-import-pg/${file}" >> /evidence/source-sha256.txt || exit 1
done < /work/tmp/tool-files.txt
cp Cargo.lock /evidence/Cargo.lock
/cargo-home/bin/sha256 file /work/src/tools/file-import-pg/Cargo.lock > /evidence/lock-sha256.txt

echo "=== fetch workspace ==="
cd /work/src
run_phase fetch-offline cargo fetch --locked --offline
if [ "$(cat /evidence/phases/fetch-offline.exit)" -ne 0 ]; then
  if ! run_phase fetch cargo fetch --locked; then
    exit "$(cat /evidence/phases/fetch.exit)"
  fi
else
  echo "offline fetch satisfied" > /evidence/fetch.stdout
  : > /evidence/fetch.stderr
  phase fetch 0
fi

echo "=== fetch tool ==="
cd /work/src/tools/file-import-pg
run_phase fetch-tool-offline cargo fetch --locked --offline
if [ "$(cat /evidence/phases/fetch-tool-offline.exit)" -ne 0 ]; then
  if ! run_phase fetch-tool cargo fetch --locked; then
    exit "$(cat /evidence/phases/fetch-tool.exit)"
  fi
else
  echo "offline tool fetch satisfied" > /evidence/fetch-tool.stdout
  : > /evidence/fetch-tool.stderr
  phase fetch-tool 0
fi

echo "=== worker ==="
cd /work/src
export CARGO_TARGET_DIR=/target
if ! run_phase worker-build cargo build --locked --offline --bin oo-file-parser-worker -p oo-connectors; then
  exit "$(cat /evidence/phases/worker-build.exit)"
fi
if [ ! -f /target/debug/oo-file-parser-worker ]; then
  echo "worker binary is absent" >&2
  phase worker-hash 2
  exit 2
fi
/cargo-home/bin/sha256 file /target/debug/oo-file-parser-worker > /evidence/worker.sha256
phase worker-hash 0

echo "=== author refuse ==="
cd /work/src/tools/file-import-pg
export CARGO_TARGET_DIR=/target/file-import
if ! run_phase refuse cargo test --locked --offline --test refuse -- --test-threads=1; then
  exit "$(cat /evidence/phases/refuse.exit)"
fi
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "refuse rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  phase lock-stable 2
  exit 2
fi

echo "=== lock subset ==="
if ! run_phase lock-subset-rustc rustc --edition 2024 -D warnings -o /cargo-home/bin/oo-a330-lock-subset /lock_subset.rs; then
  exit "$(cat /evidence/phases/lock-subset-rustc.exit)"
fi
if ! run_phase lock-subset oo-a330-lock-subset /work/src/tools/file-import-pg/Cargo.lock /work/src/Cargo.lock; then
  exit "$(cat /evidence/phases/lock-subset.exit)"
fi

echo "=== clippy before overlay ==="
if ! run_phase clippy-add rustup component add clippy --toolchain 1.94.0; then
  exit "$(cat /evidence/phases/clippy-add.exit)"
fi
if ! run_phase clippy cargo clippy --locked --offline --all-targets -- -D warnings; then
  exit "$(cat /evidence/phases/clippy.exit)"
fi
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "clippy rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  phase lock-stable 2
  exit 2
fi

echo "=== author live ==="
export OO_A308_LIVE=1
export OO_A308_PG_HOST=oo-a330-pg
export OO_A308_PG_PORT=5432
export OO_A308_ADMIN_USER=oo_fixture_admin
export OO_A308_ADMIN_PASS_FILE=/secrets/admin.pass
export OO_A308_RUNTIME_PASS_FILE=/secrets/runtime.pass
export OO_A308_AGENT_PASS_FILE=/secrets/agent.pass
export OO_A308_CA_FILE=/secrets/ca.crt
export OO_A308_WORKER=/target/debug/oo-file-parser-worker
if ! run_phase live cargo test --locked --offline --test live -- --test-threads=1 --nocapture; then
  exit "$(cat /evidence/phases/live.exit)"
fi
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "live rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  phase lock-stable 2
  exit 2
fi

echo "=== review overlay ==="
if [ ! -f /evidence/review_oracle.rs ]; then
  echo "review oracle is absent" >&2
  phase review-copy 2
  exit 2
fi
cp /evidence/review_oracle.rs /work/src/tools/file-import-pg/tests/review_oracle.rs
phase review-copy 0
export OO_A330_REVIEW=1
export OO_A330_PG_HOST=oo-a330-pg
export OO_A330_PG_PORT=5432
export OO_A330_ADMIN_PASS_FILE=/secrets/admin.pass
export OO_A330_RUNTIME_PASS_FILE=/secrets/runtime.pass
export OO_A330_AGENT_PASS_FILE=/secrets/agent.pass
export OO_A330_CA_FILE=/secrets/ca.crt
export OO_A330_WORKER=/target/debug/oo-file-parser-worker
export OO_A330_ORACLE=/evidence/oracle.txt
run_phase review cargo test --locked --offline --test review_oracle -- --test-threads=1 --nocapture
review_exit=$(cat /evidence/phases/review.exit)
run_phase clippy-review cargo clippy --locked --offline --test review_oracle -- -D warnings
clippy_review_exit=$(cat /evidence/phases/clippy-review.exit)
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "review rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  phase lock-stable 2
  exit 2
fi
phase lock-stable 0
if [ "$review_exit" -ne 0 ]; then
  exit "$review_exit"
fi
exit "$clippy_review_exit"
