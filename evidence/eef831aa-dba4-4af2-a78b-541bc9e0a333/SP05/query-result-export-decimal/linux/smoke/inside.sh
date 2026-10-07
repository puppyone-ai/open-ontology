#!/bin/bash
# Focused pre-edit smoke of fixed 63566b60. Linux arm64 Rust 1.94.
# The crate is already in the archive. No overlay. No cargo fetch.
# No cargo update. No clippy. No PostgreSQL.
set -u
echo "oo_task=sp05-query-result-export-decimal-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a333"
echo "oo_fixed=63566b60aabdf29a29bfe55c4866edf3c4081cfd"
echo "oo_platform=linux/arm64"
echo "phase=smoke"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_NET_OFFLINE=true
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/smoke/phases /cargo-home/bin
cd /src
printf '%s\n' "not-invoked" > /evidence/linux/smoke/cargo-update-invoked.txt

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
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
printf 'abc' > /cargo-home/sha256-abc.txt
/cargo-home/bin/sha256 file /cargo-home/sha256-abc.txt > /evidence/linux/smoke/sha256-self.txt
record sha256-self $?
if ! grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' /evidence/linux/smoke/sha256-self.txt; then
  echo "sha256 abc vector mismatch" >&2
  record sha256-vector 1
fi
record sha256-vector 0
export PATH="/cargo-home/bin:${PATH}"

sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/smoke/fixed-base.txt
if [ "$sha" != "63566b60aabdf29a29bfe55c4866edf3c4081cfd" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f tools/query-result-export/Cargo.toml ] || [ ! -f tools/query-result-export/src/lib.rs ]; then
  echo "committed query-result-export is absent" >&2
  record source 2
fi
if [ -e /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a333 ]; then
  echo "this session evidence was inside the fixed archive" >&2
  record source 2
fi
if ! grep -q 'Exact decimal' tools/query-result-export/CONTRACT.md; then
  echo "pre-edit narrowing sentence is absent" >&2
  record source 2
fi
if ! grep -q 'rename = "decimal"' crates/oo-gateway/src/bin/query-pg.rs; then
  echo "producer decimal variant is absent from the fixed archive" >&2
  record source 2
fi
if ! grep -q '"kind":"decimal"' execution/decisions/numeric-query-contract-a327.md; then
  echo "a327 decimal cell is absent from the fixed archive" >&2
  record source 2
fi
cmp -s tools/query-result-export/CONTRACT.md /evidence/source/blobs/query-result-export-CONTRACT.md
record blob-contract $?
cmp -s tools/query-result-export/src/csv.rs /evidence/source/blobs/csv.rs
record blob-csv $?
cmp -s execution/decisions/numeric-query-contract-a327.md /evidence/source/blobs/numeric-query-contract-a327.md
record blob-a327-contract $?
/cargo-home/bin/sha256 file /evidence/freeze/AMENDMENT.md > /evidence/linux/smoke/amendment-sha256.txt
record amendment-hash $?
record source 0

cargo test --locked --offline --manifest-path tools/query-result-export/Cargo.toml -- --test-threads=1 \
  > /evidence/linux/smoke/test.log 2>&1
record test $?
printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "smoke_done"
