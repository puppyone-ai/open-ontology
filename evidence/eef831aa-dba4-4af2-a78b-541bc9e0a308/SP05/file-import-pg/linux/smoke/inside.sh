#!/bin/bash
# Focused compiler smoke on the immutable 72dc9e2 archive, before any file-import-pg overlay.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL. No libclang.
# The archive toolchain file names 1.98.1. RUSTUP_TOOLCHAIN forces the pinned 1.94.0 image.
# MSRV in the workspace manifest is 1.94.0. This smoke does not install another toolchain.
set -u
echo "oo_task=sp05-file-import-pg-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a308"
echo "oo_fixed=72dc9e2db841de4de368216bbbe13d738f850ea9"
echo "oo_platform=linux/arm64"
echo "phase=preoverlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/smoke/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/smoke/phases
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke/rustc-version.txt
rustc -Vv > /evidence/linux/smoke/rustc-verbose.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
rustup show > /evidence/linux/smoke/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
if ! grep -q '1.94.0' /evidence/linux/smoke/rustup-show.txt; then
  echo "rustup show did not report 1.94.0" >&2
  record toolchain 2
fi
record toolchain 0
printf '%s\n' "archive rust-toolchain.toml channel is 1.98.1; smoke forces RUSTUP_TOOLCHAIN=1.94.0" \
  > /evidence/linux/smoke/toolchain-override.txt

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-connectors/tests/file_parser_worker.rs ] \
  || [ ! -f crates/oo-connectors/src/bin/oo-file-parser-worker.rs ] \
  || [ ! -f crates/oo-connectors/src/file_worker_parent.rs ]; then
  echo "fixed archive is missing the file worker" >&2
  record source 2
fi
if [ -e tools/file-import-pg ]; then
  echo "file-import-pg overlay is already on the pre-overlay archive" >&2
  record source 2
fi
if ! grep -q 'fn sha256sum_matches_the_empty_vector' crates/oo-connectors/tests/file_parser_worker.rs; then
  echo "focused smoke test is absent" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/linux/smoke/overlay-absent.txt
cmp -s crates/oo-connectors/tests/file_parser_worker.rs /evidence/source/blobs/file_parser_worker.rs
record blob-cmp $?

/cargo-home/bin/sha256 file \
  crates/oo-connectors/src/bin/oo-file-parser-worker.rs \
  crates/oo-connectors/tests/file_parser_worker.rs \
  crates/oo-connectors/src/file_worker_parent.rs \
  crates/oo-connectors/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  rust-toolchain.toml \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?
printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/smoke/fetch-offline.log 2>&1
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/linux/smoke/phases/fetch-offline.exit
echo "phase_exit=$offline_fetch_exit phase=fetch-offline"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch --locked ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
  fetch_exit=$?
  set -e
else
  cp /evidence/linux/smoke/fetch-offline.log /evidence/linux/smoke/fetch.log
fi
record fetch "$fetch_exit"

echo "=== file_parser_worker sha256sum_matches_the_empty_vector ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors \
  --test file_parser_worker -- --test-threads=1 sha256sum_matches_the_empty_vector \
  > /evidence/linux/smoke/test-file-parser-worker.log 2>&1
test_exit=$?
set -e
record test-file-parser-worker "$test_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/smoke/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "preoverlay_smoke=ok"
exit 0
