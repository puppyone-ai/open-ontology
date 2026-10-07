#!/bin/bash
# Focused Linux arm64 Rust 1.94 smoke of the pinned answer-citations module.
# The tree is git archive 96afe70 only. ontology-dialogue is not in this archive.
set -u
echo "oo_task=sp11-ontology-dialogue-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a300"
echo "oo_fixed=96afe70fd68a56ae8eec7504bf20e09ecd3cdf65"
echo "oo_toolchain_request=1.94.0"
echo "phase=smoke"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ -d tools/ontology-dialogue ]; then
  echo "smoke archive contains ontology-dialogue" >&2
  exit 2
fi
if [ ! -f tools/answer-citations/Cargo.lock ] || [ ! -f tools/answer-citations/src/lib.rs ]; then
  echo "answer-citations baseline is missing" >&2
  exit 2
fi
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml /tmp/rust-toolchain.toml.aside
fi
mkdir -p /evidence/linux/smoke/phases
record() {
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}
echo "=== cargo fetch answer-citations --locked ==="
set +e
cargo fetch --manifest-path tools/answer-citations/Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"
echo "=== cargo test answer-citations --locked --offline ==="
set +e
cargo test --manifest-path tools/answer-citations/Cargo.toml --locked --offline > /evidence/linux/smoke/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
tail -30 /evidence/linux/smoke/test.log
exit "$test_exit"
