#!/bin/bash
# Linux arm64 Rust 1.94 focused tests and Clippy for file-import-pg.
# Fixed archive at /fixed. Product overlay at /overlay. One private PostgreSQL.
# Origin: evidence/.../a301/SP04/cdc-apply-pg/linux/proof/inside.sh
# That file records a291 impl-7 inside sha256 9529fe516e5d1eaddca29bd0939d5140aa36d37e2df784a33da3509e8a4d7a52.
# Changes: this child has no lock yet, so the first offline refuse test writes it.
# Subset check is the tool lock against the root lock only. CARGO_HOME=/cargo-home.
# The worker binary is built from the fixed archive before the tool tests.
# Live test uses OO_A308_* against oo-a308-pg. No cargo update.
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
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
id

echo "=== rustc version gate ==="
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
if [ -e /fixed/tools/file-import-pg ]; then
  echo "fixed archive already contains file-import-pg" >&2
  phase source 2
  exit 2
fi
if [ -f /overlay/Cargo.lock ]; then
  echo "overlay must not carry a Cargo.lock" >&2
  phase source 2
  exit 2
fi
rm -rf /work/src
mkdir -p /work/src
cp -R /fixed/. /work/src/
mkdir -p /work/src/tools/file-import-pg
cp -R /overlay/. /work/src/tools/file-import-pg/
rm -rf /work/src/tools/file-import-pg/target
if [ -e /work/src/tools/file-import-pg/Cargo.lock ]; then
  echo "overlay copy brought a Cargo.lock" >&2
  phase source 2
  exit 2
fi
cd /work/src/tools/file-import-pg
if [ ! -f Cargo.toml ] || [ ! -f src/lib.rs ] || [ ! -f src/main.rs ] || [ ! -f CONTRACT.md ] || [ ! -f tests/live.rs ] || [ ! -f tests/refuse.rs ]; then
  echo "product crate is incomplete" >&2
  phase source 2
  exit 2
fi
find Cargo.toml CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
cp -R Cargo.toml CONTRACT.md src tests /evidence/source/
phase source 0

echo "=== sha256 tool ==="
if [ ! -x /cargo-home/bin/sha256 ]; then
  if ! run_phase sha256-tool rustc --edition 2021 -D warnings -o /cargo-home/bin/sha256 /tools/sha256.rs; then
    exit "$(cat /evidence/phases/sha256-tool.exit)"
  fi
else
  phase sha256-tool 0
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

echo "=== refuse (writes the tool lock) ==="
cd /work/src/tools/file-import-pg
export CARGO_TARGET_DIR=/target/file-import
run_phase refuse cargo test --offline --test refuse -- --test-threads=1
refuse_exit=$(cat /evidence/phases/refuse.exit)
if [ -f Cargo.lock ]; then
  cp Cargo.lock /evidence/Cargo.lock
  sha256sum Cargo.lock > /evidence/lock-sha256.txt
else
  echo "refuse did not write Cargo.lock" >&2
  phase lock-present 2
  exit 2
fi
phase lock-present 0

echo "=== lock subset compiler ==="
if ! run_phase lock-subset-rustc rustc --edition 2024 -D warnings -o /cargo-home/bin/oo-a308-lock-subset /lock_subset.rs; then
  exit "$(cat /evidence/phases/lock-subset-rustc.exit)"
fi

echo "=== lock subset ==="
run_phase lock-subset oo-a308-lock-subset /work/src/tools/file-import-pg/Cargo.lock /work/src/Cargo.lock
subset_exit=$(cat /evidence/phases/lock-subset.exit)
if [ "$refuse_exit" -ne 0 ] || [ "$subset_exit" -ne 0 ]; then
  if [ "$refuse_exit" -ne 0 ]; then
    exit "$refuse_exit"
  fi
  exit "$subset_exit"
fi

echo "=== clippy component ==="
if ! run_phase clippy-add rustup component add clippy --toolchain 1.94.0; then
  exit "$(cat /evidence/phases/clippy-add.exit)"
fi

echo "=== clippy ==="
run_phase clippy cargo clippy --locked --offline --all-targets -- -D warnings
clippy_exit=$(cat /evidence/phases/clippy.exit)
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "clippy rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  phase lock-stable 2
  exit 2
fi

echo "=== live ==="
export OO_A308_LIVE=1
export OO_A308_PG_HOST=oo-a308-pg
export OO_A308_PG_PORT=5432
export OO_A308_ADMIN_USER=oo_fixture_admin
export OO_A308_ADMIN_PASS_FILE=/secrets/admin.pass
export OO_A308_RUNTIME_PASS_FILE=/secrets/runtime.pass
export OO_A308_AGENT_PASS_FILE=/secrets/agent.pass
export OO_A308_CA_FILE=/secrets/ca.crt
export OO_A308_WORKER=/target/debug/oo-file-parser-worker
run_phase live cargo test --locked --offline --test live -- --test-threads=1 --nocapture
live_exit=$(cat /evidence/phases/live.exit)
if ! cmp -s Cargo.lock /evidence/Cargo.lock; then
  echo "live rewrote Cargo.lock" >&2
  cp Cargo.lock /evidence/Cargo.lock.rewritten
  phase lock-stable 2
  exit 2
fi
phase lock-stable 0
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi
exit "$live_exit"
