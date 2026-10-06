#!/bin/bash
# Focused pre-edit smoke on the immutable 567d621e archive.
# Linux arm64 Rust 1.94. No answer-citations overlay. No cargo fetch.
# No cargo update. No apt. No PostgreSQL. No libclang.
set -u
echo "oo_task=sp11-answer-citations-preedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a266"
echo "oo_fixed=567d621e2e52b7e0011bdb025dc9337e92ecc5a6"
echo "oo_platform=linux/arm64"
echo "phase=preedit"
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
if [ "$sha" != "567d621e2e52b7e0011bdb025dc9337e92ecc5a6" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ -e tools/answer-citations ]; then
  echo "answer-citations overlay is already on the pre-edit archive" >&2
  record source 2
fi
if [ ! -f contracts/gateway/query-pg-cli-v1.md ] || [ ! -f crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "query-pg producer is absent" >&2
  record source 2
fi
if ! grep -q 'oo.gateway.query-pg-audit/v1' contracts/gateway/query-pg-cli-v1.md; then
  echo "audit protocol is absent from the producer contract" >&2
  record source 2
fi
if ! grep -q 'struct AdmitLine' crates/oo-gateway/src/bin/query-pg.rs; then
  echo "admit record is absent from query-pg" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/linux/smoke/overlay-absent.txt
cmp -s contracts/gateway/query-pg-cli-v1.md /evidence/source/blobs/query-pg-cli-v1.md
record blob-query-pg-contract $?
cmp -s crates/oo-gateway/src/bin/query-pg.rs /evidence/source/blobs/query-pg.rs
record blob-query-pg $?
record source 0

/cargo-home/bin/sha256 file \
  contracts/gateway/query-pg-cli-v1.md \
  crates/oo-gateway/src/bin/query-pg.rs \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "smoke_done"
