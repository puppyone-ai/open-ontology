#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 focused smoke of fixed commit 0bcd494.
# The archive must not contain tools/enum-history. No PostgreSQL. No cargo update.
set -u
echo "oo_task=sp08-enum-history-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a270"
echo "oo_fixed=0bcd494457091d4b55c82fc6b9922b5bc5dc44c4"
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
mkdir -p /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -R /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt

record() {
  printf '%s\n' "$2" > "/evidence/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
rustc -vV > /evidence/rustc-verbose.txt
case "$got" in
  "rustc 1.94.0 "*) record version 0 ;;
  *) echo "refusing toolchain: $got" >&2; record version 2; exit 2 ;;
esac

if [ -e tools/enum-history ]; then
  echo "enum-history is already on the fixed archive" >&2
  record absence 2
  exit 2
fi
printf '%s\n' "enum-history absent" > /evidence/absence.txt
record absence 0

if [ ! -f examples/schema/enum-ysk-zt.yaml ]; then
  echo "archive missing enum example" >&2
  record example-hash 2
  exit 2
fi
sha256sum examples/schema/enum-ysk-zt.yaml crates/oo-core/src/oos/parse.rs crates/oo-core/src/oos/model.rs > /evidence/archive-sha256.txt
example_hash=$(sha256sum examples/schema/enum-ysk-zt.yaml | awk '{print $1}')
if [ "$example_hash" != "07d7e26d32edaba62bc2287cfec6c22816c78ce784b22b5df42ef7d437214839" ]; then
  echo "enum example hash drifted: $example_hash" >&2
  record example-hash 2
  exit 2
fi
record example-hash 0
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== cargo fetch locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
record fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  exit "$fetch_exit"
fi

echo "=== focused enum parse smoke ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-core --test schema_contract documented_oos_examples_parse -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
record test "$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
printf '%s\n' 0 > /evidence/smoke.exit
echo "preoverlay=ok"
exit 0
