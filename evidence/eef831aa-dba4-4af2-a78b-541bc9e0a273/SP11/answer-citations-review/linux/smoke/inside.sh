#!/bin/bash
# Focused pre-overlay smoke on the immutable 9951cda archive.
# Linux arm64 Rust 1.94. No review harness. No cargo fetch.
# No cargo test. No cargo update. No apt. No PostgreSQL.
set -u
echo "oo_task=sp11-answer-citations-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a273"
echo "oo_fixed=9951cda7f128e30f1f6ce02e409d542d20a9e4fb"
echo "oo_reviewed=7e404dd3d36cdeecb553ab0c707dbfe82ba94c62"
echo "oo_platform=linux/arm64"
echo "phase=smoke"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/tmp/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/smoke/phases /cargo-home/bin
cd /src
printf '%s\n' "not-invoked" > /evidence/linux/smoke/cargo-update-invoked.txt
printf '%s\n' 0 > /evidence/linux/smoke/pg.txt

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
printf 'abc' > /tmp/sha256-abc.txt
/cargo-home/bin/sha256 file /tmp/sha256-abc.txt > /evidence/linux/smoke/sha256-self.txt
record sha256-self $?
if ! grep -q 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad' /evidence/linux/smoke/sha256-self.txt; then
  echo "sha256 abc vector mismatch" >&2
  record sha256-vector 1
fi
record sha256-vector 0
export PATH="/cargo-home/bin:${PATH}"

sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/smoke/fixed-base.txt
if [ "$sha" != "9951cda7f128e30f1f6ce02e409d542d20a9e4fb" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f tools/answer-citations/Cargo.toml ] || [ ! -f tools/answer-citations/src/lib.rs ]; then
  echo "answer-citations is absent from the fixed archive" >&2
  record source 2
fi
if [ -e tools/answer-citations/tests/a273_review.rs ]; then
  echo "review harness is already on the archive" >&2
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
if ! grep -q 'does not contain SQL' contracts/gateway/query-pg-cli-v1.md; then
  echo "producer contract does not state the SQL absence" >&2
  record source 2
fi
if ! grep -q '引用与可追溯' docs/08-agent-interface.md; then
  echo "docs08 citation section is absent" >&2
  record source 2
fi
printf '%s\n' "harness_absent" > /evidence/linux/smoke/overlay-absent.txt
cmp -s contracts/gateway/query-pg-cli-v1.md /evidence/source/blobs/query-pg-cli-v1.md
record blob-query-pg-contract $?
cmp -s crates/oo-gateway/src/bin/query-pg.rs /evidence/source/blobs/query-pg.rs
record blob-query-pg $?
cmp -s tools/answer-citations/tests/fixtures/desk-explicit.audit.jsonl /evidence/source/blobs/desk-explicit-a242.audit.jsonl
record fixture-cmp $?
cmp -s tools/answer-citations/CONTRACT.md /evidence/source/blobs/CONTRACT.md
record blob-contract $?
record source 0

rustc --edition 2021 -D warnings /tools/audit_fields.rs -o /cargo-home/bin/audit-fields
record audit-fields-tool $?
/cargo-home/bin/audit-fields crates/oo-gateway/src/bin/query-pg.rs \
  > /evidence/linux/smoke/audit-fields.txt \
  2> /evidence/linux/smoke/audit-fields.err
record audit-fields $?

rustc --edition 2021 -D warnings /tools/lock_subset.rs -o /cargo-home/bin/lock-subset
record lock-subset-tool $?
/cargo-home/bin/lock-subset Cargo.lock tools/answer-citations/Cargo.lock \
  > /evidence/linux/smoke/lock-subset.txt \
  2> /evidence/linux/smoke/lock-subset.err
record lock-subset $?

sha256 file \
  tools/answer-citations/tests/fixtures/desk-explicit.audit.jsonl \
  /evidence/source/blobs/desk-explicit-a242.audit.jsonl \
  > /evidence/linux/smoke/fixture-sha256.txt
record fixture-sha $?
if ! grep -q '8efbfb0748ad65073a2204909d8b79310c7006161e924893d11b15399d624783' /evidence/linux/smoke/fixture-sha256.txt; then
  echo "fixture sha mismatch" >&2
  record fixture-sha-value 1
fi
want_lines=$(grep -c '8efbfb0748ad65073a2204909d8b79310c7006161e924893d11b15399d624783' /evidence/linux/smoke/fixture-sha256.txt)
if [ "$want_lines" != "2" ]; then
  echo "fixture sha did not match both copies" >&2
  record fixture-sha-value 1
fi
record fixture-sha-value 0

sha256 dir /evidence/linux/smoke/product-archive.txt /src/tools/answer-citations
record product-hash $?

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "smoke_done"
