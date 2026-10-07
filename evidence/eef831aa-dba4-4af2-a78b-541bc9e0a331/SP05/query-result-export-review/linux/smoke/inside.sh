#!/bin/bash
# Focused smoke on the immutable 15558917 archive before any review overlay.
# Linux arm64 Rust 1.94. No cargo fetch, cargo test, cargo update, or PostgreSQL.
set -u
echo "oo_task=sp05-query-result-export-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a331"
echo "oo_fixed=15558917e2c5be12b74dd1d64fb542b7d05a03af"
echo "oo_platform=linux/arm64"
echo "phase=smoke"
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
rustup component list --installed > /evidence/linux/smoke/components.txt
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
if [ "$sha" != "15558917e2c5be12b74dd1d64fb542b7d05a03af" ]; then
  echo "archive sha mismatch" >&2
  record source 2
fi
if [ ! -f tools/query-result-export/Cargo.toml ] || [ ! -f tools/query-result-export/src/lib.rs ]; then
  echo "committed query-result-export is absent" >&2
  record source 2
fi
if [ -e /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a331 ]; then
  echo "review evidence was inside the fixed archive" >&2
  record source 2
fi
if [ ! -f contracts/gateway/query-pg-cli-v1.md ] || [ ! -f crates/oo-gateway/src/bin/query-pg.rs ]; then
  echo "query-pg producer is absent" >&2
  record source 2
fi
if ! grep -q 'kind` is `null`, `bool`, `int`, or `text`' contracts/gateway/query-pg-cli-v1.md; then
  echo "producer kind sentence is absent" >&2
  record source 2
fi
if ! grep -q 'enum OutCell' crates/oo-gateway/src/bin/query-pg.rs; then
  echo "OutCell is absent" >&2
  record source 2
fi
if grep -q 'rename = "decimal"' crates/oo-gateway/src/bin/query-pg.rs; then
  echo "OutCell has a decimal arm" >&2
  record source 2
fi
if ! grep -q '对以 `= + - @` 开头的单元格加前缀' docs/10-security.md; then
  echo "formula prefix sentence is absent" >&2
  record source 2
fi
if ! grep -q '前导空白/控制字符' specs/open-ontology-v0.1-spec.md; then
  echo "REQ-SEC-02 formula sentence is absent" >&2
  record source 2
fi
if ! grep -F 'E44 / FILE-D04' specs/open-ontology-v0.1-plan.md | grep -q 'exactdecimal'; then
  echo "frozen E44 exactdecimal line is absent" >&2
  record source 2
fi
cmp -s contracts/gateway/query-pg-cli-v1.md /evidence/source/blobs/query-pg-cli-v1.md
record blob-query-pg-contract $?
cmp -s crates/oo-gateway/src/bin/query-pg.rs /evidence/source/blobs/query-pg.rs
record blob-query-pg $?
cmp -s docs/10-security.md /evidence/source/blobs/10-security.md
record blob-security $?
cmp -s tools/query-result-export/CONTRACT.md /evidence/source/blobs/query-result-export-CONTRACT.md
record blob-contract $?
cmp -s tools/query-result-export/src/csv.rs /evidence/source/blobs/csv.rs
record blob-csv $?
cmp -s tools/query-result-export/src/lib.rs /evidence/source/blobs/lib.rs
record blob-lib $?
FIXTURE=tools/query-result-export/tests/fixtures/query-clerk-order_doc-star.stdout
RECEIPT=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a226/SP11/reviewed-pg-composition/live/steps/query-clerk-order_doc-star.stdout
cmp -s "$FIXTURE" "$RECEIPT"
printf '%s\n' "$?" > /evidence/linux/smoke/fixture-cmp.exit
if [ "$(cat /evidence/linux/smoke/fixture-cmp.exit)" != "0" ]; then
  echo "committed fixture differs from the compiled query-pg receipt" >&2
  record fixture 2
fi
record fixture 0
sha256 file "$FIXTURE" > /evidence/linux/smoke/fixture-sha256.txt
if ! grep -q '54cabda641bf1a2128e0246a78eb1a761895a0aa5a4226d136ad8aec399ad6eb' /evidence/linux/smoke/fixture-sha256.txt; then
  echo "fixture sha256 mismatch" >&2
  record fixture-sha 1
fi
record fixture-sha 0
{
  echo "oid_allowed:"
  grep -n -A 2 'fn oid_allowed' crates/oo-gateway/src/query_executor.rs
  echo "outcell:"
  grep -n -A 16 'enum OutCell' crates/oo-gateway/src/bin/query-pg.rs
} > /evidence/linux/smoke/producer-kinds.txt
if ! grep -q 'OID_BOOL | OID_INT8 | OID_INT2 | OID_INT4 | OID_TEXT' /evidence/linux/smoke/producer-kinds.txt; then
  echo "oid allow-list changed" >&2
  record producer 2
fi
if grep -q '1700' /evidence/linux/smoke/producer-kinds.txt; then
  echo "1700 appeared in the recorded allow-list" >&2
  record producer 2
fi
record producer 0
record source 0

sha256 file \
  contracts/gateway/query-pg-cli-v1.md \
  crates/oo-gateway/src/bin/query-pg.rs \
  docs/10-security.md \
  tools/query-result-export/CONTRACT.md \
  tools/query-result-export/src/csv.rs \
  tools/query-result-export/src/lib.rs \
  > /evidence/linux/smoke/source-sha256.txt
record source-hash $?
printf '%s\n' "no-overlay" > /evidence/linux/smoke/overlay-absent.txt
printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "smoke_done"
